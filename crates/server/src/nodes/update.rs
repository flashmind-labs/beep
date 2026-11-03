//! Updating a node's beep-node from Beep, over `update/1` (see
//! `beep_proto::node::update`).

use super::Online;
use crate::protocol::node::{
    agent::{SESSIONS_PROTOCOL, Status},
    update::{Event, PROTOCOL, Request},
};
use anyhow::{Context, Result, bail};
use std::time::Duration;
use tokio::time::timeout;

/// How long the node has to download, check and install the release.
const INSTALL_TIMEOUT: Duration = Duration::from_secs(5 * 60);

/// What the node said it did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Updated {
    pub(crate) from: String,
    pub(crate) to: String,
    pub(crate) restarting: bool,
    pub(crate) note: String,
}

impl Online {
    /// Whether it can be updated over `update/1`. The Mac app's node and
    /// nodes from before it cannot.
    pub(crate) fn updatable(&self) -> bool {
        self.offers(PROTOCOL)
    }

    /// The agent sessions the node runs that have not ended, which its
    /// restart stops.
    pub(crate) async fn running_agent_sessions(&self) -> Result<u32> {
        if !self.offers(SESSIONS_PROTOCOL) {
            return Ok(0);
        }
        let sessions = self.agent_sessions().await?;
        let running = sessions
            .iter()
            .filter(|session| session.state.status != Status::Ended)
            .count();
        Ok(u32::try_from(running).unwrap_or(u32::MAX))
    }

    /// Has the node install the newest release and restart. `Err` holds
    /// the node's reason, in words for the user.
    pub(crate) async fn update(&self) -> Result<Updated> {
        let (mut tx, mut rx) = self.open_stream(PROTOCOL).await?;
        tx.send(&Request::Update {}).await?;
        let event = timeout(INSTALL_TIMEOUT, rx.recv::<Event>())
            .await
            .context("the update took too long")??
            .context("the node closed the update without an answer")?;
        updated(event)
    }
}

fn updated(event: Event) -> Result<Updated> {
    match event {
        Event::Newest { version } => Ok(Updated {
            from: version.clone(),
            to: version,
            restarting: false,
            note: String::new(),
        }),
        Event::Installed {
            from,
            to,
            restarting,
            note,
        } => Ok(Updated {
            from,
            to,
            restarting,
            note,
        }),
        Event::Failed { reason } => bail!(reason),
        other => bail!("the node answered {other:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_newest_is_neither_installed_nor_restarted() {
        let done = updated(Event::Newest {
            version: "0.1.24".into(),
        })
        .expect("an answer");
        assert_eq!((done.from.as_str(), done.to.as_str()), ("0.1.24", "0.1.24"));
        assert!(!done.restarting);
    }

    #[test]
    fn a_failure_keeps_the_nodes_reason() {
        let e = updated(Event::Failed {
            reason: "no build for this machine".into(),
        })
        .expect_err("a failure");
        assert_eq!(e.to_string(), "no build for this machine");
    }
}
