//! The node in its mesh: `beep-peer/2` and `beep-join/2` on the node's
//! endpoint, and what a peer gets from the node. A desktop gets what the
//! server gets, every session. Another node gets `status/1` and `tools/1`
//! only. A node never mints a ticket or adds a device; it only joins one.
//!
//! The protocols are served from the start and answer once the node is in a
//! mesh, so `join` takes effect without a restart.

use crate::{
    files::transfer::Transfers,
    link::connection::{Offers, Protocol, Who, changed, read_open, serve_open, withheld},
    link::from_mesh,
    link::local_socket::{Listener, Stream},
    link::local_status::{self, LocalStatus},
    link::watchdog::{self, Step},
    machine::{env, stats},
};
use anyhow::{Context, Result, bail};
use beep_mesh::{
    DeviceKind, DeviceStatus, InUse, JoinProtocol, JoinTicket, Mesh, MeshProtocol, PeerStream,
    SessionFn, StreamHandler,
};
use beep_proto::node::{EnvReport, LinkRx, LinkTx, Opened, link, status::Report};
use iroh::{
    Endpoint,
    endpoint::Connection,
    protocol::{AcceptError, ProtocolHandler},
};
use serde::de::IgnoredAny;
use std::{
    collections::BTreeSet,
    fmt::Write,
    future::Future,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard, PoisonError},
    time::{Duration, SystemTime, UNIX_EPOCH},
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
/// `leave`; the node answers `left`, or `failed <why>`.
const LEAVE: &str = "leave";
/// The node answers a `LocalStatus` in compact msgpack, then closes. A node
/// from before it closes without answering.
const STATUS: &str = "status";

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
    /// When the node started.
    started: SystemTime,
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
        // The person at this machine is not held to the setting; a device of the mesh is.
        if who.id() != self.endpoint.id()
            && let Some(reason) = withheld(&open.protocol, offers.lends_to_peers())
        {
            let mut tx = tx;
            return tx.send(&Opened::Refused { reason }).await;
        }
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
                started: SystemTime::now(),
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
        if let Some(transfers) = offers.transfers.get() {
            transfers.syncs().unfollow();
        }
        self.inner
            .active
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
    }

    /// One of the active protocols, without a handle on the mesh itself.
    fn protocols<P>(&self, take: impl FnOnce(&Active) -> P) -> Option<P> {
        self.inner
            .active
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
            .map(take)
    }

    /// The `beep-peer/2` handler.
    pub fn peer_handler(&self) -> PeerAlpn {
        PeerAlpn(self.clone())
    }

    /// The `beep-join/2` handler.
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
    /// never logged or put in an error. A node whose mesh removed it has no
    /// mesh to keep and takes the new one in its place.
    pub(crate) async fn join(&self, ticket: &str) -> Result<()> {
        let ticket = JoinTicket::parse(ticket)?;
        let old = self.active().map(|active| active.mesh);
        if old.as_ref().is_some_and(|mesh| mesh.is_member(mesh.me())) {
            bail!("this node already belongs to a mesh");
        }
        let mesh = match old {
            Some(old) => {
                self.leave().await;
                close_within(old).await?;
                Mesh::join_replacing_lone(
                    &self.inner.dir,
                    self.inner.endpoint.clone(),
                    &ticket,
                    &self.inner.name,
                )
                .await
            }
            None => {
                Mesh::join(
                    &self.inner.dir,
                    self.inner.endpoint.clone(),
                    &ticket,
                    &self.inner.name,
                )
                .await
            }
        }
        .context("could not join")?;
        self.install(mesh);
        Ok(())
    }

    /// Leaves the mesh for good and deletes what it kept: the node is in none
    /// after this, and can join another.
    pub(crate) async fn forget(&self) -> Result<()> {
        let old = self.active().map(|active| active.mesh);
        self.leave().await;
        if let Some(old) = old {
            close_within(old).await?;
        }
        Mesh::forget(&self.inner.dir)
            .await
            .context("could not delete the mesh")
    }

    /// Whether this device is a desktop in the current device list.
    pub fn is_desktop(&self) -> bool {
        let me = self.inner.endpoint.id();
        self.devices()
            .iter()
            .any(|status| status.device.id == me && status.device.kind == DeviceKind::Desktop)
    }

    /// Whether this device is in a mesh it was not removed from.
    pub fn is_member(&self) -> bool {
        self.active()
            .is_some_and(|active| active.mesh.is_member(active.mesh.me()))
    }

    /// The mesh's devices, empty before the node joined.
    pub fn devices(&self) -> Vec<DeviceStatus> {
        self.active()
            .map(|active| active.mesh.devices())
            .unwrap_or_default()
    }

    /// What `beep-node status` shows. Checks each project's folder on disk.
    async fn local_status(&self) -> Result<LocalStatus> {
        let active = self.active();
        let mesh = active
            .as_ref()
            .filter(|active| active.mesh.is_member(active.mesh.me()))
            .map(|active| active.mesh.clone());
        let devices = mesh
            .as_ref()
            .map(Mesh::devices)
            .unwrap_or_default()
            .into_iter()
            .map(|status| local_status::Peer {
                id: status.device.id,
                name: status.device.name,
                kind: status.device.kind,
                online: status.online,
                last_seen: status.last_seen.map(unix_secs),
            })
            .collect();
        let syncs = self
            .inner
            .offers()
            .transfers
            .get()
            .map(|transfers| transfers.syncs());
        let projects = match syncs {
            Some(syncs) => {
                let held = syncs.held();
                let statuses = syncs.status();
                // A folder on a disk that hangs must not hold a worker.
                tokio::task::spawn_blocking(move || {
                    statuses
                        .into_iter()
                        .map(|status| local_status::Project {
                            held: held.contains(&status.name),
                            missing: !Path::new(&status.folder).exists(),
                            status,
                        })
                        .collect()
                })
                .await?
            }
            None => Vec::new(),
        };
        Ok(LocalStatus {
            version: env!("CARGO_PKG_VERSION").to_owned(),
            started: unix_secs(self.inner.started),
            name: self.inner.name.clone(),
            id: self.inner.endpoint.id(),
            mesh: mesh.map(|mesh| mesh.id()),
            devices,
            projects,
        })
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
        if let Some(transfers) = offers.transfers.get() {
            transfers.syncs().follow(active.mesh.clone());
        }
        let follow = from_mesh::follow(
            active.mesh.clone(),
            self.inner.endpoint.id(),
            offers.clone(),
        );
        let skills = crate::agents::skills::follow(
            active.mesh.clone(),
            self.inner.dir.clone(),
            offers.skills.clone(),
        );
        let moves = crate::agents::follow_moves(
            active.mesh,
            self.inner.endpoint.clone(),
            self.inner.dir.clone(),
            offers.agents.clone(),
            offers.transfers.get().map(|transfers| transfers.syncs()),
        );
        *self.inner.follower() = Some(AbortOnDropHandle::new(tokio::spawn(async move {
            tokio::join!(follow, moves, skills);
        })));
    }

    /// The node's transfer and sync services started: the mesh drives them.
    pub fn serve_transfers(&self, transfers: Transfers) {
        self.inner.offers().transfers.set(transfers);
        self.follow();
    }

    /// The node's session for a caller in this process, which is a desktop
    /// by being the user at this machine: no network, every protocol.
    pub fn local_session(&self) -> SessionFn {
        let peers = self.clone();
        Arc::new(move |tx: LinkTx, rx: LinkRx| {
            let peers = peers.clone();
            Box::pin(async move {
                if let Err(e) = peers
                    .inner
                    .serve(tx, rx, Who::Everything(peers.inner.endpoint.id()))
                    .await
                {
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
                    DeviceKind::Desktop => Who::Everything(peer),
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

/// Serves `beep-peer/2`.
#[derive(Clone)]
pub struct PeerAlpn(Peers);

impl std::fmt::Debug for PeerAlpn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PeerAlpn")
    }
}

impl ProtocolHandler for PeerAlpn {
    async fn accept(&self, connection: Connection) -> Result<(), AcceptError> {
        match self.0.protocols(|active| active.peer.clone()) {
            // Only the protocol is kept for the connection's life: `active`
            // holds the mesh, and a leave waits for every holder of that.
            Some(peer) => peer.accept(connection).await,
            None => {
                connection.close(1u32.into(), b"not in a mesh");
                Ok(())
            }
        }
    }
}

/// Serves `beep-join/2`; a node refuses it, since only a desktop mints
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
        match self.0.protocols(|active| active.join.clone()) {
            Some(join) => join.accept(connection).await,
            None => {
                connection.close(1u32.into(), b"not in a mesh");
                Ok(())
            }
        }
    }
}

/// Serves `status/1`: the offers, then the machine's reports until the peer
/// closes the stream.
pub(crate) async fn status(tx: LinkTx, rx: LinkRx, offers: Offers) -> Result<()> {
    status_with(tx, rx, offers, env::probe).await
}

/// `status` with the way to probe the machine given. A probe runs a dozen
/// commands and can take half a minute: it runs beside the loop, so the
/// peer's close and the stats and traffic reports are not held up by it.
async fn status_with<P, F>(mut tx: LinkTx, mut rx: LinkRx, offers: Offers, probe: P) -> Result<()>
where
    P: Fn() -> F,
    F: Future<Output = EnvReport> + Send + 'static,
{
    tx.send(&Report::Offers(offers.names())).await?;
    let mut reprobe = interval(REPROBE);
    reprobe.tick().await;
    let mut probing = Some(AbortOnDropHandle::new(tokio::spawn(probe())));
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
            _ = reprobe.tick(), if probing.is_none() => {
                probing = Some(AbortOnDropHandle::new(tokio::spawn(probe())));
            }
            Some(probed) = async { probing.as_mut()?.await.into() }, if probing.is_some() => {
                probing = None;
                match probed {
                    Ok(now) if reported.as_ref().is_none_or(|before| changed(before, &now)) => {
                        tx.send(&Report::Env(now.clone())).await?;
                        reported = Some(now);
                    }
                    Ok(_) => {}
                    Err(e) => log::warn!("probing the machine failed: {e}"),
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

/// How long a close waits for the tasks that still hold the mesh to let go.
const CLOSE_WAIT: Duration = Duration::from_secs(5);
/// How often a close looks again while it waits.
const CLOSE_POLL: Duration = Duration::from_millis(25);

/// Closes `mesh`, trying again while a task that was just stopped still holds
/// a handle, for at most `CLOSE_WAIT`.
async fn close_within(mut mesh: Mesh) -> Result<()> {
    let deadline = tokio::time::Instant::now() + CLOSE_WAIT;
    loop {
        match mesh.close().await {
            Ok(()) => return Ok(()),
            Err(in_use) if tokio::time::Instant::now() >= deadline => {
                bail!("{in_use}")
            }
            Err(InUse(back)) => {
                mesh = back;
                tokio::time::sleep(CLOSE_POLL).await;
            }
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
            let _running = watchdog::enter(Step::MeshCommand);
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
    } else if request == LEAVE {
        match peers.forget().await {
            Ok(()) => {
                log::info!("left the mesh");
                "left\n".to_owned()
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
    } else if request == STATUS {
        let status = peers.local_status().await?;
        write.write_all(&rmp_serde::to_vec(&status)?).await?;
        write.shutdown().await?;
        return Ok(());
    } else {
        return Ok(());
    };
    write.write_all(reply.as_bytes()).await?;
    Ok(())
}

/// Seconds since the Unix epoch; 0 for a time before it.
fn unix_secs(at: SystemTime) -> u64 {
    at.duration_since(UNIX_EPOCH)
        .map(|since| since.as_secs())
        .unwrap_or(0)
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

/// Has the node running from `dir` leave its mesh and delete what it kept.
#[cfg(feature = "cli")]
pub async fn leave(dir: &Path) -> Result<()> {
    let lines = ask(dir, LEAVE).await?;
    match lines.first().map(String::as_str) {
        Some("left") => Ok(()),
        Some(failed) => bail!("{}", failed.strip_prefix("failed ").unwrap_or(failed)),
        None => bail!("the node closed without answering"),
    }
}

/// Whether a node is running from `dir` and takes mesh commands.
#[cfg(feature = "cli")]
pub fn is_running(dir: &Path) -> bool {
    crate::link::local_socket::is_listening(&dir.join(SOCKET))
}

/// How long `join_when_up` waits for a node that was just started.
#[cfg(feature = "cli")]
pub const START_LIMIT: Duration = Duration::from_secs(15);

/// Joins like `join`, but first waits up to `limit` for the node's socket,
/// which a node started a moment ago has not opened yet. The ticket is sent
/// only once the node accepts a connection, so a node that never comes up
/// does not spend it.
#[cfg(feature = "cli")]
pub async fn join_when_up(dir: &Path, ticket: &str, limit: Duration) -> Result<()> {
    wait_for_node(dir, limit, || {}).await?;
    join(dir, ticket).await
}

/// Retries a connection to the node's socket until `limit` runs out;
/// `retrying` runs after each failed attempt.
#[cfg(feature = "cli")]
async fn wait_for_node(dir: &Path, limit: Duration, mut retrying: impl FnMut()) -> Result<()> {
    use crate::link::local_socket;
    /// The pause between attempts.
    const RETRY: Duration = Duration::from_millis(100);
    let socket = dir.join(SOCKET);
    let attempts = async {
        loop {
            if local_socket::connect(&socket).await.is_ok() {
                return;
            }
            retrying();
            tokio::time::sleep(RETRY).await;
        }
    };
    timeout(limit, attempts).await.map_err(|_| {
        anyhow::anyhow!(
            "the node did not start listening within {}s; run `beep-node run` in another \
             terminal, then join again",
            limit.as_secs()
        )
    })
}

/// The devices of the mesh the node running from `dir` belongs to, one
/// printable line each.
#[cfg(feature = "cli")]
pub async fn devices(dir: &Path) -> Result<Vec<String>> {
    let mut lines = ask(dir, DEVICES).await?;
    lines.retain(|line| line != "end");
    Ok(lines)
}

/// What the node running from `dir` says of itself; `None` when no node
/// runs from there.
#[cfg(feature = "cli")]
pub async fn local_status(dir: &Path) -> Result<Option<LocalStatus>> {
    use crate::link::local_socket;
    /// How long the node may take to answer.
    const STATUS_LIMIT: Duration = Duration::from_secs(20);
    /// The most bytes read: far more than a status takes.
    const MAX_STATUS: u64 = 4 * 1024 * 1024;
    let Ok(stream) = local_socket::connect(&dir.join(SOCKET)).await else {
        return Ok(None);
    };
    let (read, mut write) = split(stream);
    write.write_all(format!("{STATUS}\n").as_bytes()).await?;
    let mut bytes = Vec::new();
    timeout(STATUS_LIMIT, read.take(MAX_STATUS).read_to_end(&mut bytes))
        .await
        .context("the node took too long to answer")??;
    if bytes.is_empty() {
        bail!(
            "the running node is older than this beep-node and cannot say; restart it on \
             this version (`beep-node update`, or restart the service)"
        );
    }
    let status = beep_proto::lenient::from_slice(&bytes).context("reading the node's status")?;
    Ok(Some(status))
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

#[cfg(all(test, feature = "cli"))]
mod tests {
    use super::*;
    use iroh_tickets::Ticket;

    /// Answers the wait's probe, then one join, the way the node does.
    fn serve_probe_then_join(mut listener: Listener) {
        tokio::spawn(async move {
            for _ in 0..2 {
                let stream = listener.accept().await.expect("accepting");
                let (read, mut write) = split(stream);
                let request = BufReader::new(read)
                    .lines()
                    .next_line()
                    .await
                    .expect("reading");
                if request.is_some_and(|line| line.starts_with(JOIN)) {
                    write.write_all(b"joined\n").await.expect("writing");
                }
            }
        });
    }

    #[tokio::test]
    async fn status_reports_and_ends_while_the_probe_is_still_running() {
        use iroh::{SecretKey, endpoint::presets};
        const ALPN: &[u8] = b"status-test";
        let bind = || async {
            Endpoint::builder(presets::Minimal)
                .secret_key(SecretKey::generate())
                .alpns(vec![ALPN.to_vec()])
                .bind()
                .await
                .expect("the endpoint binds")
        };
        let (node, desktop) = (bind().await, bind().await);
        let addr = node.addr();
        let dialing = tokio::spawn({
            let desktop = desktop.clone();
            async move { desktop.connect(addr, ALPN).await }
        });
        let incoming = node.accept().await.expect("a connection comes");
        let from_desktop = incoming.await.expect("it is accepted");
        let to_node = dialing.await.expect("joined").expect("connected");

        // The node serves status with a probe that never finishes.
        let serving = tokio::spawn(async move {
            let (send, recv) = from_desktop.open_bi().await.expect("a stream opens");
            let (tx, rx) = link(send, recv);
            status_with(tx, rx, Offers::default(), std::future::pending).await
        });
        let (send, recv) = to_node.accept_bi().await.expect("the stream arrives");
        let (client_tx, mut reports) = link(send, recv);
        let (mut stats, mut traffic) = (false, false);
        timeout(Duration::from_secs(30), async {
            while !(stats && traffic) {
                match reports.recv::<Report>().await.expect("a report reads") {
                    Some(Report::Stats(_)) => stats = true,
                    Some(Report::Traffic(_)) => traffic = true,
                    Some(Report::Env(_)) => panic!("the probe never finished"),
                    Some(_) => {}
                    None => panic!("the stream ended"),
                }
            }
        })
        .await
        .expect("the reports came while the probe ran");
        // The desktop leaving ends the stream without waiting for the probe.
        drop(client_tx);
        to_node.close(0u32.into(), b"done");
        timeout(Duration::from_secs(30), serving)
            .await
            .expect("status ended with the peer")
            .expect("the task joined")
            .ok();
    }

    /// A peer's connection in its handshake must not keep a leave from
    /// closing the mesh.
    #[tokio::test]
    async fn leaving_does_not_wait_for_a_connection_still_in_its_handshake() {
        use beep_mesh::{JOIN_ALPN, PEER_ALPN};
        use iroh::{SecretKey, protocol::Router};
        let bind = || async {
            Endpoint::builder(iroh::endpoint::presets::Minimal)
                .secret_key(SecretKey::generate())
                .bind()
                .await
                .expect("the endpoint binds")
        };
        let (desktop, node) = (bind().await, bind().await);
        let desktop_dir = tempfile::tempdir().expect("temp dir");
        let mesh = Mesh::create(desktop_dir.path(), desktop.clone(), "desktop")
            .await
            .expect("the mesh is created");
        let (peer, join) = mesh.protocols();
        let _desktop_router = Router::builder(desktop.clone())
            .accept(PEER_ALPN, peer)
            .accept(JOIN_ALPN, join)
            .spawn();
        let ticket = mesh
            .invite(DeviceKind::Node, Duration::from_secs(60))
            .await
            .expect("a ticket");
        let node_dir = tempfile::tempdir().expect("temp dir");
        let peers = Peers::new(
            node.clone(),
            node_dir.path().to_owned(),
            "box".to_owned(),
            Offers::default(),
        );
        let _node_router = Router::builder(node.clone())
            .accept(PEER_ALPN, peers.peer_handler())
            .accept(JOIN_ALPN, peers.join_handler())
            .spawn();
        peers
            .join(&ticket.encode_string())
            .await
            .expect("the node joins");
        // The desktop opens the sync stream and sends half a frame: the node
        // answers with its hello and then waits for the rest.
        let conn = desktop
            .connect(node.addr(), PEER_ALPN)
            .await
            .expect("connected");
        let (mut send, mut recv) = conn.open_bi().await.expect("a stream opens");
        send.write_all(&[0]).await.expect("a byte is sent");
        let mut hello = [0u8; 1];
        timeout(Duration::from_secs(30), recv.read_exact(&mut hello))
            .await
            .expect("the node answered")
            .expect("the hello reads");
        timeout(CLOSE_WAIT / 2, peers.forget())
            .await
            .expect("the leave did not wait for the connection")
            .expect("the node leaves");
        drop((send, recv, conn));
    }

    #[tokio::test]
    async fn transfers_served_to_the_mesh_reach_the_offers_it_was_made_from() {
        use crate::files::{Folders, syncthing::Syncthing};
        use iroh::{SecretKey, endpoint::presets};
        let dir = tempfile::tempdir().expect("temp dir");
        let endpoint = Endpoint::builder(presets::Minimal)
            .secret_key(SecretKey::generate())
            .bind()
            .await
            .expect("the endpoint binds");
        let offers = Offers::default();
        let peers = Peers::new(
            endpoint.clone(),
            dir.path().to_owned(),
            "box".to_owned(),
            offers.clone(),
        );
        let transfers = Transfers::start_with(
            &endpoint,
            &dir.path().join("blobs"),
            Folders::new(&[]),
            Syncthing::at(vec![dir.path().join("syncthing")], None),
        )
        .await
        .expect("the transfers start");
        peers.serve_transfers(transfers.clone());
        // The host's own copy, which its agents' tools read, sees them too.
        assert!(offers.transfers.get().is_some());
        transfers.shutdown().await.expect("the transfers stop");
    }

    /// A device of the mesh opening an agent stream on a machine that is not
    /// lent is refused, over a real connection.
    #[tokio::test]
    async fn a_machine_not_lent_refuses_agent_streams_from_other_desktops() {
        use beep_proto::node::Open;
        use iroh::{SecretKey, endpoint::presets};
        const ALPN: &[u8] = b"withheld-test";
        let bind = || async {
            Endpoint::builder(presets::Minimal)
                .secret_key(SecretKey::generate())
                .alpns(vec![ALPN.to_vec()])
                .bind()
                .await
                .expect("the endpoint binds")
        };
        let (node, desktop) = (bind().await, bind().await);
        let dir = tempfile::tempdir().expect("temp dir");
        let offers = Offers::default();
        offers.lend_to_peers(false);
        let peers = Peers::new(
            node.clone(),
            dir.path().to_owned(),
            "box".to_owned(),
            offers,
        );
        let addr = node.addr();
        let dialing = tokio::spawn({
            let desktop = desktop.clone();
            async move { desktop.connect(addr, ALPN).await }
        });
        let from_desktop = node
            .accept()
            .await
            .expect("a connection comes")
            .await
            .expect("it is accepted");
        let to_node = dialing.await.expect("joined").expect("connected");
        let who = Who::Everything(desktop.id());
        let serving = tokio::spawn(async move {
            for _ in 0..2 {
                let (send, recv) = from_desktop.accept_bi().await.expect("a stream arrives");
                let (tx, rx) = link(send, recv);
                peers.inner.serve(tx, rx, who).await.expect("served");
            }
            // Dropping the connection now would cut off the last answer.
            from_desktop
        });
        for protocol in ["agents/1", "claude/1"] {
            let (send, recv) = to_node.open_bi().await.expect("a stream opens");
            let (mut tx, mut rx) = link(send, recv);
            tx.send(&Open {
                protocol: protocol.to_owned(),
            })
            .await
            .expect("the protocol is sent");
            let answer = timeout(Duration::from_secs(30), rx.recv::<Opened>())
                .await
                .expect("an answer came")
                .expect("it reads");
            assert!(
                matches!(answer, Some(Opened::Refused { .. })),
                "{protocol}: {answer:?}"
            );
        }
        drop(serving.await.expect("served both"));
    }

    #[tokio::test]
    async fn join_waits_for_a_node_that_starts_late() {
        let dir = tempfile::tempdir().expect("temp dir");
        let mut retries = 0;
        // The socket opens after the first attempt failed, as it does for a
        // node that was just started.
        wait_for_node(dir.path(), Duration::from_secs(10), || {
            retries += 1;
            if retries == 1 {
                let listener = Listener::bind(&dir.path().join(SOCKET)).expect("bind");
                serve_probe_then_join(listener);
            }
        })
        .await
        .expect("node came up");
        assert!(retries >= 1, "the first attempt found no node");
        join(dir.path(), "ticket").await.expect("joined");
    }

    #[tokio::test]
    async fn join_says_what_to_do_when_no_node_comes_up() {
        let dir = tempfile::tempdir().expect("temp dir");
        let err = join_when_up(dir.path(), "ticket", Duration::from_millis(300))
            .await
            .expect_err("no node");
        assert!(format!("{err}").contains("beep-node run"), "{err}");
    }
}
