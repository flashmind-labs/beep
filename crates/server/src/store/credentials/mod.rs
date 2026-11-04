//! The credentials a user added, sealed (see `crate::account::credentials`).

mod rows;
use rows::{ACCOUNT, Columns, SHOWN, account_from, columns, credential_from, opens, slot_from};

mod github;
mod tokens;

use super::Store;
use crate::{
    account::{
        credentials::{CredentialKey, Owner},
        user::UserKey,
    },
    protocol::{AgentAccount, ApiProvider, CredentialKind},
};
use anyhow::{Result, bail};
use beep_projects::credentials::Slot;
use chrono::{DateTime, Utc};
use secrecy::SecretString;
use sqlx::{PgConnection, Row};
use uuid::Uuid;

/// A credential as the apps see it: everything but the secret.
#[derive(Debug, Clone, PartialEq)]
pub struct CredentialRow {
    pub id: String,
    /// As stored, so a kind this server does not know (from a newer server) still lists.
    pub kind: CredentialKind,
    pub env_var: String,
    pub last4: String,
    pub account: Option<String>,
    pub to_machines: bool,
    pub created_at: DateTime<Utc>,
    /// The secret no longer opens or the kind is unknown; the user must enter it again.
    pub unreadable: bool,
}

/// A git host token, opened, with who it signs in as.
#[derive(Debug)]
pub struct GitLogin {
    /// `CredentialRow::id`; a new one each time the token is saved.
    pub id: String,
    pub slot: Slot,
    pub secret: SecretString,
    pub account: Option<String>,
}

/// A credential to save, replacing the user's one in the same slot.
pub struct NewCredential<'a> {
    pub slot: &'a Slot,
    pub secret: &'a SecretString,
    pub last4: &'a str,
    pub account: Option<&'a str>,
    pub to_machines: bool,
}

/// A GitHub account as the apps see it: everything but the token.
#[derive(Debug, Clone, PartialEq)]
pub struct GitHubAccountRow {
    pub id: String,
    pub login: String,
    pub orgs: Vec<String>,
    pub default: bool,
    pub last4: String,
    pub created_at: DateTime<Utc>,
    pub unreadable: bool,
}

/// A GitHub token to save as the account it signs in as.
pub struct NewGitHubAccount<'a> {
    pub secret: &'a SecretString,
    pub last4: &'a str,
    pub login: &'a str,
    pub orgs: &'a [String],
    /// With no account of this login, replace the default instead of adding (what pre-multi-account apps mean).
    pub replace_default: bool,
}

const GITHUB: &str = "github";
const FORGEJO: &str = "forgejo";
const API_KEY: &str = "api_key";
const AGENT_TOKEN: &str = "agent_token";

/// A coding agent's token as Agent accounts shows it, without the secret.
#[derive(Debug, Clone, PartialEq)]
pub struct AgentTokenRow {
    pub agent: AgentAccount,
    pub last4: String,
    pub created_at: DateTime<Utc>,
    /// Its secret no longer opens; the user has to sign in again.
    pub unreadable: bool,
}

