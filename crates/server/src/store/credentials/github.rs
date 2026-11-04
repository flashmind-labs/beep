//! Git accounts: GitHub and Forgejo logins and the default among them.

use super::{
    super::Store, ACCOUNT, FORGEJO, GITHUB, GitHubAccountRow, GitLogin, NewGitHubAccount,
    account_from, opens, slot_from,
};
use crate::{
    account::credentials::{CredentialKey, Owner, Slot},
    account::user::UserKey,
};
use anyhow::Result;
use sqlx::Row;
use uuid::Uuid;

impl Store {
    /// Seals and saves a GitHub token as `new.login`, replacing that login's account; the first account becomes the default.
    pub(crate) async fn save_github_account(
        &self,
        user: &UserKey,
        key: &CredentialKey,
        new: NewGitHubAccount<'_>,
    ) -> Result<GitHubAccountRow> {
        let id = Uuid::new_v4().simple().to_string();
        let owner = Owner {
            user: user.as_str(),
            slot: &Slot::GitHub,
            id: &id,
        };
        let sealed = key.seal(&owner, new.secret)?;
        let login_key = new.login.to_ascii_lowercase();
        let mut tx = self.pool.begin().await?;
        // One save of a user's accounts at a time, so two first saves do
        // not both become the default.
        sqlx::query("SELECT id FROM users WHERE id=$1 FOR UPDATE")
            .bind(user.as_str())
            .execute(&mut *tx)
            .await?;
        let accounts: Vec<(String, bool)> = sqlx::query_as(
            "SELECT provider, is_default FROM credentials WHERE user_id=$1 AND kind=$2",
        )
        .bind(user.as_str())
        .bind(GITHUB)
        .fetch_all(&mut *tx)
        .await?;
        let known = accounts.iter().any(|(login, _)| *login == login_key);
        let mut default = !accounts.iter().any(|(_, default)| *default);
        if new.replace_default && !known && !default {
            sqlx::query("DELETE FROM credentials WHERE user_id=$1 AND kind=$2 AND is_default")
                .bind(user.as_str())
                .bind(GITHUB)
                .execute(&mut *tx)
                .await?;
            default = true;
        }
        let row = sqlx::query(&format!(
            "INSERT INTO credentials (id, user_id, kind, provider, host_url, env_var, to_machines,
                 last4, account, secret, is_default, orgs)
             VALUES ($1, $2, $3, $4, '', $5, true, $6, $7, $8, $9, $10)
             ON CONFLICT (user_id, kind, provider, host_url) DO UPDATE SET id=excluded.id,
                 env_var=excluded.env_var, to_machines=true, last4=excluded.last4,
                 account=excluded.account, secret=excluded.secret, orgs=excluded.orgs,
                 is_default=credentials.is_default OR excluded.is_default, created_at=now()
             RETURNING {ACCOUNT}"
        ))
        .bind(&id)
        .bind(user.as_str())
        .bind(GITHUB)
        .bind(&login_key)
        .bind(Slot::GitHub.env_var().unwrap_or_default())
        .bind(new.last4)
        .bind(new.login)
        .bind(sealed)
        .bind(default)
        .bind(new.orgs)
        .fetch_one(&mut *tx)
        .await?;
        tx.commit().await?;
        account_from(&row, false)
    }

    /// The user's GitHub accounts, default first then oldest; with none marked, the oldest is the default.
    pub(crate) async fn github_accounts(
        &self,
        user: &UserKey,
        key: &CredentialKey,
    ) -> Result<Vec<GitHubAccountRow>> {
        let rows = sqlx::query(&format!(
            "SELECT {ACCOUNT}, secret FROM credentials WHERE user_id=$1 AND kind=$2
             ORDER BY is_default DESC, created_at, id"
        ))
        .bind(user.as_str())
        .bind(GITHUB)
        .fetch_all(&self.pool)
        .await?;
        let mut accounts = rows
            .iter()
            .map(|row| account_from(row, !opens(row, user, key)))
            .collect::<Result<Vec<_>>>()?;
        if let Some(first) = accounts.first_mut() {
            first.default = true;
        }
        Ok(accounts)
    }

    /// Makes GitHub account `id` the default; false when there is no such account.
    pub(crate) async fn set_default_github(&self, user: &UserKey, id: &str) -> Result<bool> {
        let mut tx = self.pool.begin().await?;
        let found: Option<String> = sqlx::query_scalar(
            "SELECT id FROM credentials WHERE user_id=$1 AND kind=$2 AND id=$3 FOR UPDATE",
        )
        .bind(user.as_str())
        .bind(GITHUB)
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?;
        if found.is_none() {
            return Ok(false);
        }
        sqlx::query("UPDATE credentials SET is_default=false WHERE user_id=$1 AND kind=$2")
            .bind(user.as_str())
            .bind(GITHUB)
            .execute(&mut *tx)
            .await?;
        sqlx::query("UPDATE credentials SET is_default=true WHERE id=$1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(true)
    }

    /// Keeps `account` as the login of Forgejo credential `id`; false when there is no such one.
    pub(crate) async fn set_forgejo_account(
        &self,
        user: &UserKey,
        id: &str,
        account: &str,
    ) -> Result<bool> {
        let done =
            sqlx::query("UPDATE credentials SET account=$3 WHERE user_id=$1 AND id=$2 AND kind=$4")
                .bind(user.as_str())
                .bind(id)
                .bind(account)
                .bind(FORGEJO)
                .execute(&self.pool)
                .await?;
        Ok(done.rows_affected() > 0)
    }

    /// The user's git host tokens, opened, with their logins; unopenable ones are logged and skipped.
    pub(crate) async fn git_logins(
        &self,
        user: &UserKey,
        key: &CredentialKey,
    ) -> Result<Vec<GitLogin>> {
        let rows = sqlx::query(
            "SELECT id, kind, provider, host_url, account, secret FROM credentials
             WHERE user_id=$1 AND kind IN ('github', 'forgejo') ORDER BY kind, host_url, id",
        )
        .bind(user.as_str())
        .fetch_all(&self.pool)
        .await?;
        let mut out = Vec::new();
        for row in &rows {
            let id: String = row.try_get("id")?;
            let sealed: Vec<u8> = row.try_get("secret")?;
            let account: Option<String> = row.try_get("account")?;
            let opened = slot_from(row).and_then(|slot| {
                let owner = Owner {
                    user: user.as_str(),
                    slot: &slot,
                    id: &id,
                };
                let secret = key.open(&owner, &sealed)?;
                Ok((slot, secret))
            });
            match opened {
                Ok((slot, secret)) => out.push(GitLogin {
                    id,
                    slot,
                    secret,
                    account,
                }),
                Err(e) => log::warn!("credential {id} skipped: {e:#}"),
            }
        }
        Ok(out)
    }
}
