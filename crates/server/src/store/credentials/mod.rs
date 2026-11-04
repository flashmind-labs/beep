//! The credentials a user added, sealed (see `crate::account::credentials`).

use super::Store;
use crate::{
    account::credentials::{CredentialKey, GitHubLogin, Opened, Owner, Slot},
    account::user::UserKey,
    protocol::{AgentAccount, ApiProvider, CredentialKind},
};
use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use secrecy::SecretString;
use sqlx::{PgConnection, Row, postgres::PgRow};
use std::str::FromStr;
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

/// `kind, provider, host_url` of a slot.
struct Columns {
    kind: &'static str,
    provider: &'static str,
    host_url: String,
}

fn columns(slot: &Slot) -> Columns {
    match slot {
        Slot::GitHub => Columns {
            kind: GITHUB,
            provider: "",
            host_url: String::new(),
        },
        Slot::Forgejo(url) => Columns {
            kind: FORGEJO,
            provider: "",
            host_url: url.as_str().to_owned(),
        },
        Slot::ApiKey(provider) => Columns {
            kind: API_KEY,
            provider: provider.slug(),
            host_url: String::new(),
        },
        Slot::AgentToken(agent) => Columns {
            kind: AGENT_TOKEN,
            provider: agent.slug(),
            host_url: String::new(),
        },
    }
}

fn slot_from(row: &PgRow) -> Result<Slot> {
    let kind: String = row.try_get("kind")?;
    Ok(match kind.as_str() {
        GITHUB => Slot::GitHub,
        FORGEJO => Slot::Forgejo(row.try_get::<String, _>("host_url")?.parse()?),
        API_KEY => {
            let provider: String = row.try_get("provider")?;
            Slot::ApiKey(
                ApiProvider::from_str(&provider)
                    .with_context(|| format!("unknown provider {provider:?}"))?,
            )
        }
        AGENT_TOKEN => {
            let agent: String = row.try_get("provider")?;
            Slot::AgentToken(
                AgentAccount::from_str(&agent)
                    .with_context(|| format!("unknown agent {agent:?}"))?,
            )
        }
        other => bail!("unknown credential kind {other:?}"),
    })
}

/// The row's kind as stored, without checking it.
fn kind_from(row: &PgRow) -> Result<CredentialKind> {
    let kind: String = row.try_get("kind")?;
    Ok(match kind.as_str() {
        GITHUB => CredentialKind::GitHub {},
        FORGEJO => CredentialKind::Forgejo {
            url: row.try_get("host_url")?,
        },
        API_KEY => CredentialKind::ApiKey {
            provider: row.try_get("provider")?,
        },
        _ => CredentialKind::ApiKey { provider: kind },
    })
}

fn credential_from(row: &PgRow, unreadable: bool) -> Result<CredentialRow> {
    Ok(CredentialRow {
        id: row.try_get("id")?,
        kind: kind_from(row)?,
        env_var: row.try_get("env_var")?,
        last4: row.try_get("last4")?,
        account: row.try_get("account")?,
        to_machines: row.try_get("to_machines")?,
        created_at: row.try_get("created_at")?,
        unreadable,
    })
}

const SHOWN: &str =
    "id, kind, provider, host_url, env_var, last4, account, to_machines, created_at";
const ACCOUNT: &str = "id, kind, provider, host_url, account, orgs, is_default, last4, created_at";

fn account_from(row: &PgRow, unreadable: bool) -> Result<GitHubAccountRow> {
    let account: Option<String> = row.try_get("account")?;
    Ok(GitHubAccountRow {
        id: row.try_get("id")?,
        login: match account {
            Some(login) => login,
            None => row.try_get("provider")?,
        },
        orgs: row.try_get("orgs")?,
        default: row.try_get("is_default")?,
        last4: row.try_get("last4")?,
        created_at: row.try_get("created_at")?,
        unreadable,
    })
}

/// Whether the row's sealed secret opens with `key`.
fn opens(row: &PgRow, user: &UserKey, key: &CredentialKey) -> bool {
    let (Ok(id), Ok(sealed)) = (
        row.try_get::<String, _>("id"),
        row.try_get::<Vec<u8>, _>("secret"),
    ) else {
        return false;
    };
    slot_from(row).is_ok_and(|slot| {
        let owner = Owner {
            user: user.as_str(),
            slot: &slot,
            id: &id,
        };
        key.open(&owner, &sealed).is_ok()
    })
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
