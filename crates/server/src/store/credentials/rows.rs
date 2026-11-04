//! Reading credential rows: slots, kinds, and whether a row still opens.

use super::{AGENT_TOKEN, API_KEY, CredentialRow, FORGEJO, GITHUB, GitHubAccountRow};
use crate::{
    account::credentials::{CredentialKey, Owner, Slot},
    account::user::UserKey,
    protocol::{AgentAccount, ApiProvider, CredentialKind},
};
use anyhow::{Context, Result, bail};
use sqlx::{Row, postgres::PgRow};
use std::str::FromStr;

/// `kind, provider, host_url` of a slot.
pub(super) struct Columns {
    pub(super) kind: &'static str,
    pub(super) provider: &'static str,
    pub(super) host_url: String,
}

pub(super) fn columns(slot: &Slot) -> Columns {
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

pub(super) fn slot_from(row: &PgRow) -> Result<Slot> {
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
pub(super) fn kind_from(row: &PgRow) -> Result<CredentialKind> {
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

pub(super) fn credential_from(row: &PgRow, unreadable: bool) -> Result<CredentialRow> {
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

pub(super) const SHOWN: &str =
    "id, kind, provider, host_url, env_var, last4, account, to_machines, created_at";

pub(super) const ACCOUNT: &str =
    "id, kind, provider, host_url, account, orgs, is_default, last4, created_at";

pub(super) fn account_from(row: &PgRow, unreadable: bool) -> Result<GitHubAccountRow> {
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
pub(super) fn opens(row: &PgRow, user: &UserKey, key: &CredentialKey) -> bool {
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
