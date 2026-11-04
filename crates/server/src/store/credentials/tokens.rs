//! Agent tokens and the credentials a machine receives.

use super::{super::Store, ACCOUNT, AGENT_TOKEN, AgentTokenRow, account_from, opens, slot_from};
use crate::{
    account::credentials::{CredentialKey, GitHubLogin, Opened, Owner, Slot},
    account::user::UserKey,
    protocol::AgentAccount,
};
use anyhow::Result;
use sqlx::Row;

impl Store {
    /// The user's coding agent tokens, marking ones `key` cannot open.
    pub(crate) async fn agent_tokens(
        &self,
        user: &UserKey,
        key: &CredentialKey,
    ) -> Result<Vec<AgentTokenRow>> {
        let rows = sqlx::query(
            "SELECT id, kind, provider, host_url, last4, created_at, secret FROM credentials
             WHERE user_id=$1 AND kind=$2 ORDER BY provider",
        )
        .bind(user.as_str())
        .bind(AGENT_TOKEN)
        .fetch_all(&self.pool)
        .await?;
        let mut out = Vec::new();
        for row in &rows {
            // One a newer server saved for an agent this one does not know.
            let Ok(Slot::AgentToken(agent)) = slot_from(row) else {
                continue;
            };
            out.push(AgentTokenRow {
                agent,
                last4: row.try_get("last4")?,
                created_at: row.try_get("created_at")?,
                unreadable: !opens(row, user, key),
            });
        }
        Ok(out)
    }

    /// Removes `agent`'s token; `false` when the user has none.
    pub(crate) async fn remove_agent_token(
        &self,
        user: &UserKey,
        agent: AgentAccount,
    ) -> Result<bool> {
        let done =
            sqlx::query("DELETE FROM credentials WHERE user_id=$1 AND kind=$2 AND provider=$3")
                .bind(user.as_str())
                .bind(AGENT_TOKEN)
                .bind(agent.slug())
                .execute(&self.pool)
                .await?;
        Ok(done.rows_affected() > 0)
    }

    /// The user's machine credentials, opened; unopenable ones are logged and skipped.
    pub(crate) async fn machine_credentials(
        &self,
        user: &UserKey,
        key: &CredentialKey,
    ) -> Result<Vec<Opened>> {
        let rows = sqlx::query(&format!(
            "SELECT {ACCOUNT}, secret FROM credentials WHERE user_id=$1 AND to_machines
             ORDER BY kind, is_default DESC, created_at, host_url, provider"
        ))
        .bind(user.as_str())
        .fetch_all(&self.pool)
        .await?;
        let mut out = Vec::new();
        for row in &rows {
            let id: String = row.try_get("id")?;
            let sealed: Vec<u8> = row.try_get("secret")?;
            let opened = slot_from(row).and_then(|slot| {
                let owner = Owner {
                    user: user.as_str(),
                    slot: &slot,
                    id: &id,
                };
                let secret = key.open(&owner, &sealed)?;
                let github = match slot {
                    Slot::GitHub => Some(account_from(row, false).map(|account| GitHubLogin {
                        login: account.login,
                        orgs: account.orgs,
                        default: account.default,
                    })?),
                    _ => None,
                };
                Ok(Opened {
                    slot,
                    secret,
                    github,
                })
            });
            match opened {
                Ok(opened) => out.push(opened),
                Err(e) => log::warn!("credential {id} skipped: {e:#}"),
            }
        }
        // With no default among those that open, the oldest is.
        if !out
            .iter()
            .any(|o| o.github.as_ref().is_some_and(|login| login.default))
            && let Some(first) = out.iter_mut().find_map(|o| o.github.as_mut())
        {
            first.default = true;
        }
        Ok(out)
    }
}
