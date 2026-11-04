//! The node in its mesh: `beep-peer/1` and `beep-join/1` on the node's
//! endpoint, and what a peer gets from the node. A desktop gets what the
//! server gets, every session. Another node gets `status/1` and `tools/1`
//! only. A node never mints a ticket or adds a device; it only joins one.
//!
//! The protocols are served from the start and answer once the node is in a
//! mesh, so `join` takes effect without a restart.

use crate::{
    link::connection::{Offers, Who, changed, session},
    link::local_socket::{Listener, Stream},
    machine::{env, stats},
};
use anyhow::{Context, Result, bail};
use beep_mesh::{
    DeviceKind, DeviceStatus, JoinProtocol, JoinTicket, Mesh, MeshProtocol, PeerStream,
    StreamHandler,
};
use beep_proto::node::{LinkRx, LinkTx, status::Report};
use iroh::{
    Endpoint,
    endpoint::Connection,
    protocol::{AcceptError, ProtocolHandler},
};
use iroh_tickets::Ticket;
use serde::de::IgnoredAny;
use std::{
    fmt::Write,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, PoisonError},
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader, split},
    time::{MissedTickBehavior, interval},
};

/// The socket `beep-node mesh` reaches the running node on, in its folder.
const SOCKET: &str = "mesh.sock";
/// `join <ticket>`; the node answers `joined`, or `failed <why>`.
const JOIN: &str = "join ";
/// The node answers one line per device, then `end`.
const DEVICES: &str = "devices";

/// How often the machine is probed again for changes.
const REPROBE: Duration = Duration::from_secs(10 * 60);
/// How often free disk and memory are sampled.
const RESAMPLE: Duration = Duration::from_secs(60);
/// How often the traffic counters are reported.
const REPORT_TRAFFIC: Duration = Duration::from_secs(60);

/// What the node holds once it is in a mesh. The mesh stops with its last
/// clone.
#[derive(Clone)]
struct Active {
    mesh: Mesh,
    peer: MeshProtocol,
    join: JoinProtocol,
}

struct Inner {
    endpoint: Endpoint,
    dir: PathBuf,
    name: String,
    offers: Offers,
    active: Mutex<Option<Active>>,
}

/// The node's membership, shared by the router, the control socket and the
/// shutdown.
#[derive(Clone)]
pub(crate) struct Peers {
    inner: Arc<Inner>,
}

impl Peers {
    pub(crate) fn new(endpoint: Endpoint, dir: PathBuf, name: String, offers: Offers) -> Self {
        Self {
            inner: Arc::new(Inner {
                endpoint,
                dir,
                name,
                offers,
                active: Mutex::new(None),
            }),
        }
    }

    /// The `beep-peer/1` handler.
    pub(crate) fn peer_handler(&self) -> PeerAlpn {
        PeerAlpn(self.clone())
    }

    /// The `beep-join/1` handler.
    pub(crate) fn join_handler(&self) -> JoinAlpn {
        JoinAlpn(self.clone())
    }

    /// Takes the mesh the node's folder holds, if it holds one.
    pub(crate) async fn open(&self) -> Result<bool> {
        let dir = self.inner.dir.clone();
        let opened = Mesh::open(&dir, self.inner.endpoint.clone())
            .await
            .context("could not open the mesh")?;
        Ok(match opened {
            Some(mesh) => {
                self.install(mesh);
                true
            }
            None => false,
        })
    }

    /// Joins the mesh a desktop's ticket names. The ticket is read once and
    /// never logged or put in an error.
    pub(crate) async fn join(&self, ticket: &str) -> Result<()> {
        let ticket = JoinTicket::decode_string(ticket.trim())
            .map_err(|_| anyhow::anyhow!("that is not a join ticket"))?;
        if self.is_member() {
            bail!("this node already belongs to a mesh");
        }
        let mesh = Mesh::join(
            &self.inner.dir,
            self.inner.endpoint.clone(),
            &ticket,
            &self.inner.name,
        )
        .await
        .context("could not join")?;
        self.install(mesh);
        Ok(())
    }

    pub(crate) fn is_member(&self) -> bool {
        self.active().is_some()
    }

    /// The mesh's devices, empty before the node joined.
    pub(crate) fn devices(&self) -> Vec<DeviceStatus> {
        self.active()
            .map(|active| active.mesh.devices())
            .unwrap_or_default()
    }

    fn active(&self) -> Option<Active> {
        self.inner
            .active
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn install(&self, mesh: Mesh) {
        let (peer, join) = mesh.protocols();
        peer.on_stream(self.stream_handler());
        *self
            .inner
            .active
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(Active { mesh, peer, join });
    }

    /// Serves each stream a peer opens, by what the peer is now.
    fn stream_handler(&self) -> StreamHandler {
        let offers = self.inner.offers.clone();
        Arc::new(move |stream: PeerStream| {
            let offers = offers.clone();
            Box::pin(async move {
                let PeerStream {
                    peer,
                    kind,
                    send,
                    recv,
                } = stream;
                let who = match kind {
                    DeviceKind::Desktop => Who::Everything,
                    DeviceKind::Node => Who::Node,
                };
                if let Err(e) = session(send, recv, offers, who).await {
                    log::debug!("a stream from {peer} ended: {e:#}");
                }
            })
        })
    }
}

/// Serves `beep-peer/1`.
#[derive(Clone)]
pub(crate) struct PeerAlpn(Peers);

impl std::fmt::Debug for PeerAlpn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PeerAlpn")
    }
}

