//! Asking a node about its coding agent harnesses.

use super::Online;
use crate::protocol::node::{
    agent::{SESSIONS_PROTOCOL, SessionsReply, SessionsRequest},
    harness::HarnessReport,
};
use anyhow::{Context, Result, bail};
use std::time::Duration;
use tokio::time::timeout;

/// How long a node may take to report: it runs each harness and asks npm for versions.
const REPORT_TIMEOUT: Duration = Duration::from_secs(45);
/// How long an install or update may take on the node, and a little more.
const UPDATE_TIMEOUT: Duration = Duration::from_secs(11 * 60);

impl Online {
    /// What the node finds of each harness; `None` from a node too old to say.
    pub(crate) async fn harnesses(&self, refresh: bool) -> Result<Option<HarnessReport>> {
        self.ask_harnesses(SessionsRequest::Harnesses { refresh }, REPORT_TIMEOUT)
            .await
    }

    /// Installs or updates `agent` on the node; `None` from a node too old to.
    pub(crate) async fn update_harness(&self, agent: &str) -> Result<Option<HarnessReport>> {
        let request = SessionsRequest::UpdateHarness {
            agent: agent.to_owned(),
        };
        self.ask_harnesses(request, UPDATE_TIMEOUT).await
    }

    async fn ask_harnesses(
        &self,
        request: SessionsRequest,
        limit: Duration,
    ) -> Result<Option<HarnessReport>> {
        let (mut tx, mut rx) = self.open_stream(SESSIONS_PROTOCOL).await?;
        tx.send(&request).await?;
        let reply = timeout(limit, rx.recv::<SessionsReply>())
            .await
            .context("the node did not answer in time")?;
        match reply {
            Ok(Some(SessionsReply::Harnesses { report })) => Ok(Some(report)),
            // A node from before harnesses says it does not serve them.
            Ok(Some(SessionsReply::Error { message }))
                if message.starts_with("this node does not serve") =>
            {
                Ok(None)
            }
            Ok(Some(SessionsReply::Error { message })) => bail!("{message}"),
            Ok(Some(other)) => bail!("the node answered {other:?}"),
            Ok(None) => Ok(None),
            Err(e) => {
                log::debug!("node did not report its harnesses; error={e:#}");
                Ok(None)
            }
        }
    }
}
