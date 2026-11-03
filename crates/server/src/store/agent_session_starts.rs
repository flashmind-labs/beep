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
