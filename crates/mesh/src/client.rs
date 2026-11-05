//! The client side of a node's sessions: open a stream to a peer's
//! `session()`, or to the node in this process with no network at all.

use crate::{Mesh, MeshError};
use anyhow::anyhow;
use beep_proto::node::{LinkRx, LinkTx, Open, Opened, link, link_on, local_link};
use futures::Stream;
use futures::stream::unfold;
use iroh::EndpointId;
use iroh::endpoint::Connection;
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
/// How long a network open may take, from the new stream to the node's answer.
pub const OPEN_TIMEOUT: Duration = Duration::from_secs(10);

/// How long `call` waits for a node to reply.
pub const CALL_TIMEOUT: Duration = Duration::from_secs(30);

/// Serves one session stream: the node's `session()` over in-memory halves.
pub type SessionFn =
    Arc<dyn Fn(LinkTx, LinkRx) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync>;

/// The machine is not a member, or did not come online within `DEADLINE`.
#[derive(Debug, Error)]
#[error("{0} is offline")]
pub struct Offline(pub EndpointId);

/// Opens a stream to a peer, picking the connection afresh each time, and
/// writes it the way that connection's protocol is written.
type OpenBi = Arc<
    dyn Fn() -> Pin<Box<dyn Future<Output = Result<(LinkTx, LinkRx), MeshError>> + Send>>
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
                    let (send, recv) = connection
                        .open_bi()
                        .await
                        .map_err(|e| MeshError::Link(e.into()))?;
                    Ok(link_on(&connection, send, recv))
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
                Box::pin(async move {
                    // Mesh streams are always compact.
                    let (send, recv) = mesh
                        .open_bi(peer)
                        .await
                        .map_err(|e| MeshError::Link(e.into()))?;
                    Ok(link(send, recv))
                })
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
    /// Over the network the whole exchange, from opening the stream to the
    /// node's answer, gets `OPEN_TIMEOUT`.
    pub async fn open(&self, protocol: &str) -> Result<(LinkTx, LinkRx), MeshError> {
        let (tx, mut rx) = self.begin(protocol).await?;
        self.confirmed(&mut rx, protocol).await?;
        Ok((tx, rx))
    }

    /// Like `open`, with `first` sent right behind `Open`, before the node's
    /// answer is read: a stream whose first message decides everything
    /// (start a session, follow one) costs one round trip. A refusal is
    /// reported in place of a failure to send.
    pub async fn open_sending<Q: Serialize>(
        &self,
        protocol: &str,
        first: &Q,
    ) -> Result<(LinkTx, LinkRx), MeshError> {
        let (mut tx, mut rx) = self.begin(protocol).await?;
        let sent = tx.send(first).await;
        self.confirmed(&mut rx, protocol).await?;
        sent.map_err(MeshError::Link)?;
        Ok((tx, rx))
    }

    /// Opens the stream and sends `Open`, without waiting for the node's
    /// answer. What the caller sends next follows `Open` at once, so a call
    /// costs one round trip instead of two. `confirmed` reads the answer.
    async fn begin(&self, protocol: &str) -> Result<(LinkTx, LinkRx), MeshError> {
        match &self.via {
            Via::Peer(open_bi) => timeout(OPEN_TIMEOUT, async {
                let (mut tx, rx) = open_bi().await?;
                send_open(&mut tx, protocol).await?;
                Ok((tx, rx))
            })
            .await
            .map_err(|e| {
                MeshError::Link(anyhow::Error::new(e).context("the node did not open the stream"))
            })?,
            Via::Local(session) => {
                let pair = local_link();
                let (node_tx, node_rx) = pair.node;
                // The session task ends when either side drops its half of
                // the pipes, so nothing outlives the call or the stream.
                spawn(session(node_tx, node_rx));
                let (mut tx, rx) = pair.caller;
                send_open(&mut tx, protocol).await?;
                Ok((tx, rx))
            }
        }
    }

    /// Reads the node's answer to `Open`. Over the network it gets
    /// `OPEN_TIMEOUT`.
    async fn confirmed(&self, rx: &mut LinkRx, protocol: &str) -> Result<(), MeshError> {
        match &self.via {
            Via::Peer(_) => timeout(OPEN_TIMEOUT, answer(rx, protocol))
                .await
                .map_err(|e| {
                    MeshError::Link(
                        anyhow::Error::new(e).context("the node did not open the stream"),
                    )
                })?,
            Via::Local(_) => answer(rx, protocol).await,
        }
    }

    /// Sends one request on a new stream and reads one reply, giving up
    /// after `CALL_TIMEOUT`.
    pub async fn call<Q: Serialize, R: DeserializeOwned>(
        &self,
        protocol: &str,
        request: &Q,
    ) -> Result<R, MeshError> {
        self.call_within(CALL_TIMEOUT, protocol, request).await
    }

    /// Like `call`, with the whole exchange limited to `limit`.
    pub async fn call_within<Q: Serialize, R: DeserializeOwned>(
        &self,
        limit: Duration,
        protocol: &str,
        request: &Q,
    ) -> Result<R, MeshError> {
        timeout(limit, async {
            let (_tx, mut rx) = self.open_sending(protocol, request).await?;
            rx.recv()
                .await
                .map_err(MeshError::Link)?
                .ok_or_else(|| MeshError::Link(anyhow!("the node closed the stream")))
        })
        .await
        .map_err(|e| MeshError::Link(anyhow::Error::new(e).context("the node did not answer")))?
    }

    /// Sends one request and yields the replies until the node finishes the
    /// stream. A broken stream yields its error once and then ends.
    pub async fn follow<Q: Serialize, R: DeserializeOwned + Send + 'static>(
        &self,
        protocol: &str,
        request: &Q,
    ) -> Result<impl Stream<Item = Result<R, MeshError>> + use<Q, R>, MeshError> {
        let (tx, rx) = self.open_sending(protocol, request).await?;
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

/// Sends `Open`.
async fn send_open(tx: &mut LinkTx, protocol: &str) -> Result<(), MeshError> {
    let open = Open {
        protocol: protocol.to_owned(),
    };
    tx.send(&open).await.map_err(MeshError::Link)
}

/// Reads the node's answer to `Open`.
async fn answer(rx: &mut LinkRx, protocol: &str) -> Result<(), MeshError> {
    let opened = rx
        .recv::<Opened>()
        .await
        .map_err(MeshError::Link)?
        .ok_or_else(|| MeshError::Link(anyhow!("the node closed the stream")))?;
    match opened {
        Opened::Ready {} => Ok(()),
        Opened::Refused { reason } => Err(MeshError::Declined {
            protocol: protocol.to_owned(),
            reason,
        }),
        other => Err(MeshError::Unexpected(format!("{other:?}"))),
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
    use futures::StreamExt;
    use std::time::Instant;
    use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};
    use tokio::time::timeout as within;

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
            "stall/1" => {
                tx.send(&Opened::Ready {}).await.ok();
                rx.recv::<u32>().await.ok();
                std::future::pending::<()>().await;
            }
            other => {
                let reason = format!("this node does not offer {other}");
                tx.send(&Opened::Refused { reason }).await.ok();
            }
        }
    }

    /// Sends until the caller is gone, then says so.
    async fn tick(mut tx: LinkTx, mut rx: LinkRx, gone: UnboundedSender<()>) {
        if rx.recv::<Open>().await.is_err() {
            return;
        }
        tx.send(&Opened::Ready {}).await.ok();
        rx.recv::<u32>().await.ok();
        let mut i = 0u32;
        while tx.send(&i).await.is_ok() {
            i = i.wrapping_add(1);
        }
        gone.send(()).ok();
    }

    fn session() -> SessionFn {
        Arc::new(|tx, rx| Box::pin(serve(tx, rx)))
    }

    #[tokio::test]
    async fn dropping_a_local_follow_reaches_the_session() {
        let (gone, mut gone_rx) = unbounded_channel();
        let session: SessionFn = Arc::new(move |tx, rx| Box::pin(tick(tx, rx, gone.clone())));
        let client = PeerClient::local(session);
        let mut items = Box::pin(
            client
                .follow::<u32, u32>("tick/1", &0)
                .await
                .expect("opened"),
        );
        items.next().await.expect("an item").expect("ok");
        drop(items);
        within(Duration::from_secs(60), gone_rx.recv())
            .await
            .expect("the session noticed")
            .expect("signal");
    }

    #[tokio::test(start_paused = true)]
    async fn a_call_to_a_stalled_node_gives_up_at_its_limit() {
        let client = PeerClient::local(session());
        let start = tokio::time::Instant::now();
        let limit = Duration::from_secs(5);
        let reply = within(
            Duration::from_secs(600),
            client.call_within::<u32, u32>(limit, "stall/1", &0),
        )
        .await
        .expect("returned by itself");
        assert!(matches!(reply, Err(MeshError::Link(_))), "{reply:?}");
        assert!(start.elapsed() >= limit);
        assert!(start.elapsed() <= limit + Duration::from_millis(2));
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
    async fn a_local_call_costs_under_five_milliseconds() {
        let client = PeerClient::local(session());
        let mut fastest = Duration::MAX;
        for _ in 0..200 {
            let start = Instant::now();
            let _: u32 = client.call("echo/1", &1u32).await.expect("answered");
            fastest = fastest.min(start.elapsed());
        }
        eprintln!("fastest local call: {fastest:?}");
        // The fastest of 200 is steady on a loaded box; 5 ms is far above it.
        assert!(fastest < Duration::from_millis(5), "{fastest:?}");
    }

    /// A node that answers `Open` only after it read the request, as a slow
    /// link makes it look: a call that waits for the answer before sending
    /// the request never finishes.
    async fn eager(mut tx: LinkTx, mut rx: LinkRx) {
        let Ok(Some(_)) = rx.recv::<Open>().await else {
            return;
        };
        let Ok(Some(n)) = rx.recv::<u32>().await else {
            return;
        };
        tx.send(&Opened::Ready {}).await.ok();
        tx.send(&(n + 1)).await.ok();
    }

    #[tokio::test(start_paused = true)]
    async fn a_call_sends_its_request_right_behind_open() {
        let client = PeerClient::local(Arc::new(|tx, rx| Box::pin(eager(tx, rx))));
        let reply = client
            .call_within::<u32, u32>(Duration::from_secs(5), "eager/1", &41)
            .await;
        assert_eq!(reply.expect("answered"), 42);
    }

    #[tokio::test(start_paused = true)]
    async fn a_refused_call_says_why() {
        let client = PeerClient::local(session());
        let err = client
            .call_within::<u32, u32>(Duration::from_secs(5), "nope/1", &1)
            .await
            .expect_err("refused");
        assert_eq!(
            err.to_string(),
            "the node refused nope/1: this node does not offer nope/1"
        );
    }
}
