//! The node in its mesh: `beep-peer/2` and `beep-join/2` on the node's
//! endpoint, and what a peer gets from the node. A desktop gets what the
//! server gets, every session. Another node gets `status/1` and `tools/1`
//! only. A node never mints a ticket or adds a device; it only joins one.
//!
//! The protocols are served from the start and answer once the node is in a
//! mesh, so `join` takes effect without a restart.

use crate::{
    agents::SessionsAlpn,
    files::transfer::Transfers,
    link::Protocol,
    run::local_socket::{Listener, Stream},
    status::{MESH_SOCKET, STATUS_LINE},

    link::protocol::SessionProtocol,
    link::watchdog::{self, Step},
    machine::{env, stats},
    team::link::TeamLink,
    tools::{Node, team_link::TeamAlpn},
};
use anyhow::{Context, Result, bail};
use beep_mesh::{
    DeviceKind, DeviceStatus, Invitation, JoinProtocol, Mesh, MeshProtocol,
    PeerStream, SessionFn,
};
use beep_proto::node::{EnvReport, LinkRx, LinkTx, link, status::Report};
use futures::future::BoxFuture;
use beep_proto::watched;
#[cfg(feature = "cli")]
use beep_report::Share;
use beep_report::collect;
use iroh::{
    endpoint::Connection,
    protocol::{AcceptError, ProtocolHandler},
};
use serde::de::IgnoredAny;
use std::{
    collections::BTreeSet,
    fmt::Write,
    path::Path,
    sync::{Arc, Mutex, MutexGuard, OnceLock, Weak},
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader, split},
    sync::{mpsc, watch},
    task::JoinSet,
    time::timeout,
};
use tokio_util::task::AbortOnDropHandle;

mod offers;
mod own;

pub use offers::{Bound, Offers};
pub(crate) use crate::status::Own;
pub(crate) use offers::{Who, read_open, withheld};

/// `join <ticket>`; the node answers `joined`, or `failed <why>`.
const JOIN: &str = "join ";
/// The node answers one line per device, then `end`.
const DEVICES: &str = "devices";
/// `leave`; the node answers `left`, or `failed <why>`.
const LEAVE: &str = "leave";
/// `report <since>`, Unix seconds; the node answers every device's
/// `Share` of a problem report, a `Vec<Share>` in compact msgpack, then
/// closes. A node from before it closes without answering.
const REPORT: &str = "report ";

/// How long a channel may wait for its opening frame.
pub(super) const CHANNEL_OPEN_WAIT: Duration = Duration::from_secs(10);

/// What the node holds once it is in a mesh: the mesh itself is read from
/// `Offers::mesh`. The mesh stops with its last clone.
#[derive(Clone)]
pub(super) struct Protocols {
    peer: MeshProtocol,
    join: JoinProtocol,
    /// Serves the streams peers open, until the node lets go of the mesh.
    /// Unlike a handler set on the mesh, it ends with `leave`, and streams
    /// open then end with it; a join that keeps the old mesh installs it
    /// again, so those are cut. A lone mesh is the only one that joins.
    _streams: Arc<AbortOnDropHandle<()>>,
}

pub(super) struct Inner {
    /// The node's key, name and folder.
    pub(super) own: Own,
    /// `beep-team/1`, served whether or not the node is in a mesh.
    pub(super) team: TeamLink,
    pub(super) offers: Mutex<Offers>,
    /// Protocols a host will offer once its services are up.
    pub(super) later: watch::Sender<BTreeSet<String>>,
    pub(super) active: Mutex<Option<Protocols>>,
    /// Applies the mesh's state to the node while it is a member: started
    /// by `from_mesh::run` when the mesh or the transfers are set, and ended
    /// by `leave`.
    pub(super) follower: Mutex<Option<AbortOnDropHandle<()>>>,
    /// Watches the mesh and the transfers to start the follower.
    pub(super) _following: OnceLock<AbortOnDropHandle<()>>,
    /// Serves a stream once its protocol is known: set by the node's
    /// router, which this module does not import.
    pub(super) serve_protocol: ServeProtocol,
}

/// How a stream that opened with a protocol is served, and its trace ended.
pub(super) type ServeProtocol = fn(
    Arc<Inner>,
    LinkTx,
    LinkRx,
    SessionProtocol,
    Who,
    beep_proto::logging::RequestTrace,
) -> BoxFuture<'static, Result<()>>;

