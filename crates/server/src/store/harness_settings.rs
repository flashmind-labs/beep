//! Per-machine, per-agent harness settings; env vars are sealed like credentials since they may be secrets.

use super::Store;
use crate::{
    account::credentials::{CredentialKey, HarnessOwner},
    account::user::UserKey,
    protocol::node::EnvSecret,
};
use anyhow::{Context, Result};
use secrecy::{ExposeSecret, SecretString};
use sqlx::{PgConnection, Row, postgres::PgRow};

/// One harness's settings as stored, the env vars still sealed.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct HarnessRow {
    pub(crate) machine_id: String,
    pub(crate) agent: String,
    pub(crate) enabled: bool,
    pub(crate) program: String,
    pub(crate) home: String,
    pub(crate) args: Vec<String>,
    /// The env vars' names, in order; their values are sealed.
    pub(crate) env_names: Vec<String>,
}

/// Settings to save for one harness, replacing the ones before.
pub(crate) struct SaveHarness<'a> {
    pub(crate) machine_id: &'a str,
    pub(crate) agent: &'a str,
    pub(crate) enabled: bool,
    pub(crate) program: &'a str,
    pub(crate) home: &'a str,
    pub(crate) args: &'a [String],
    pub(crate) env: &'a [EnvSecret],
}

fn harness_from(row: &PgRow) -> Result<HarnessRow> {
    Ok(HarnessRow {
        machine_id: row.try_get("machine_id")?,
        agent: row.try_get("agent")?,
        enabled: row.try_get("enabled")?,
        program: row.try_get("program")?,
        home: row.try_get("home")?,
        args: row.try_get("args")?,
        env_names: row.try_get("env_names")?,
    })
}

impl Store {
    /// The user's settings for harness `agent`, by machine.
    pub(crate) async fn harness_settings_of(
        &self,
        user: &UserKey,
        agent: &str,
    ) -> Result<Vec<HarnessRow>> {
        let rows = sqlx::query(
            "SELECT machine_id, agent, enabled, program, home, args, env_names
             FROM harness_settings WHERE user_id=$1 AND agent=$2 ORDER BY machine_id",
        )
        .bind(user.as_str())
        .bind(agent)
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(harness_from).collect()
    }

    /// Opened env vars of harness `agent` on `machine_id`; empty when none.
    pub(crate) async fn harness_env(
        &self,
        user: &UserKey,
        key: &CredentialKey,
        machine_id: &str,
        agent: &str,
    ) -> Result<Vec<EnvSecret>> {
        let sealed: Option<Option<Vec<u8>>> = sqlx::query_scalar(
            "SELECT env FROM harness_settings WHERE user_id=$1 AND machine_id=$2 AND agent=$3",
        )
        .bind(user.as_str())
        .bind(machine_id)
        .bind(agent)
        .fetch_optional(&self.pool)
        .await?;
        let Some(sealed) = sealed.flatten() else {
            return Ok(Vec::new());
        };
        let owner = HarnessOwner {
            user: user.as_str(),
            machine: machine_id,
            agent,
        };
        let plain = key.open_harness(&owner, &sealed)?;
        serde_json::from_str(plain.expose_secret())
            .with_context(|| format!("the {agent} env vars do not read"))
    }

    /// Replaces the user's settings for one harness.
    pub(crate) async fn save_harness_settings(
        &self,
        user: &UserKey,
        key: &CredentialKey,
        save: SaveHarness<'_>,
    ) -> Result<()> {
        let names: Vec<&str> = save.env.iter().map(|var| var.name.as_str()).collect();
        let sealed = if save.env.is_empty() {
            None
        } else {
            let plain = SecretString::from(serde_json::to_string(save.env)?);
            let owner = HarnessOwner {
                user: user.as_str(),
                machine: save.machine_id,
                agent: save.agent,
            };
            Some(key.seal_harness(&owner, &plain)?)
        };
        sqlx::query(
            "INSERT INTO harness_settings
                 (user_id, machine_id, agent, enabled, program, home, args, env_names, env)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
             ON CONFLICT (user_id, machine_id, agent) DO UPDATE SET
                 enabled=$4, program=$5, home=$6, args=$7, env_names=$8, env=$9,
                 updated_at=now()",
        )
        .bind(user.as_str())
        .bind(save.machine_id)
        .bind(save.agent)
        .bind(save.enabled)
        .bind(save.program)
        .bind(save.home)
        .bind(save.args)
        .bind(&names)
        .bind(sealed)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

/// Reseals `source`'s harness env vars for `target` before a merge; unopenable ones stay as they are.
pub(super) async fn reseal_harnesses_for(
    tx: &mut PgConnection,
    key: &CredentialKey,
    source: &str,
    target: &str,
) -> Result<()> {
    let rows: Vec<(String, String, Vec<u8>)> = sqlx::query_as(
        "SELECT machine_id, agent, env FROM harness_settings
         WHERE user_id=$1 AND env IS NOT NULL",
    )
    .bind(source)
    .fetch_all(&mut *tx)
    .await?;
    for (machine, agent, sealed) in rows {
        let owner = |user| HarnessOwner {
            user,
            machine: &machine,
            agent: &agent,
        };
        let resealed = key
            .open_harness(&owner(source), &sealed)
            .and_then(|plain| key.seal_harness(&owner(target), &plain));
        match resealed {
            Ok(resealed) => {
                sqlx::query(
                    "UPDATE harness_settings SET env=$4
                     WHERE user_id=$1 AND machine_id=$2 AND agent=$3",
                )
                .bind(source)
                .bind(&machine)
                .bind(&agent)
                .bind(resealed)
                .execute(&mut *tx)
                .await?;
            }
            Err(e) => log::warn!("{agent} settings on {machine} not resealed for the merge: {e:#}"),
        }
    }
    Ok(())
}