impl ProtocolHandler for PeerAlpn {
    async fn accept(&self, connection: Connection) -> Result<(), AcceptError> {
        match self.0.active() {
            Some(active) => active.peer.accept(connection).await,
            None => {
                connection.close(1u32.into(), b"not in a mesh");
                Ok(())
            }
        }
    }
}

/// Serves `beep-join/1`; a node refuses it, since only a desktop mints
/// tickets.
#[derive(Clone)]
pub(crate) struct JoinAlpn(Peers);

impl std::fmt::Debug for JoinAlpn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("JoinAlpn")
    }
}

impl ProtocolHandler for JoinAlpn {
    async fn accept(&self, connection: Connection) -> Result<(), AcceptError> {
        match self.0.active() {
            Some(active) => active.join.accept(connection).await,
            None => {
                connection.close(1u32.into(), b"not in a mesh");
                Ok(())
            }
        }
    }
}

/// Serves `status/1`: the offers, then the machine's reports until the peer
/// closes the stream.
pub(crate) async fn status(mut tx: LinkTx, mut rx: LinkRx, offers: Offers) -> Result<()> {
    tx.send(&Report::Offers(offers.names())).await?;
    let mut reprobe = interval(REPROBE);
    let mut resample = interval(RESAMPLE);
    resample.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut traffic = interval(REPORT_TRAFFIC);
    traffic.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut reported = None;
    loop {
        tokio::select! {
            // The peer finished its side: it has no more to say, or left.
            gone = rx.recv::<IgnoredAny>() => {
                gone?;
                return Ok(());
            }
            _ = reprobe.tick() => {
                let now = env::probe().await;
                if reported.as_ref().is_none_or(|before| changed(before, &now)) {
                    tx.send(&Report::Env(now.clone())).await?;
                    reported = Some(now);
                }
            }
            _ = resample.tick() => {
                let folders = dirs::home_dir().into_iter().collect();
                tx.send(&Report::Stats(stats::sample(folders).await)).await?;
            }
            _ = traffic.tick() => tx.send(&Report::Traffic(offers.traffic.report())).await?,
        }
    }
}

/// Takes the mesh commands on the node's socket until the node stops.
pub(crate) async fn listen(dir: &Path, peers: Peers) -> Result<()> {
    let mut listener = Listener::bind(&dir.join(SOCKET))?;
    log::info!("taking mesh commands on the control socket");
    loop {
        let stream = listener.accept().await.context("taking a mesh command")?;
        if let Err(e) = answer(stream, &peers).await {
            log::debug!("a mesh command failed: {e:#}");
        }
    }
}

async fn answer(stream: Stream, peers: &Peers) -> Result<()> {
    let (read, mut write) = split(stream);
    let request = BufReader::new(read.take(MAX_REQUEST))
        .lines()
        .next_line()
        .await?
        .unwrap_or_default();
    let reply = if let Some(ticket) = request.strip_prefix(JOIN) {
        match peers.join(ticket).await {
            Ok(()) => {
                log::info!("joined a mesh");
                "joined\n".to_owned()
            }
            Err(e) => format!("failed {e:#}\n"),
        }
    } else if request == DEVICES {
        let mut out = String::new();
        for status in peers.devices() {
            let device = status.device;
            let online = if status.online { "online" } else { "offline" };
            writeln!(
                out,
                "{} {online} {} {}",
                device.kind, device.id, device.name
            )?;
        }
        out.push_str("end\n");
        out
    } else {
        return Ok(());
    };
    write.write_all(reply.as_bytes()).await?;
    Ok(())
}

/// The longest request the node reads; a ticket is far shorter.
const MAX_REQUEST: u64 = 16 * 1024;

/// Joins the mesh `ticket` names through the node running from `dir`. The
/// ticket goes over the node's own socket, which only its user can open.
#[cfg(feature = "cli")]
pub async fn join(dir: &Path, ticket: &str) -> Result<()> {
    let lines = ask(dir, &format!("{JOIN}{}", ticket.trim())).await?;
    match lines.first().map(String::as_str) {
        Some("joined") => Ok(()),
        Some(failed) => bail!("{}", failed.strip_prefix("failed ").unwrap_or(failed)),
        None => bail!("the node closed without answering"),
    }
}

/// The devices of the mesh the node running from `dir` belongs to, one
/// printable line each.
#[cfg(feature = "cli")]
pub async fn devices(dir: &Path) -> Result<Vec<String>> {
    let mut lines = ask(dir, DEVICES).await?;
    lines.retain(|line| line != "end");
    Ok(lines)
}

#[cfg(feature = "cli")]
async fn ask(dir: &Path, request: &str) -> Result<Vec<String>> {
    use crate::link::local_socket;
    use tokio::time::timeout;
    /// How long a command waits for the running node.
    const ASK_LIMIT: Duration = Duration::from_secs(60);
    let stream = local_socket::connect(&dir.join(SOCKET))
        .await
        .context("no node is running from this folder; start it with `beep-node run`")?;
    let (read, mut write) = split(stream);
    write.write_all(format!("{request}\n").as_bytes()).await?;
    let mut lines = BufReader::new(read).lines();
    let mut answer = Vec::new();
    timeout(ASK_LIMIT, async {
        while let Some(line) = lines.next_line().await? {
            let end = line == "end";
            answer.push(line);
            if end {
                break;
            }
        }
        anyhow::Ok(())
    })
    .await
    .context("the node took too long to answer")??;
    Ok(answer)
}
