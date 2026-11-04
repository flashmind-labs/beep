//! The node in its mesh: `beep-peer/1` and `beep-join/1` on the node's
//! endpoint, and what a peer gets from the node. A desktop gets what the
//! server gets, every session. Another node gets `status/1` and `tools/1`
//! only. A node never mints a ticket or adds a device; it only joins one.
//!
//! The protocols are served from the start and answer once the node is in a
//! mesh, so `join` takes effect without a restart.

use crate::{
    files::transfer::Transfers,
    link::connection::{Offers, Protocol, Who, changed, read_open, serve_open},
    link::from_mesh,
    link::local_socket::{Listener, Stream},
    machine::{env, stats},
};
use anyhow::{Context, Result, bail};
use beep_mesh::{
    DeviceKind, DeviceStatus, JoinProtocol, JoinTicket, Mesh, MeshProtocol, PeerStream, SessionFn,
    StreamHandler,
};
use beep_proto::node::{LinkRx, LinkTx, link, status::Report};
use iroh::{
    Endpoint,
    endpoint::Connection,
    protocol::{AcceptError, ProtocolHandler},
};
use iroh_tickets::Ticket;
use serde::de::IgnoredAny;
use std::{
    collections::BTreeSet,
    fmt::Write,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard, PoisonError},
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader, split},
    sync::watch,
    task::JoinSet,
    time::{MissedTickBehavior, interval, timeout},
};
use tokio_util::task::AbortOnDropHandle;

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
    offers: Mutex<Offers>,
    /// Protocols a host will offer once its services are up.
    later: watch::Sender<BTreeSet<String>>,
    active: Mutex<Option<Active>>,
    /// Applies the mesh's state to the node while it is a member.
    follower: Mutex<Option<AbortOnDropHandle<()>>>,
}

impl Inner {
    fn offers(&self) -> MutexGuard<'_, Offers> {
        self.offers.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn follower(&self) -> MutexGuard<'_, Option<AbortOnDropHandle<()>>> {
        self.follower.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Serves a stream a peer, or this process, opened.
    async fn serve(&self, tx: LinkTx, mut rx: LinkRx, who: Who) -> Result<()> {
        let open = read_open(&mut rx).await?;
        // A protocol still on its way is waited for; the others are not held up.
        let mut later = self.later.subscribe();
        if later
            .wait_for(|later| !later.contains(&open.protocol))
            .await
            .is_err()
        {
            bail!("the node stopped");
        }
        let offers = self.offers().clone();
        serve_open(tx, rx, open, offers, who).await
    }
}

/// The node's membership, shared by the router, the control socket and the
/// shutdown. A host that is a node itself, such as the Mac app, makes one
/// with its own mesh through `install`.
#[derive(Clone)]
pub struct Peers {
    inner: Arc<Inner>,
}

impl Peers {
    pub fn new(endpoint: Endpoint, dir: PathBuf, name: String, offers: Offers) -> Self {
        Self {
            inner: Arc::new(Inner {
                endpoint,
                dir,
                name,
                offers: Mutex::new(offers),
                later: watch::channel(BTreeSet::new()).0,
                active: Mutex::new(None),
                follower: Mutex::new(None),
            }),
        }
    }

    /// Adds a protocol to what peers can open, for a host that starts it
    /// after the node is up.
    pub fn offer(&self, protocol: Arc<dyn Protocol>) {
        self.inner.offers().extra.push(protocol);
    }

    /// Names protocols a later `offer` will add, so a session that opens one
    /// waits for it instead of being refused. `settle` ends the wait.
    pub fn expect(&self, protocols: &[&str]) {
        self.inner.later.send_modify(|later| {
            later.extend(protocols.iter().map(|name| (*name).to_owned()));
        });
    }

    /// Nothing more is coming: waiting sessions go on, to be served or refused.
    pub fn settle(&self) {
        self.inner.later.send_modify(BTreeSet::clear);
    }

    /// Lets go of the mesh, so it can stop: nothing in the node holds it
    /// once this returns. Peers are refused until `install`.
    pub async fn leave(&self) {
        let follower = self.inner.follower().take();
        if let Some(follower) = follower {
            follower.abort();
            // Returns once the task, and the mesh it held, is gone.
            follower.await.ok();
        }
        let offers = self.inner.offers().clone();
        offers.mesh.clear();
        if let Some(transfers) = &offers.transfers {
            transfers.syncs().unfollow();
        }
        self.inner
            .active
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
    }

