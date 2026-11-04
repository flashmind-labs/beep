//! One coding-agent session's own stream (`<agent>/1`): starting, resuming
//! or attaching to a session, and giving it an order. A session lives in
//! the node, so dropping the stream leaves it running.

use super::{said, unexpected};
use crate::{CALL_TIMEOUT, MeshError, PeerClient};
use beep_proto::node::{
    LinkRx, LinkTx,
    agent::{self, Event, Request, SessionInfo},
};
use std::time::Duration;
use tokio::time::timeout;

/// What a session starts on besides its folder and prompt; `None` keeps the agent's default.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StartOn {
    /// An id from the agent's model list.
    pub model: Option<String>,
    /// One of `agent::efforts` for the agent.
    pub effort: Option<agent::Effort>,
    /// The branch or worktree to make first, for a node that makes them.
    pub checkout: Option<agent::CheckoutPlan>,
}

/// A session stream the node accepted.
pub struct Opened {
    pub info: SessionInfo,
    pub tx: LinkTx,
    pub rx: LinkRx,
}

/// Opens `agent`'s stream and sends `first`, which is `Start`, `Attach` or
/// `Resume`. The node answers with the session as it stands.
pub async fn open_agent(
    client: &PeerClient,
    agent: &str,
    first: Request,
) -> Result<Opened, MeshError> {
    let (mut tx, mut rx) = client.open(&agent::protocol(agent)).await?;
    tx.send(&first).await.map_err(MeshError::Link)?;
    let event = timeout(CALL_TIMEOUT, rx.recv::<Event>())
        .await
        .map_err(|_| said("the node did not answer"))?
        .map_err(MeshError::Link)?
        .ok_or_else(|| said("the node closed the session stream"))?;
    match event {
        Event::Attached { session } => Ok(Opened {
            info: *session,
            tx,
            rx,
        }),
        Event::Refused { reason } => Err(said(reason)),
        other => Err(unexpected(&other)),
    }
}

/// Sends `request` down an attached stream and waits for the node's `Ack`,
/// skipping the state changes that come first. `Err` carries the node's
/// reason when it did nothing.
pub async fn order(
    opened: &mut Opened,
    request: Request,
    limit: Duration,
) -> Result<(), MeshError> {
    opened.tx.send(&request).await.map_err(MeshError::Link)?;
    let wait = async {
        loop {
            match opened.rx.recv::<Event>().await.map_err(MeshError::Link)? {
                Some(Event::Ack { error: None }) => return Ok(()),
                Some(Event::Ack { error: Some(error) }) => return Err(said(error)),
                Some(Event::Ended { reason }) => return Err(said(reason)),
                Some(_) => {}
                None => return Err(said("lost the session's stream")),
            }
        }
    };
    timeout(limit, wait)
        .await
        .map_err(|_| said("the node did not answer"))?
}