impl Inner {
    /// Serves a stream a peer, or this process, opened.
    async fn serve(self: Arc<Self>, tx: LinkTx, mut rx: LinkRx, who: Who) -> Result<()> {
        let protocol = read_open(&mut rx).await?;
        let mut trace = beep_proto::logging::RequestTrace::new(protocol.to_string());
        trace.phase("waiting for protocol readiness");
        (self.serve_protocol)(self.clone(), tx, rx, protocol, who, trace).await
    }

    pub(super) fn offers(&self) -> MutexGuard<'_, Offers> {
        crate::lock(&self.offers)
    }

    fn follower(&self) -> MutexGuard<'_, Option<AbortOnDropHandle<()>>> {
        crate::lock(&self.follower)
    }

    pub(super) fn active(&self) -> Option<Mesh> {
        self.offers().mesh.get()
    }

    /// What the tools and diagnostics read of this node.
    pub(super) fn node(&self) -> Node {
        self.offers().node(&self.own, &self.team)
    }
}

/// The node's membership, shared by the router, the control socket and the
/// shutdown. A host that is a node itself, such as the Mac app, makes one
/// with its own mesh through `install`.
#[derive(Clone)]
pub struct Peers {
    pub(super) inner: Arc<Inner>,
}

impl Peers {
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
        let offers = self.inner.offers().clone();
        // Cleared first: the follower starts nothing on a mesh that is gone.
        offers.mesh.clear();
        let follower = self.inner.follower().take();

        if let Some(follower) = follower {
            follower.abort();
            // Returns once the task, and the mesh it held, is gone.
            follower.await.ok();
        }

        if let Some(transfers) = offers.transfers.get() {
            transfers.syncs().forget_recoveries();
        }

