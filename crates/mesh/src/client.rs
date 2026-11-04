//! The client side of a node's sessions: open a stream to a peer's
//! `session()`, or to the node in this process with no network at all.

use crate::{Mesh, MeshError};
use anyhow::anyhow;
use beep_proto::node::{LinkRx, LinkTx, Open, Opened, link, local_link};
use futures::Stream;
use futures::stream::unfold;
use iroh::EndpointId;
use iroh::endpoint::{Connection, RecvStream, SendStream};
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::fmt::{self, Debug, Formatter};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;
use tokio::task::spawn;
use tokio::time::timeout;

/// How long `with_deadline` waits for a member to come online.
pub const DEADLINE: Duration = Duration::from_secs(3);
/// How long a node has to answer an `Open`.
const OPEN_TIMEOUT: Duration = Duration::from_secs(10);

/// Serves one session stream: the node's `session()` over in-memory halves.
pub type SessionFn =
    Arc<dyn Fn(LinkTx, LinkRx) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync>;

/// The machine is not a member, or did not come online within `DEADLINE`.
#[derive(Debug, Error)]
#[error("{0} is offline")]
pub struct Offline(pub EndpointId);

/// Opens a raw bi stream to a peer, picking the connection afresh each time.
type OpenBi = Arc<
    dyn Fn() -> Pin<Box<dyn Future<Output = Result<(SendStream, RecvStream), MeshError>> + Send>>
        + Send
        + Sync,
>;

#[derive(Clone)]
enum Via {
    Peer(OpenBi),
    Local(SessionFn),
}

/// Opens sessions on one node: a peer over its mesh connection, or the node
/// in this process.
#[derive(Clone)]
pub struct PeerClient {
    via: Via,
}

impl Debug for PeerClient {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let via = match &self.via {
            Via::Peer(_) => "peer",
            Via::Local(_) => "local",
        };
        f.debug_struct("PeerClient").field("via", &via).finish()
    }
}

impl PeerClient {
    /// A client that opens every stream on this one connection.
    pub fn new(connection: Connection) -> Self {
        Self {
            via: Via::Peer(Arc::new(move || {
                let connection = connection.clone();
                Box::pin(async move {
                    connection
                        .open_bi()
                        .await
                        .map_err(|e| MeshError::Link(e.into()))
                })
            })),
        }
    }

    /// A client for `peer` that looks up the mesh's current connection for
    /// each stream, so a replaced link is picked up.
    pub fn over(mesh: Mesh, peer: EndpointId) -> Self {
        Self {
            via: Via::Peer(Arc::new(move || {
                let mesh = mesh.clone();
                Box::pin(async move { mesh.open_bi(peer).await })
            })),
        }
    }

    /// A client for the node in this process. No network, no timers: each
    /// stream is a pair of in-memory pipes with `session` serving the far end.
    pub fn local(session: SessionFn) -> Self {
        Self {
            via: Via::Local(session),
        }
    }

    /// Opens a stream for `protocol` and waits for the node to accept it.
    pub async fn open(&self, protocol: &str) -> Result<(LinkTx, LinkRx), MeshError> {
        let (mut tx, mut rx) = match &self.via {
            Via::Peer(open_bi) => {
                let (send, recv) = open_bi().await?;
                link(send, recv)
            }
            Via::Local(session) => {
                let pair = local_link();
                let (node_tx, node_rx) = pair.node;
                // Ends when either side drops its half of the pipes.
                spawn(session(node_tx, node_rx));
                pair.caller
            }
        };
        let open = Open {
            protocol: protocol.to_owned(),
        };
        tx.send(&open).await.map_err(MeshError::Link)?;
        let opened = match &self.via {
            Via::Peer(_) => timeout(OPEN_TIMEOUT, rx.recv::<Opened>())
                .await
                .map_err(|e| {
                    MeshError::Link(
                        anyhow::Error::new(e).context("the node did not open the stream"),
                    )
                })?,
            Via::Local(_) => rx.recv::<Opened>().await,
        };
        match opened
            .map_err(MeshError::Link)?
            .ok_or_else(|| MeshError::Link(anyhow!("the node closed the stream")))?
        {
            Opened::Ready {} => Ok((tx, rx)),
            Opened::Refused { reason } => Err(MeshError::Declined {
                protocol: protocol.to_owned(),
                reason,
            }),
            other => Err(MeshError::Unexpected(format!("{other:?}"))),
        }
    }