impl Store {
    /// Seals and saves `new`, replacing the user's credential in its slot.
    pub(crate) async fn save_credential(
        &self,
        user: &UserKey,
        key: &CredentialKey,
        new: NewCredential<'_>,
    ) -> Result<CredentialRow> {
        if *new.slot == Slot::GitHub {
            bail!("a GitHub token is saved as its account, with save_github_account");
        }
        let id = Uuid::new_v4().simple().to_string();
        let owner = Owner {
            user: user.as_str(),
            slot: new.slot,
            id: &id,
        };
        let sealed = key.seal(&owner, new.secret)?;
        let Columns {
            kind,
            provider,
            host_url,
        } = columns(new.slot);
        let row = sqlx::query(&format!(
            "INSERT INTO credentials (id, user_id, kind, provider, host_url, env_var, to_machines,
                 last4, account, secret)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
             ON CONFLICT (user_id, kind, provider, host_url) DO UPDATE SET id=excluded.id,
                 env_var=excluded.env_var, to_machines=excluded.to_machines,
                 last4=excluded.last4, account=excluded.account, secret=excluded.secret,
                 created_at=now()
             RETURNING {SHOWN}"
        ))
        .bind(&id)
        .bind(user.as_str())
        .bind(kind)
        .bind(provider)
        .bind(host_url)
        .bind(new.slot.env_var().unwrap_or_default())
        .bind(new.to_machines)
        .bind(new.last4)
        .bind(new.account)
        .bind(sealed)
        .fetch_one(&self.pool)
        .await?;
        credential_from(&row, false)
    }

    /// The user's credentials, git hosts then API keys, marking ones `key` cannot open.
    /// Only the default GitHub account is listed (older apps know one); agent tokens are left out.
    pub(crate) async fn credentials(
        &self,
        user: &UserKey,
        key: &CredentialKey,
    ) -> Result<Vec<CredentialRow>> {
        let rows = sqlx::query(&format!(
            "SELECT {SHOWN}, secret FROM credentials WHERE user_id=$1 AND kind <> '{AGENT_TOKEN}'
             ORDER BY kind <> 'github', kind <> 'forgejo', is_default DESC, created_at,
                 host_url, provider"
        ))
        .bind(user.as_str())
        .fetch_all(&self.pool)
        .await?;
        let mut github = false;
        let mut out = Vec::new();
        for row in &rows {
            let credential = credential_from(row, !opens(row, user, key))?;
            if credential.kind == (CredentialKind::GitHub {}) {
                if github {
                    continue;
                }
                github = true;
            }
            out.push(credential);
        }
        Ok(out)
    }

    /// Removes credential `id`; removing the default GitHub account promotes the oldest other.
    pub(crate) async fn remove_credential(&self, user: &UserKey, id: &str) -> Result<bool> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("SELECT id FROM users WHERE id=$1 FOR UPDATE")
            .bind(user.as_str())
            .execute(&mut *tx)
            .await?;
        let done = sqlx::query("DELETE FROM credentials WHERE user_id=$1 AND id=$2")
            .bind(user.as_str())
            .bind(id)
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            "UPDATE credentials SET is_default=true WHERE id = (
                 SELECT id FROM credentials WHERE user_id=$1 AND kind=$2
                 ORDER BY created_at, id LIMIT 1)
             AND NOT EXISTS (
                 SELECT 1 FROM credentials WHERE user_id=$1 AND kind=$2 AND is_default)",
        )
        .bind(user.as_str())
        .bind(GITHUB)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(done.rows_affected() > 0)
    }
}

impl Store {
    /// The user's API keys by provider, opened; unopenable or unknown ones are logged and skipped.
    pub(crate) async fn api_keys(
        &self,
        user: &UserKey,
        key: &CredentialKey,
    ) -> Result<Vec<ApiKey>> {
        let rows = sqlx::query(
            "SELECT id, kind, provider, host_url, secret FROM credentials
             WHERE user_id=$1 AND kind=$2 ORDER BY provider",
        )
        .bind(user.as_str())
        .bind(API_KEY)
        .fetch_all(&self.pool)
        .await?;
        let mut out = Vec::new();
        for row in &rows {
            let id: String = row.try_get("id")?;
            let sealed: Vec<u8> = row.try_get("secret")?;
            let opened = slot_from(row).and_then(|slot| {
                let Slot::ApiKey(provider) = slot else {
                    bail!("not an API key");
                };
                let owner = Owner {
                    user: user.as_str(),
                    slot: &slot,
                    id: &id,
                };
                let secret = key.open(&owner, &sealed)?;
                Ok(ApiKey { provider, secret })
            });
            match opened {
                Ok(opened) => out.push(opened),
                Err(e) => log::warn!("credential {id} skipped: {e:#}"),
            }
        }
        Ok(out)
    }
}

/// One of the user's API keys, opened.
pub(crate) struct ApiKey {
    pub(crate) provider: ApiProvider,
    pub(crate) secret: SecretString,
}

/// Reseals `source`'s credentials for `target` before a merge, since the seal names its user; unopenable ones stay as they are.
pub(super) async fn reseal_for(
    tx: &mut PgConnection,
    key: &CredentialKey,
    source: &str,
    target: &str,
) -> Result<()> {
    let rows = sqlx::query(
        "SELECT id, kind, provider, host_url, secret FROM credentials WHERE user_id=$1",
    )
    .bind(source)
    .fetch_all(&mut *tx)
    .await?;
    for row in &rows {
        let id: String = row.try_get("id")?;
        let sealed: Vec<u8> = row.try_get("secret")?;
        let resealed = slot_from(row).and_then(|slot| {
            let from = Owner {
                user: source,
                slot: &slot,
                id: &id,
            };
            let secret = key.open(&from, &sealed)?;
            let to = Owner {
                user: target,
                slot: &slot,
                id: &id,
            };
            key.seal(&to, &secret)
        });
        match resealed {
            Ok(resealed) => {
                sqlx::query("UPDATE credentials SET secret=$2 WHERE id=$1")
                    .bind(&id)
                    .bind(resealed)
                    .execute(&mut *tx)
                    .await?;
            }
            Err(e) => log::warn!("credential {id} not resealed for the merge: {e:#}"),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
