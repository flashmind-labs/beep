//! Which device started each coding-agent session, so its alerts go to that device only.

use super::Store;
use crate::user::UserKey;
use anyhow::{Context, Result};
use iroh::EndpointId;

/// A session on a machine: node key, agent, and the agent's session id.
#[derive(Clone, Copy)]
pub(crate) struct SessionOn<'a> {
    pub machine: &'a EndpointId,
    pub agent: &'a str,
    pub session: &'a str,
}

impl Store {
    /// Remembers that `device` started `on`; a resume makes the newer device the one told.
    pub(crate) async fn record_agent_start(
        &self,
        user: &UserKey,
        on: SessionOn<'_>,
        device: &EndpointId,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO agent_session_starts (user_id, machine, agent, session, device)
             VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT (machine, agent, session)
             DO UPDATE SET user_id=$1, device=$5, created_at=now()",
        )
        .bind(user.as_str())
        .bind(on.machine.to_string())
        .bind(on.agent)
        .bind(on.session)
        .bind(device.to_string())
        .execute(&self.pool)
        .await
        .context("the session's start was not saved")?;
        Ok(())
    }

    /// The device that started `on` for `user`, if one did through Beep.
    pub(crate) async fn agent_starter(
        &self,
        user: &UserKey,
        on: SessionOn<'_>,
    ) -> Result<Option<EndpointId>> {
        let device: Option<String> = sqlx::query_scalar(
            "SELECT device FROM agent_session_starts
             WHERE machine=$1 AND agent=$2 AND session=$3 AND user_id=$4",
        )
        .bind(on.machine.to_string())
        .bind(on.agent)
        .bind(on.session)
        .bind(user.as_str())
        .fetch_optional(&self.pool)
        .await
        .context("the session's start does not read")?;
        device
            .map(|device| device.parse().context("a saved device key does not parse"))
            .transpose()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::test_db::TestDb;
    use iroh::SecretKey;

    fn key(n: u8) -> EndpointId {
        SecretKey::from_bytes(&[n; 32]).public()
    }

    #[tokio::test]
    async fn the_latest_start_names_the_device() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let alice = UserKey::for_test("alice");
        let bob = UserKey::for_test("bob");
        let machine = key(9);
        let on = SessionOn {
            machine: &machine,
            agent: "claude",
            session: "a3f9c1",
        };
        assert_eq!(store.agent_starter(&alice, on).await.unwrap(), None);
        store.record_agent_start(&alice, on, &key(1)).await.unwrap();
        assert_eq!(store.agent_starter(&alice, on).await.unwrap(), Some(key(1)));
        assert_eq!(
            store.agent_starter(&bob, on).await.unwrap(),
            None,
            "only its own user's"
        );
        store.record_agent_start(&alice, on, &key(2)).await.unwrap();
        assert_eq!(store.agent_starter(&alice, on).await.unwrap(), Some(key(2)));
        let codex = SessionOn {
            agent: "codex",
            ..on
        };
        assert_eq!(store.agent_starter(&alice, codex).await.unwrap(), None);
    }
}
