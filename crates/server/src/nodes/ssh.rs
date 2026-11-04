//! Adding a machine over another node's ssh, over `ssh/1` (see
//! `beep_proto::node::ssh`).

use super::Online;
use crate::protocol::{
    SecretText,
    node::ssh::{Event, PROTOCOL, Request},
    setup::SetupToken,
};
use anyhow::{Context, Result, bail};
use std::time::Duration;
use tokio::time::timeout;

/// Longer than the node's own limits for its steps together.
const ADD_TIMEOUT: Duration = Duration::from_secs(8 * 60);
const HOSTS_TIMEOUT: Duration = Duration::from_secs(10);

/// What `add_over_ssh` did: the target's system, and why it failed.
pub(crate) struct SshResult {
    pub(crate) system: String,
    /// `None` once beep-node runs there, linked.
    pub(crate) failure: Option<SshFailure>,
}

pub(crate) struct SshFailure {
    pub(crate) reason: String,
    pub(crate) output: Vec<String>,
}

impl Online {
    /// Whether the node serves `ssh/1`.
    pub(crate) fn runs_ssh(&self) -> bool {
        self.offers(PROTOCOL)
    }

    /// The hosts the node's ssh knows.
    pub(crate) async fn ssh_hosts(&self) -> Result<Vec<String>> {
        let (mut tx, mut rx) = self.open_stream(PROTOCOL).await?;
        tx.send(&Request::Hosts {}).await?;
        let event = timeout(HOSTS_TIMEOUT, rx.recv::<Event>())
            .await
            .context("the node took too long to list its ssh hosts")??
            .context("the node closed the stream without an answer")?;
        match event {
            Event::Hosts { hosts } => Ok(hosts),
            other => bail!("the node answered {other:?}"),
        }
    }

    /// Has the node install beep-node on `target` and link it with `token`,
    /// looking for checkouts in `search` (empty for home).
    pub(crate) async fn add_over_ssh(
        &self,
        target: &str,
        token: &SetupToken,
        search: &str,
    ) -> Result<SshResult> {
        let (mut tx, mut rx) = self.open_stream(PROTOCOL).await?;
        tx.send(&Request::Add {
            target: target.to_owned(),
            token: SecretText::new(token.to_string()),
            search: search.to_owned(),
        })
        .await?;
        let event = timeout(ADD_TIMEOUT, rx.recv::<Event>())
            .await
            .context("adding the machine took too long")??
            .context("the node closed the stream without an answer")?;
        match event {
            Event::Added { system } => Ok(SshResult {
                system,
                failure: None,
            }),
            Event::Failed {
                reason,
                system,
                output,
            } => Ok(SshResult {
                system,
                failure: Some(SshFailure { reason, output }),
            }),
            other => bail!("the node answered {other:?}"),
        }
    }
}
