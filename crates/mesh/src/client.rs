//! The client side of a node's sessions: open a stream to a peer's
//! `session()`, or to the node in this process with no network at all.

use crate::{Mesh, MeshError};
use beep_proto::node::events::{Read, Value};
use beep_proto::node::{LinkRx, LinkTx, Open, Opened, link, link_on, local_link};
use futures::Stream;
use futures::stream::unfold;
use iroh::EndpointId;
use iroh::endpoint::{
    ClosedStream, Connection, ConnectionError, ReadError, ReadExactError, ReadToEndError,
    StoppedError, WriteError,
};
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::error::Error as StdError;
use std::fmt::{self, Debug, Formatter};
use std::future::Future;
use std::io::{self, ErrorKind};
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};
use thiserror::Error;
use tokio::task::spawn;
use tokio::time::error::Elapsed;
use tokio::time::timeout;

/// How long `with_deadline` waits for a member to come online.
pub const DEADLINE: Duration = Duration::from_secs(3);
/// How long a network open may take, from the new stream to the node's answer.
pub const OPEN_TIMEOUT: Duration = Duration::from_secs(10);

/// How long `call` waits for a node to reply.
pub const CALL_TIMEOUT: Duration = Duration::from_secs(30);

/// Records cancelled calls as well as calls that complete: a caller may
/// impose a shorter deadline than `call_within` itself.
struct CallTiming<'a> {
    protocol: &'a str,
    peer: Option<EndpointId>,
    started: Instant,
    phase_started: Instant,
    phase: &'static str,
    outcome: &'static str,
}

impl Drop for CallTiming<'_> {
    fn drop(&mut self) {
        let elapsed = self.started.elapsed();
        metrics::counter!("beep_peer_calls_total", "protocol" => self.protocol.to_owned(), "outcome" => self.outcome).increment(1);
        metrics::histogram!("beep_peer_call_seconds", "protocol" => self.protocol.to_owned(), "outcome" => self.outcome).record(elapsed.as_secs_f64());
        metrics::histogram!("beep_peer_phase_seconds", "protocol" => self.protocol.to_owned(), "phase" => self.phase, "outcome" => self.outcome)
            .record(self.phase_started.elapsed().as_secs_f64());
        if elapsed >= Duration::from_secs(1) {
            tracing::warn!(
                protocol = self.protocol,
                peer = ?self.peer,
                phase = self.phase,
                outcome = self.outcome,
                elapsed_ms = elapsed.as_millis(),
                phase_ms = self.phase_started.elapsed().as_millis(),
                "slow peer call"
            );
        }
    }
}

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

/// The current value of a subscribed read, or `None` to use an ordinary request.
pub type SnapshotFuture<'a> =
    Pin<Box<dyn Future<Output = Option<Result<Value, MeshError>>> + Send + 'a>>;

