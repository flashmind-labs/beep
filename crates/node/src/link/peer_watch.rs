//! Following one topic of another node of the mesh over `peer-subscriptions/1`,
//! in place of asking it again and again.

use crate::agents::mcp::{Refused, handshake};
use anyhow::{Context, Result, bail};
use beep_mesh::Mesh;
use beep_proto::node::{
    LinkRx, LinkTx,
    events::{HeldSessions, Topic, Value},
    subscriptions::{self, Open, Reply, Request},
};
use iroh::EndpointId;
use std::time::Duration;
use tokio::time::timeout;

/// A silent stream is lost after this long without a message; the peer
/// sends one every `HEARTBEAT`.
const IDLE: Duration = Duration::from_secs(35);

/// One topic of one peer being followed.
pub(crate) struct PeerWatch {
    /// Kept so the peer does not see the stream closed.
    _tx: LinkTx,
    rx: LinkRx,
    held: HeldSessions,
}

impl PeerWatch {
    /// Watches `topic` on `peer`. An error that is `Refused` when the peer will
    /// not serve it to a node, as one from before this.
    pub(crate) async fn open(mesh: &Mesh, peer: EndpointId, topic: Topic) -> Result<Self> {
        let (mut tx, mut rx) = handshake(mesh, peer, subscriptions::PROTOCOL).await?;
        tx.send(&Open {}).await?;

        match timeout(IDLE, rx.recv::<Reply>()).await?? {
            Some(Reply::Ready) => {}
            _ => bail!("the subscription stream did not start with Ready"),
        }

        tx.send(&Request::Watch {
            id: 1,
            topic,
            queries: Vec::new(),
            deltas: true,
        })
        .await?;

        Ok(Self {
            _tx: tx,
            rx,
            held: HeldSessions::default(),
        })
    }

    /// The next value of the topic, the `agents` list whole. An error once the
    /// stream drops or goes quiet.
    pub(crate) async fn next(&mut self) -> Result<Value> {
        loop {
            let reply = timeout(IDLE, self.rx.recv::<Reply>())
                .await
                .context("the machine went quiet")??
                .context("the machine closed the stream")?;

            match reply {
                Reply::Snapshot(snapshot) => {
                    // A delta that does not follow the last message means one
                    // was missed: watching again starts with the whole list.
                    let snapshot = self
                        .held
                        .hear(*snapshot)
                        .context("a change of the list was missed")?;

                    return Ok(snapshot.value);
                }
                Reply::Error { reason, .. } | Reply::SnapshotError { reason, .. } => {
                    return Err(Refused(reason).into());
                }
                Reply::Ready | Reply::Watching { .. } | Reply::Pong { .. } => {}
            }
        }
    }
}