        // The gateway stopped with the follower: no team is served from here.
        self.inner.team.reset();
        crate::lock(&self.inner.active).take();
    }

    /// One of the active protocols, without a handle on the mesh itself.
    fn protocols<P>(&self, take: impl FnOnce(&Protocols) -> P) -> Option<P> {
        crate::lock(&self.inner.active).as_ref().map(take)
    }

    /// The `beep-peer/2` handler.
    pub fn peer_handler(&self) -> PeerAlpn {
        PeerAlpn(self.clone())
    }

    pub(super) async fn serve_command_session(
        &self,
        tx: LinkTx,
        rx: LinkRx,
        peer: iroh::EndpointId,
        mesh: iroh::EndpointId,
        protocol: &str,
    ) -> Result<()> {
        let protocol = protocol.parse::<SessionProtocol>()?;
        anyhow::ensure!(
            protocol != SessionProtocol::Events,
            "subscriptions use their own ALPN"
        );
        let trace = beep_proto::logging::RequestTrace::new(protocol.to_string());
        let who = self.channel_who(peer, mesh).context("not authorized")?;
        (self.inner.serve_protocol)(self.inner.clone(), tx, rx, protocol, who, trace).await
    }

    pub(super) fn channel_offers(&self) -> Offers {
        self.inner.offers().clone()
    }

    pub(super) fn channel_access(
        &self,
        connection: &iroh::endpoint::Connection,
    ) -> Option<(iroh::EndpointId, watch::Receiver<u64>)> {
        let peer = connection.remote_id();
        let active = self.active()?;
        active.track_protocol_connection(connection).ok()?;
        self.channel_allowed(peer, active.id())
            .then(|| (active.id(), active.updates()))
    }

    pub(super) fn channel_allowed(&self, peer: iroh::EndpointId, mesh: iroh::EndpointId) -> bool {
        self.channel_who(peer, mesh).is_some()
    }

    pub(super) fn controller_allowed(
        &self,
        peer: iroh::EndpointId,
        mesh: iroh::EndpointId,
    ) -> bool {
        matches!(self.channel_who(peer, mesh), Some(Who::Everything(_)))
            && self.inner.offers().lends_to_peers()
    }

    fn channel_who(&self, peer: iroh::EndpointId, mesh: iroh::EndpointId) -> Option<Who> {
        let active = self.active()?;
        if active.id() != mesh || !active.is_member(active.me()) {
            return None;
        }

        let devices = active.devices();
        let kind = devices
            .iter()
            .find(|status| status.device.id == peer)?
            .device
            .kind;
        Some(match kind {
            DeviceKind::Desktop => Who::Everything(peer),
            DeviceKind::Node => Who::Node {
                peer,
                desktop_here: devices.iter().any(|status| {
                    status.device.id == active.me()
                        && status.device.kind == DeviceKind::Desktop
                }),
            },
        })
    }

    /// The `beep-team/1` handler.
    pub fn team_handler(&self) -> TeamAlpn {
        TeamAlpn::new(self.inner.node())
    }

    /// The `beep-join/2` handler.
    pub fn join_handler(&self) -> JoinAlpn {
        JoinAlpn(self.clone())
    }

    /// The `beep-sessions/1` handler: the other machine of a move reads
    /// this node's sessions through it.
    pub fn sessions_handler(&self) -> SessionsAlpn {
        let offers = self.inner.offers();
        SessionsAlpn::new(offers.agents.clone(), offers.mesh.clone())
    }

    /// Takes the mesh the node's folder holds, if it holds one.
    pub(crate) async fn open(&self) -> Result<bool> {
        let dir = self.inner.own.dir.clone();
        let opened = Mesh::open(&dir, self.inner.own.endpoint.clone())
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
    /// mesh to keep and takes the new one in its place, also after a refused
    /// try already closed the old one.
    pub(crate) async fn join(&self, ticket: &str) -> Result<()> {
        let invite: Invitation = ticket.parse()?;
        let old = self.active();

        if old.as_ref().is_some_and(|mesh| mesh.is_member(mesh.me())) {
            bail!("this node already belongs to a mesh");
        }

        if let Some(old) = old {
            self.leave().await;
            close_within(old).await?;
        }

        let mesh = Mesh::join_replacing_lone(
            &self.inner.own.dir,
            self.inner.own.endpoint.clone(),
            invite,
            &self.inner.own.name,
        )
        .await
        .context("could not join")?;
        self.install(mesh);
        Ok(())
    }

    /// Leaves the mesh for good and deletes what it kept: the node is in none
    /// after this, and can join another.
    pub(crate) async fn forget(&self) -> Result<()> {
        let old = self.active();
        self.leave().await;

        if let Some(old) = old {
            close_within(old).await?;
        }

        Mesh::forget(&self.inner.own.dir)
            .await
            .context("could not delete the mesh")
    }

    /// Whether this device is a desktop in the current device list.
    pub fn is_desktop(&self) -> bool {
        self.inner.own.is_desktop(&self.inner.node())
    }

    /// Whether this device is in a mesh it was not removed from.
    pub fn is_member(&self) -> bool {
        self.active()
            .is_some_and(|mesh| mesh.is_member(mesh.me()))
    }

    /// The mesh's devices, empty before the node joined.
    pub fn devices(&self) -> Vec<DeviceStatus> {
        self.inner.own.devices(&self.inner.node())
    }

    fn active(&self) -> Option<Mesh> {
        self.inner.active()
    }

    /// The node's membership, for a service started beside the router.
    pub fn membership(&self) -> crate::membership::Membership {
        self.inner.offers().mesh.clone()
    }

    /// Takes `mesh` as the node's membership; its peers' streams are served
    /// from here on.
    pub fn install(&self, mesh: Mesh) {
        let (peer, join) = mesh.protocols();
        let streams = Arc::new(self.serve_streams(mesh.incoming()));
        // The mesh first: `active` needs it with the protocols.
        self.inner.offers().mesh.set(mesh);
        *crate::lock(&self.inner.active) = Some(Protocols {
            peer,
            join,
            _streams: streams,
        });
    }

    /// The node's transfer and sync services started: the mesh drives them.
    pub fn serve_transfers(&self, transfers: Transfers) {
        self.inner.offers().transfers.set(transfers);
    }

    /// The node's session for a caller in this process, which is a desktop
    /// by being the user at this machine: no network, every protocol.
    pub fn local_session(&self) -> SessionFn {
        let peers = self.clone();
        Arc::new(move |tx: LinkTx, rx: LinkRx| {
            let peers = peers.clone();
            Box::pin(async move {
                let who = Who::Everything(peers.inner.own.endpoint.id());

                if let Err(e) = peers.inner.clone().serve(tx, rx, who).await {
                    log::debug!("a local session ended: {e:#}");
                }
            })
        })
    }

    /// Serves each stream a peer opens, by what the peer is now, until the
    /// returned handle drops.
    fn serve_streams(&self, mut incoming: mpsc::Receiver<PeerStream>) -> AbortOnDropHandle<()> {
        // Weak: the node holds the task, and the task would hold the node.
        let peers = Arc::downgrade(&self.inner);

        AbortOnDropHandle::new(tokio::spawn(async move {
            let mut serving = JoinSet::new();

            loop {
                tokio::select! {
                    Some(stream) = incoming.recv() => {
                        serving.spawn(serve_peer_stream(peers.clone(), stream));
                    }
                    Some(_) = serving.join_next() => {}
                    else => return,
                }
            }
        }))
    }
}