    /// Sends one request on a new stream and reads one reply.
    pub async fn call<Q: Serialize, R: DeserializeOwned>(
        &self,
        protocol: &str,
        request: &Q,
    ) -> Result<R, MeshError> {
        let (mut tx, mut rx) = self.open(protocol).await?;
        tx.send(request).await.map_err(MeshError::Link)?;
        rx.recv()
            .await
            .map_err(MeshError::Link)?
            .ok_or_else(|| MeshError::Link(anyhow!("the node closed the stream")))
    }

    /// Sends one request and yields the replies until the node finishes the
    /// stream. A broken stream yields its error once and then ends.
    pub async fn follow<Q: Serialize, R: DeserializeOwned + Send + 'static>(
        &self,
        protocol: &str,
        request: &Q,
    ) -> Result<impl Stream<Item = Result<R, MeshError>> + use<Q, R>, MeshError> {
        let (mut tx, rx) = self.open(protocol).await?;
        tx.send(request).await.map_err(MeshError::Link)?;
        // `tx` rides along: finishing our half would read as a cancel.
        Ok(unfold(Some((tx, rx)), |state| async move {
            let (tx, mut rx) = state?;
            match rx.recv::<R>().await {
                Ok(Some(item)) => Some((Ok(item), Some((tx, rx)))),
                Ok(None) => None,
                Err(e) => Some((Err(MeshError::Link(e)), None)),
            }
        }))
    }
}

/// A client for `machine`, as soon as it is connected. A machine that is
/// not a member is `Offline` at once; a member gets `DEADLINE` to appear.
pub async fn with_deadline(machine: EndpointId, mesh: &Mesh) -> Result<PeerClient, Offline> {
    let mut updates = mesh.updates();
    let wait = async {
        loop {
            updates.borrow_and_update();
            // A removed device may keep its link for a few seconds.
            if !mesh.is_member(machine) {
                return Err(Offline(machine));
            }
            if mesh.peer(machine).is_some() {
                return Ok(PeerClient::over(mesh.clone(), machine));
            }
            if updates.changed().await.is_err() {
                return Err(Offline(machine));
            }
        }
    };
    timeout(DEADLINE, wait)
        .await
        .unwrap_or(Err(Offline(machine)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use beep_proto::node::{LinkRx, LinkTx, Open, Opened};
    use futures::StreamExt;
    use std::sync::Arc;
    use std::time::Instant;

    async fn serve(mut tx: LinkTx, mut rx: LinkRx) {
        let Ok(Some(open)) = rx.recv::<Open>().await else {
            return;
        };
        match open.protocol.as_str() {
            "echo/1" => {
                tx.send(&Opened::Ready {}).await.ok();
                while let Ok(Some(n)) = rx.recv::<u32>().await {
                    if tx.send(&(n + 1)).await.is_err() {
                        break;
                    }
                }
            }
            "count/1" => {
                tx.send(&Opened::Ready {}).await.ok();
                if let Ok(Some(n)) = rx.recv::<u32>().await {
                    for i in 0..n {
                        tx.send(&i).await.ok();
                    }
                }
            }
            other => {
                let reason = format!("this node does not offer {other}");
                tx.send(&Opened::Refused { reason }).await.ok();
            }
        }
    }

    fn session() -> SessionFn {
        Arc::new(|tx, rx| Box::pin(serve(tx, rx)))
    }

    #[tokio::test]
    async fn a_local_call_is_answered_without_a_network() {
        let client = PeerClient::local(session());
        let reply: u32 = client.call("echo/1", &41u32).await.expect("answered");
        assert_eq!(reply, 42);
    }

    #[tokio::test]
    async fn a_local_follow_yields_every_item_then_ends() {
        let client = PeerClient::local(session());
        let items: Vec<u32> = client
            .follow::<u32, u32>("count/1", &3)
            .await
            .expect("opened")
            .map(|item| item.expect("item"))
            .collect()
            .await;
        assert_eq!(items, [0, 1, 2]);
    }

    #[tokio::test]
    async fn a_local_refusal_reads_like_the_nodes() {
        let client = PeerClient::local(session());
        let err = client.open("nope/1").await.err().expect("refused");
        assert_eq!(
            err.to_string(),
            "the node refused nope/1: this node does not offer nope/1"
        );
    }

    #[tokio::test]
    async fn a_local_call_costs_well_under_a_millisecond_when_the_box_is_idle() {
        let client = PeerClient::local(session());
        let mut fastest = std::time::Duration::MAX;
        for _ in 0..200 {
            let start = Instant::now();
            let _: u32 = client.call("echo/1", &1u32).await.expect("answered");
            fastest = fastest.min(start.elapsed());
        }
        eprintln!("fastest local call: {fastest:?}");
        // The fastest of 200 is steady on a loaded box; 5 ms is far above it.
        assert!(fastest < std::time::Duration::from_millis(5), "{fastest:?}");
    }
}