/// A caller-owned source of live read projections.
pub trait SnapshotReader: Send + Sync {
    fn snapshot(&self, read: Read) -> SnapshotFuture<'_>;
}

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
    peer: Option<EndpointId>,
    snapshots: Option<Arc<dyn SnapshotReader>>,
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
        let peer = connection.remote_id();
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
            peer: Some(peer),
            snapshots: None,
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
            peer: Some(peer),
            snapshots: None,
        }
    }

    /// A client for the node in this process. No network: each stream is a
    /// pair of in-memory pipes with `session` serving the far end.
    pub fn local(session: SessionFn) -> Self {
        Self {
            via: Via::Local(session),
            peer: None,
            snapshots: None,
        }
    }

    /// Uses the owner's current snapshots for subscribed reads.
    pub fn with_snapshot_reader(mut self, snapshots: Arc<dyn SnapshotReader>) -> Self {
        self.snapshots = Some(snapshots);
        self
    }

    /// Reads from a live subscription when one owns this resource.
    pub async fn snapshot(&self, read: Read) -> Option<Result<Value, MeshError>> {
        self.snapshots.as_ref()?.snapshot(read).await
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

    /// Reads the node's answer to `Open`, within `OPEN_TIMEOUT`. The node in
    /// this process gets it too: a stream it never answers would otherwise
    /// hold the caller for good, with nothing in the log (#427).
    async fn confirmed(&self, rx: &mut LinkRx, protocol: &str) -> Result<(), MeshError> {
        timeout(OPEN_TIMEOUT, answer(rx, protocol))
            .await
            .map_err(|e| {
                MeshError::Link(anyhow::Error::new(e).context("the node did not open the stream"))
            })?
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
        let now = Instant::now();
        let mut timing = CallTiming {
            protocol,
            peer: self.peer,
            started: now,
            phase_started: now,
            phase: "open",
            outcome: "cancelled",
        };
        let result = timeout(limit, async {
            let (_tx, mut rx) = self.open_sending(protocol, request).await?;
            metrics::histogram!("beep_peer_phase_seconds", "protocol" => protocol.to_owned(), "phase" => "open", "outcome" => "ok")
                .record(timing.phase_started.elapsed().as_secs_f64());
            timing.phase = "reply";
            timing.phase_started = Instant::now();
            rx.recv()
                .await
                .map_err(MeshError::Link)?
                .ok_or_else(|| {
                    MeshError::Link(
                        io::Error::new(ErrorKind::UnexpectedEof, "the node closed the stream").into(),
                    )
                })
        })
        .await;
        match result {
            Ok(result) => {
                timing.outcome = if result.is_ok() { "ok" } else { "error" };
                result
            }
            Err(e) => {
                timing.outcome = "timeout";
                Err(MeshError::Link(
                    anyhow::Error::new(e).context("the node did not answer"),
                ))
            }
        }
    }

    /// Sends one request and yields the replies until the node finishes the
    /// stream. A broken stream yields its error once, logged, and then ends.
    pub async fn follow<Q: Serialize, R: DeserializeOwned + Send + 'static>(
        &self,
        protocol: &str,
        request: &Q,
    ) -> Result<impl Stream<Item = Result<R, MeshError>> + use<Q, R>, MeshError> {
        let (tx, rx) = self.open_sending(protocol, request).await?;
        let protocol = protocol.to_owned();
        // `tx` rides along: finishing our half would read as a cancel.
        Ok(unfold(Some((tx, rx, protocol)), |state| async move {
            let (tx, mut rx, protocol) = state?;
            match rx.recv::<R>().await {
                Ok(Some(item)) => Some((Ok(item), Some((tx, rx, protocol)))),
                Ok(None) => None,
                Err(e) => {
                    tracing::warn!("a {protocol} stream broke: {e:#}");
                    Some((Err(MeshError::Link(e)), None))
                }
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
        .ok_or_else(|| {
            MeshError::Link(
                io::Error::new(ErrorKind::UnexpectedEof, "the node closed the stream").into(),
            )
        })?;
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

impl MeshError {
    /// Whether the call failed on its way to or from the node (the stream
    /// stopped or reset, the connection lost or closed, no answer in time,
    /// the node not reachable) rather than in what the node said. Such a
    /// call can be made again once the node is back.
    pub fn is_transport(&self) -> bool {
        match self {
            Self::Net(_) | Self::Closed => true,
            Self::Link(e) => e.chain().any(dropped),
            _ => false,
        }
    }
}

/// Whether `cause` is the network failing: QUIC's own errors, the I/O
/// errors the framed streams turn them into, a timeout, a peer offline.
fn dropped(cause: &(dyn StdError + 'static)) -> bool {
    if let Some(e) = cause.downcast_ref::<io::Error>() {
        if let Some(inner) = e.get_ref() {
            return dropped(inner);
        }
        return matches!(
            e.kind(),
            ErrorKind::ConnectionReset
                | ErrorKind::ConnectionAborted
                | ErrorKind::NotConnected
                | ErrorKind::BrokenPipe
                | ErrorKind::UnexpectedEof
                | ErrorKind::TimedOut
        );
    }
    if let Some(e) = cause.downcast_ref::<MeshError>() {
        return e.is_transport();
    }
    cause.is::<WriteError>()
        || cause.is::<ReadError>()
        || cause.is::<ReadExactError>()
        || cause.is::<ReadToEndError>()
        || cause.is::<StoppedError>()
        || cause.is::<ClosedStream>()
        || cause.is::<ConnectionError>()
        || cause.is::<Elapsed>()
        || cause.is::<Offline>()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::machine::said;
    use iroh::SecretKey;
    use iroh::endpoint::VarInt;
    use std::future::pending;

    struct CurrentSessions;

    impl SnapshotReader for CurrentSessions {
        fn snapshot(&self, read: Read) -> SnapshotFuture<'_> {
            Box::pin(async move {
                match read {
                    Read::Sessions(request)
                        if matches!(
                            request.as_ref(),
                            beep_proto::node::agent::SessionsRequest::ListAll {}
                        ) =>
                    {
                        Some(Ok(Value::Sessions(Box::new(
                            beep_proto::node::agent::SessionsReply::Sessions {
                                sessions: Vec::new(),
                            },
                        ))))
                    }
                    Read::SignIn { tool } if tool == "codex" => Some(Ok(Value::SignIn(
                        beep_proto::node::sign_in::State::SignedIn {},
                    ))),
                    _ => None,
                }
            })
        }
    }

    #[tokio::test]
    async fn a_subscribed_sign_in_state_uses_its_snapshot_without_opening_an_rpc() {
        let unavailable: SessionFn =
            Arc::new(|_, _| panic!("a cached read must not open a stream"));
        let client = PeerClient::local(unavailable).with_snapshot_reader(Arc::new(CurrentSessions));
        assert_eq!(
            crate::machine::sign_in(
                &client,
                &beep_proto::node::sign_in::Request::State {
                    tool: "codex".into()
                }
            )
            .await
            .unwrap(),
            beep_proto::node::sign_in::State::SignedIn {}
        );
    }

    #[tokio::test]
    async fn a_subscribed_session_read_uses_its_snapshot_without_opening_an_rpc() {
        let unavailable: SessionFn =
            Arc::new(|_, _| panic!("a cached read must not open a stream"));
        let client = PeerClient::local(unavailable).with_snapshot_reader(Arc::new(CurrentSessions));
        assert!(
            crate::machine::sessions::list_all(&client)
                .await
                .unwrap()
                .is_empty()
        );
    }

    fn link(e: impl StdError + Send + Sync + 'static) -> MeshError {
        MeshError::Link(anyhow::Error::new(e))
    }

    #[test]
    fn a_stream_the_node_stopped_or_reset_is_transport() {
        // What a framed write hands back: the QUIC error inside an I/O error.
        let stopped = io::Error::from(WriteError::Stopped(VarInt::from_u32(0)));
        assert_eq!(stopped.to_string(), "sending stopped by peer: error 0");
        assert!(link(stopped).is_transport());
        let reset = io::Error::from(ReadError::Reset(VarInt::from_u32(0)));
        assert!(link(reset).is_transport());
        let lost = anyhow::Error::new(ConnectionError::TimedOut).context("opening");
        assert!(MeshError::Link(lost).is_transport());
        assert!(link(Offline(SecretKey::generate().public())).is_transport());
        assert!(MeshError::Net("not connected".into()).is_transport());
        // `PeerClient::over` wraps the mesh's own error.
        let wrapped = anyhow::Error::new(MeshError::Net("not connected".into()));
        assert!(MeshError::Link(wrapped).is_transport());
    }

    #[tokio::test(start_paused = true)]
    async fn no_answer_in_time_is_transport() {
        let elapsed = timeout(CALL_TIMEOUT, pending::<()>())
            .await
            .expect_err("a pending future times out");
        let e = anyhow::Error::new(elapsed).context("the node did not answer");
        assert!(MeshError::Link(e).is_transport());
    }

    /// A node in this process that never answers `Open` fails the open in
    /// time instead of holding the caller for good (#427).
    #[tokio::test(start_paused = true)]
    async fn a_local_node_that_never_answers_fails_the_open() {
        let silent: SessionFn = Arc::new(|tx: LinkTx, rx: LinkRx| {
            Box::pin(async move {
                let _held = (tx, rx);
                pending::<()>().await;
            })
        });
        let opened = PeerClient::local(silent)
            .open_sending("codex/1", &"resume")
            .await;
        let Err(e) = opened else {
            panic!("no answer is an error");
        };
        assert!(e.is_transport(), "{e:#}");
    }

    #[test]
    fn what_the_node_said_is_not_transport() {
        assert!(!said("no such session").is_transport());
        assert!(!link(io::Error::other("bad frame")).is_transport());
        let declined = MeshError::Declined {
            protocol: "sessions/1".into(),
            reason: "not served".into(),
        };
        assert!(!declined.is_transport());
        assert!(!MeshError::Unexpected("Pong".into()).is_transport());
    }
}