/// Serves one stream a peer opened.
async fn serve_peer_stream(peers: Weak<Inner>, stream: PeerStream) {
    let Some(inner) = peers.upgrade() else {
        return;
    };
    let desktop_here = Peers {
        inner: inner.clone(),
    }
    .is_desktop();
    // Held to the end of the function: the link counts the stream open
    // while it is served.
    let PeerStream {
        peer,
        kind,
        send,
        recv,
        lease: _lease,
    } = stream;
    let who = match kind {
        DeviceKind::Desktop => Who::Everything(peer),
        DeviceKind::Node => Who::Node { peer, desktop_here },
    };
    let (tx, rx) = link(send, recv);

    if let Err(e) = inner.serve(tx, rx, who).await {
        log::debug!("a stream from {peer} ended: {e:#}");
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
    status_with(tx, rx, offers, env::machine()).await
}

/// `status` with the machine's probes given. Every stream follows the one
/// shared probe: it sends the latest report at once and each new one, so a
/// stream never starts a probe of its own.
async fn status_with(
    mut tx: LinkTx,
    mut rx: LinkRx,
    offers: Offers,
    probes: &Arc<env::Probes>,
) -> Result<()> {
    // An agent whose adapter the node installed later is offered then.
    let mut kinds = offers.agents.kinds_changed();
    kinds.mark_unchanged();
    let mut adapters = offers.agents.adapters_changed();
    adapters.mark_unchanged();
    tx.send(&Report::Offers(offers.names())).await?;
    let mut probed = probes.follow();
    probed.mark_changed();
    // The sample is taken once for every stream; each sends a part when it
    // differs from what it sent.
    let traffic = offers.traffic.clone();
    let mut sampled = offers.sampler.follow(
        Box::new(|| Box::pin(stats::sample(dirs::home_dir().into_iter().collect()))),
        Box::new(move || traffic.report()),
    );
    let mut reported = None;
    let mut stats_sent = None;
    let mut traffic_sent = None;

    loop {
        tokio::select! {
            // The peer finished its side: it has no more to say, or left.
            gone = rx.recv::<IgnoredAny>() => {
                gone?;
                return Ok(());
            }
            Ok(()) = kinds.changed() => {
                tx.send(&Report::Offers(offers.names())).await?;
            }
            Ok(()) = probed.changed() => {
                let now = watched::read_and_update(&mut probed, |probed| {
                    probed.as_ref().map(|probed| probed.report.clone())
                });

                if let Some(now) = now {
                    let now = EnvReport { adapters: offers.agents.adapter_installs(), ..now };
                    send_env(&mut tx, &mut reported, now).await?;
                }
            }
            // An adapter install started or ended: said with the last probe.
            Ok(()) = adapters.changed() => {
                if let Some(before) = reported.clone() {
                    let now = EnvReport { adapters: offers.agents.adapter_installs(), ..before };
                    send_env(&mut tx, &mut reported, now).await?;
                }
            }
            Ok(()) = sampled.changed() => {
                let sample = watched::read_and_update(&mut sampled, Clone::clone);

                if let Some(stats) = sample.stats
                    && stats_sent.as_ref() != Some(&stats)
                {
                    tx.send(&Report::Stats(stats.clone())).await?;
                    stats_sent = Some(stats);
                }

                if let Some(traffic) = sample.traffic
                    && traffic_sent.as_ref() != Some(&traffic)
                {
                    tx.send(&Report::Traffic(traffic.clone())).await?;
                    traffic_sent = Some(traffic);
                }
            }
        }
    }
}

/// Sends `now` when it differs from what was `reported`.
async fn send_env(tx: &mut LinkTx, reported: &mut Option<EnvReport>, now: EnvReport) -> Result<()> {
    if reported.as_ref() != Some(&now) {
        tx.send(&Report::Env(now.clone())).await?;
        *reported = Some(now);
    }

    Ok(())
}

/// How long a close waits for the tasks that still hold the mesh to let go.
const CLOSE_WAIT: Duration = Duration::from_secs(5);

/// Closes `mesh` once the tasks that were just stopped let go of their
/// handles, waiting at most `CLOSE_WAIT` for them.
async fn close_within(mesh: Mesh) -> Result<()> {
    let _ = timeout(CLOSE_WAIT, mesh.released()).await;

    mesh.close().await.map_err(|in_use| anyhow::anyhow!("{in_use}"))
}

/// Takes the mesh commands on the node's socket until the node stops.
pub(crate) async fn listen(dir: &Path, peers: Peers) -> Result<()> {
    let mut listener = Listener::bind(&dir.join(MESH_SOCKET))?;
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
    } else if request == STATUS_LINE {
        let node = peers.inner.node();
        let status = peers.inner.own.local_status(&node).await?;
        write.write_all(&rmp_serde::to_vec(&status)?).await?;
        write.shutdown().await?;
        return Ok(());
    } else if let Some(since) = request.strip_prefix(REPORT) {
        let since: i64 = since.trim().parse().context("reading the report's start")?;
        let node = peers.inner.node();
        let shares = collect(peers.inner.own.report_targets(&node), since).await;
        write.write_all(&rmp_serde::to_vec(&shares)?).await?;
        write.shutdown().await?;
        return Ok(());
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
    expect_reply(ask(dir, &format!("{JOIN}{}", ticket.trim())).await?, "joined")
}

/// `Ok` when the node's first line is `done`, else its failure.
#[cfg(feature = "cli")]
fn expect_reply(lines: Vec<String>, done: &str) -> Result<()> {
    match lines.first().map(String::as_str) {
        Some(line) if line == done => Ok(()),
        Some(failed) => bail!("{}", failed.strip_prefix("failed ").unwrap_or(failed)),
        None => bail!("the node closed without answering"),
    }
}

/// Has the node running from `dir` leave its mesh and delete what it kept.
#[cfg(feature = "cli")]
pub async fn leave(dir: &Path) -> Result<()> {
    expect_reply(ask(dir, LEAVE).await?, "left")
}

/// Whether a node is running from `dir` and takes mesh commands.
#[cfg(feature = "cli")]
pub fn is_running(dir: &Path) -> bool {
    crate::run::local_socket::is_listening(&dir.join(MESH_SOCKET))
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
    use crate::run::local_socket;
    /// The pause between attempts.
    const RETRY: Duration = Duration::from_millis(100);
    let socket = dir.join(MESH_SOCKET);
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

/// Why the running node gave no report; the text is the share's status.
#[cfg(feature = "cli")]
#[derive(Debug, thiserror::Error)]
pub enum ReportError {
    #[error("the running node did not answer in {0} seconds")]
    Silent(u64),
    #[error("the running node is older and cannot collect")]
    Older,
}

/// How long the running node has to collect a report.
#[cfg(feature = "cli")]
const REPORT_LIMIT: Duration = Duration::from_secs(30);

/// Every device's share of a problem report from Unix second `since`, as
/// the node running from `dir` collects it; `None` when no node runs there.
#[cfg(feature = "cli")]
pub async fn collect_report(dir: &Path, since: i64) -> Result<Option<Vec<Share>>> {
    collect_report_within(dir, since, REPORT_LIMIT).await
}

/// `collect_report` with the node given `limit` to answer.
#[cfg(feature = "cli")]
pub async fn collect_report_within(
    dir: &Path,
    since: i64,
    limit: Duration,
) -> Result<Option<Vec<Share>>> {
    use crate::run::local_socket;
    /// The most bytes read: every device's share at its cap, and more.
    const MAX_SHARES: u64 = 64 * 1024 * 1024;
    let Ok(stream) = local_socket::connect(&dir.join(MESH_SOCKET)).await else {
        return Ok(None);
    };
    let (read, mut write) = split(stream);
    let mut bytes = Vec::new();
    timeout(limit, async {
        write
            .write_all(format!("{REPORT}{since}\n").as_bytes())
            .await?;
        read.take(MAX_SHARES).read_to_end(&mut bytes).await
    })
    .await
    .map_err(|_| ReportError::Silent(limit.as_secs()))??;

    if bytes.is_empty() {
        return Err(ReportError::Older.into());
    }

    let shares = beep_proto::lenient::from_slice(&bytes).context("reading the node's report")?;
    Ok(Some(shares))
}

#[cfg(feature = "cli")]
async fn ask(dir: &Path, request: &str) -> Result<Vec<String>> {
    use crate::run::local_socket;
    /// How long a command waits for the running node.
    const ASK_LIMIT: Duration = Duration::from_secs(60);
    let stream = local_socket::connect(&dir.join(MESH_SOCKET))
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
#[path = "../tests/link_peers.rs"]
mod tests;
