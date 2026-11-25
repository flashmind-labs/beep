//! Streams to another node of the mesh: a call's stream opened with the
//! session handshake, and one topic of `peer-subscriptions/1` followed over it.
//! It names no other module of the node, so the agents, the team and the
//! elections can all use it.

use anyhow::{Context, Result, bail};
use beep_mesh::Mesh;
use beep_proto::{
    node::{
        LinkRx, LinkTx, Open, Opened,
        events::{HeldSessions, Topic, Value},
        link,
        subscriptions::{self, Open as SubscriptionsOpen, Reply, Request},
    },
    watched,
};
use iroh::EndpointId;
use std::time::Duration;
use tokio::time::timeout;

/// How many times opening a call's stream is tried.
const HANDSHAKES: u32 = 3;
/// How long all attempts to connect and open a call's stream have together.
const OPEN_LIMIT: Duration = Duration::from_secs(15);
/// A silent stream is lost after this long without a message; the peer
/// sends one every `HEARTBEAT`.
const IDLE: Duration = Duration::from_secs(35);

/// The machine turned a stream away, which trying again does not change.
#[derive(Debug, thiserror::Error)]
#[error("the machine refused the call: {0}")]
pub(crate) struct Refused(pub(crate) String);

/// Opens `protocol` on `peer`. A link replaced while it opens (two machines
/// dialing each other keep one) is tried again; nothing has run yet.
pub(crate) async fn handshake(
    mesh: &Mesh,
    peer: EndpointId,
    protocol: &str,
) -> Result<(LinkTx, LinkRx)> {
    let opening = async {
        let mut tries = HANDSHAKES;

        loop {
            match try_handshake(mesh, peer, protocol).await {
                Err(e) if tries > 1 => {
                    log::debug!("opening {protocol} on {peer} failed, trying again: {e:#}");
                    tries -= 1;
                }
                Err(e) => return Err(e),
                Ok(Ok(streams)) => return Ok(streams),
                Ok(Err(reason)) => return Err(Refused(reason).into()),
            }
        }
    };

    timeout(OPEN_LIMIT, opening).await.context(
        "the machine did not connect and accept the call within 15 seconds; the request was not sent",
    )?
}

/// The streams, or why the machine turned the call away, which trying
/// again does not change.
async fn try_handshake(
    mesh: &Mesh,
    peer: EndpointId,
    protocol: &str,
) -> Result<Result<(LinkTx, LinkRx), String>> {
    let (send, recv) = open(mesh, peer).await?;
    let (mut tx, mut rx) = link(send, recv);
    tx.send(&Open {
        protocol: protocol.to_owned(),
    })
    .await?;
    let opened: Opened = rx.recv().await?.context("the machine closed the call")?;

    Ok(match opened {
        Opened::Refused { reason } => Err(reason),
        _ => Ok((tx, rx)),
    })
}

/// A stream to `peer`, waiting for a link within the handshake's deadline.
async fn open(
    mesh: &Mesh,
    peer: EndpointId,
) -> Result<(iroh::endpoint::SendStream, iroh::endpoint::RecvStream)> {
    let mut updates = mesh.updates();

    loop {
        watched::read_and_update(&mut updates, |_| ());

        match mesh.open_bi(peer).await {
            Ok(streams) => return Ok(streams),
            Err(e) => {
                if updates.changed().await.is_err() {
                    return Err(e).context("the mesh closed while waiting for the machine");
                }
            }
        }
    }
}

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
        tx.send(&SubscriptionsOpen {}).await?;

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
