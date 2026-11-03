//! A user's settings for the coding agent harnesses on their machines, per
//! machine and agent. The env vars are sealed like credentials (see
//! `crate::credentials`), since their values may be secrets.

use super::Store;
use crate::{
    credentials::{CredentialKey, HarnessOwner},
    protocol::node::EnvSecret,
    user::UserKey,
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

/// A harness turned off on a machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OffHarness {
    pub(crate) machine_id: String,
    pub(crate) agent: String,
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
    /// The user's settings for the harnesses on `machine_id`, by agent.
    pub(crate) async fn harness_settings(
        &self,
        user: &UserKey,
        machine_id: &str,
    ) -> Result<Vec<HarnessRow>> {
        let rows = sqlx::query(
            "SELECT machine_id, agent, enabled, program, home, args, env_names
             FROM harness_settings WHERE user_id=$1 AND machine_id=$2 ORDER BY agent",
        )
        .bind(user.as_str())
        .bind(machine_id)
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(harness_from).collect()
    }

    /// The env vars of the user's harness `agent` on `machine_id`, opened;
    /// empty when there are none.
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

    /// Saves the user's settings for one harness in place of the ones
    /// before.
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

    /// The harnesses the user turned off, on any machine.
    pub(crate) async fn off_harnesses(&self, user: &UserKey) -> Result<Vec<OffHarness>> {
        let rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT machine_id, agent FROM harness_settings
             WHERE user_id=$1 AND NOT enabled ORDER BY machine_id, agent",
        )
        .bind(user.as_str())
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(machine_id, agent)| OffHarness { machine_id, agent })
            .collect())
    }
}

/// Seals the env vars of `source`'s harnesses again for `target`, before
/// an account merge moves them: what is sealed names its user. One that
/// does not open is left as it is, unreadable either way.
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{protocol::SecretText, store::test_db::TestDb};

    fn env_var(name: &str, value: &str) -> EnvSecret {
        EnvSecret {
            name: name.to_owned(),
            value: SecretText::new(value),
        }
    }

    #[tokio::test]
    async fn settings_round_trip_with_their_env_sealed() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let key = CredentialKey::random();
        let user = UserKey::for_test("alice");
        let other = UserKey::for_test("bob");
        let args = vec!["--profile".to_owned(), "work".to_owned()];
        let env = vec![env_var("HTTPS_PROXY", "http://proxy:3128")];
        store
            .save_harness_settings(
                &user,
                &key,
                SaveHarness {
                    machine_id: "m1",
                    agent: "codex",
                    enabled: false,
                    program: "/opt/codex-acp",
                    home: "~/.codex-work",
                    args: &args,
                    env: &env,
                },
            )
            .await
            .unwrap();
        let rows = store.harness_settings(&user, "m1").await.unwrap();
        assert_eq!(
            rows,
            [HarnessRow {
                machine_id: "m1".into(),
                agent: "codex".into(),
                enabled: false,
                program: "/opt/codex-acp".into(),
                home: "~/.codex-work".into(),
                args: args.clone(),
                env_names: vec!["HTTPS_PROXY".into()],
            }]
        );
        assert_eq!(
            store.harness_env(&user, &key, "m1", "codex").await.unwrap(),
            env
        );
        // The value is not in the row as text.
        let raw: Vec<u8> = sqlx::query_scalar("SELECT env FROM harness_settings")
            .fetch_one(&store.pool)
            .await
            .unwrap();
        assert!(!raw.windows(4).any(|w| w == b"3128"));
        assert_eq!(
            store.off_harnesses(&user).await.unwrap(),
            [OffHarness {
                machine_id: "m1".into(),
                agent: "codex".into(),
            }]
        );
        assert!(
            store
                .harness_settings(&other, "m1")
                .await
                .unwrap()
                .is_empty()
        );
        assert!(
            store
                .harness_settings(&user, "m2")
                .await
                .unwrap()
                .is_empty()
        );

        // Saving again replaces them, and no env vars leave none sealed.
        store
            .save_harness_settings(
                &user,
                &key,
                SaveHarness {
                    machine_id: "m1",
                    agent: "codex",
                    enabled: true,
                    program: "",
                    home: "",
                    args: &[],
                    env: &[],
                },
            )
            .await
            .unwrap();
        let rows = store.harness_settings(&user, "m1").await.unwrap();
        assert!(rows[0].enabled && rows[0].env_names.is_empty());
        assert!(
            store
                .harness_env(&user, &key, "m1", "codex")
                .await
                .unwrap()
                .is_empty()
        );
        assert!(store.off_harnesses(&user).await.unwrap().is_empty());
    }
}