    /// The `beep-peer/1` handler.
    pub fn peer_handler(&self) -> PeerAlpn {
        PeerAlpn(self.clone())
    }

    /// The `beep-join/1` handler.
    pub fn join_handler(&self) -> JoinAlpn {
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

    /// Whether this device is a desktop in the current device list.
    pub fn is_desktop(&self) -> bool {
        let me = self.inner.endpoint.id();
        self.devices()
            .iter()
            .any(|status| status.device.id == me && status.device.kind == DeviceKind::Desktop)
    }

    pub fn is_member(&self) -> bool {
        self.active().is_some()
    }

    /// The mesh's devices, empty before the node joined.
    pub fn devices(&self) -> Vec<DeviceStatus> {
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

    /// Takes `mesh` as the node's membership; its peers' streams are served
    /// from here on.
    pub fn install(&self, mesh: Mesh) {
        let (peer, join) = mesh.protocols();
        peer.on_stream(self.stream_handler());
        *self
            .inner
            .active
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(Active { mesh, peer, join });
        self.follow();
    }

    /// From here on the mesh, not the server, says what the node syncs,
    /// holds and runs agents with. Starts over on the current mesh and
    /// offers, which a service started later may have added to, and ends
    /// what followed before.
    fn follow(&self) {
        let Some(active) = self.active() else {
            return;
        };
        let offers = self.inner.offers().clone();
        offers.mesh.set(active.mesh.clone());
        if let Some(transfers) = &offers.transfers {
            transfers.syncs().follow(active.mesh.clone());
        }
        let follow = from_mesh::follow(active.mesh, self.inner.endpoint.id(), offers);
        *self.inner.follower() = Some(AbortOnDropHandle::new(tokio::spawn(follow)));
    }

    /// The node's transfer and sync services started: the mesh drives them.
    pub fn serve_transfers(&self, transfers: Transfers) {
        self.inner.offers().transfers = Some(transfers);
        self.follow();
    }

    /// The node's session for a caller in this process, which is a desktop
    /// by being the user at this machine: no network, every protocol.
    pub fn local_session(&self) -> SessionFn {
        let peers = self.clone();
        Arc::new(move |tx: LinkTx, rx: LinkRx| {
            let peers = peers.clone();
            Box::pin(async move {
                if let Err(e) = peers.inner.serve(tx, rx, Who::Everything).await {
                    log::debug!("a local session ended: {e:#}");
                }
            })
        })
    }

    /// Serves each stream a peer opens, by what the peer is now.
    fn stream_handler(&self) -> StreamHandler {
        // Weak: the mesh holds this handler, and the node holds the mesh.
        let peers = Arc::downgrade(&self.inner);
        Arc::new(move |stream: PeerStream| {
            let peers = peers.clone();
            Box::pin(async move {
                let Some(inner) = peers.upgrade() else {
                    return;
                };
                let desktop_here = Peers {
                    inner: inner.clone(),
                }
                .is_desktop();
                let PeerStream {
                    peer,
                    kind,
                    send,
                    recv,
                } = stream;
                let who = match kind {
                    DeviceKind::Desktop => Who::Everything,
                    DeviceKind::Node => Who::Node { peer, desktop_here },
                };
                let (tx, rx) = link(send, recv);
                if let Err(e) = inner.serve(tx, rx, who).await {
                    log::debug!("a stream from {peer} ended: {e:#}");
                }
            })
        })
    }
}

/// Serves `beep-peer/1`.
#[derive(Clone)]
pub struct PeerAlpn(Peers);

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
pub struct JoinAlpn(Peers);

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
    let mut commands = JoinSet::new();
    log::info!("taking mesh commands on the control socket");
    loop {
        let stream = listener.accept().await.context("taking a mesh command")?;
        if commands.len() >= MAX_COMMANDS {
            log::warn!("refused a mesh command: {MAX_COMMANDS} already open");
            continue;
        }
        let peers = peers.clone();
        commands.spawn(async move {
            match timeout(COMMAND_LIMIT, answer(stream, &peers)).await {
                Ok(Ok(())) => {}
                Ok(Err(e)) => log::debug!("a mesh command failed: {e:#}"),
                Err(_) => log::debug!("a mesh command took too long"),
            }
        });
        // Finished commands are collected as they come.
        while commands.try_join_next().is_some() {}
    }
}

/// The most mesh commands open at once.
const MAX_COMMANDS: usize = 8;
/// How long a command may take; a join dials a desktop, which has its own
/// shorter limits.
const COMMAND_LIMIT: Duration = Duration::from_secs(60);

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
