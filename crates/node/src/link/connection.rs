//! The connection to the server: the link code that joins the account, then
//! the control stream, kept open and reopened when it drops.

use crate::files::paths;
use crate::{
    agents::mcp::{self, Upstream},
    agents::{AgentKind, Agents, SESSIONS_ALPN, SessionsAlpn, skills::Skills},
    files::Folders,
    files::search::Searches,
    files::transfer::Transfers,
    link::reconnect::{self, Backoff},
    link::restart::{self, Reopen},
    link::state::State,
    link::traffic::Traffic,
    link::watchdog,
    link::{
        from_mesh::{Family, Membership},
        peer_tools::{self, Caller},
        peers::{self, Peers},
    },
    lock,
    machine::env,
    machine::provision::Provision,
    machine::service::logs::Logs,
    machine::stats,
    run::shell,
    run::tasks,
    run::terminal,
};
use anyhow::{Context, Result, bail};
use beep_mesh::{JOIN_ALPN, PEER_ALPN};
use beep_proto::{
    link::LinkOffer,
    node::{
        CLOSE_SETUP_REFUSED, Down, EnvReport, LinkRx, LinkTx, MachineCredentials, NODE_ALPN,
        NODE_ALPN_NAMED, NodeHello, NodeSetup, Open, Opened, StatsReport, Up, agent, desktop,
        harness::AgentSettings, leave, link_on, skills, status, tool_update, tools, version,
    },
    port::CLOSE_REVOKED,
};
use iroh::{
    Endpoint, EndpointAddr, EndpointId,
    endpoint::{ConnectOptions, Connection, ConnectionError, VarInt, presets},
    protocol::Router,
};
use std::{
    future::Future,
    path::{Path, PathBuf},
    pin::Pin,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::{
    task::JoinSet,
    time::{MissedTickBehavior, interval, sleep, timeout},
};
use tokio_util::task::AbortOnDropHandle;

/// Set, the node binds with no relay and no discovery service. For tests
/// that run nodes side by side; a real node must not set it.
const NO_RELAY: &str = "BEEP_NODE_NO_RELAY";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
/// How often the machine is probed again for changes.
const REPROBE: Duration = Duration::from_secs(10 * 60);
/// How often free disk and memory are sampled.
const RESAMPLE: Duration = Duration::from_secs(60);
/// How often the traffic counters are reported.
const REPORT_TRAFFIC: Duration = Duration::from_secs(60);
/// How long the server has to name a session's protocol.
const OPEN_TIMEOUT: Duration = Duration::from_secs(10);
/// The most sessions open at once; more are refused.
const MAX_SESSIONS: usize = 32;
/// How long a stopping node waits for its agent sessions to close. The stop
/// steps add up to under the 10 s the command line gives a stop before it
/// exits anyway.
const STOP_SESSIONS: Duration = Duration::from_secs(3);
/// How long a stopping node gives each other step.
const STOP_STEP: Duration = Duration::from_secs(1);
/// How long a temporary device has to be removed from the mesh as it stops.
const STOP_LEAVE: Duration = Duration::from_secs(3);

/// A session protocol a host serves beside the node's own, such as the Mac
/// app's `desktop/1`.
pub trait Protocol: Send + Sync {
    /// The name a session opens with, which the hello offers.
    fn name(&self) -> &str;
    /// Serves one session stream until it ends.
    fn serve(&self, tx: LinkTx, rx: LinkRx) -> Session;
}

/// One session served by a `Protocol`.
pub type Session = Pin<Box<dyn Future<Output = Result<()>> + Send>>;

pub fn print_offer(state: &State) -> Result<()> {
    let server = state
        .server
        .context("this node has no server; link one with `beep-node setup <token>`")?;
    let offer = LinkOffer::sign(&state.key, server, &state.name, SystemTime::now())?;
    println!("Link this machine from Beep on your phone: copy the link below to the phone, then");
    println!("open Settings, Linked devices, Type a code instead, and paste it. Check that the");
    println!("phone shows the same code.");
    println!();
    println!("  Code: {}", offer.code());
    println!("  Link: {}", offer.to_uri());
    println!();
    println!("The code works for 10 minutes. Then start it with `beep-node run`.");
    Ok(())
}

/// Stays connected, waiting longer after each failed try, until `stop`
/// finishes; then closes the agent sessions and returns. With `workspace`,
/// this node is the user's environment: Beep keeps their files and runs its
/// code there, and it is shared through `files/1` before the folders in
/// `node.toml`.
pub async fn serve(
    state: State,
    workspace: Option<&Path>,
    stop: impl Future<Output = ()>,
) -> Result<()> {
    let traffic = Traffic::default();
    let relays_off = std::env::var_os(NO_RELAY).is_some();
    if relays_off {
        log::warn!("{NO_RELAY} is set: no relay, no discovery (for tests only)");
    }
    let builder = || {
        // Tests run nodes side by side on one machine, without relays.
        let preset = if relays_off {
            Endpoint::builder(presets::Minimal)
        } else {
            Endpoint::builder(presets::N0)
        };
        preset
            .secret_key(state.key.clone())
            .transport_config(beep_mesh::transport_config())
            .hooks(traffic.clone())
    };
    let endpoint = beep_proto::port::bind_on(builder, state.port).await?;
    traffic.measure_endpoint(endpoint.metrics().clone());
    log::info!("node key {}", endpoint.id());
    // Made once, so agent sessions outlive each connection.
    // Installs gh, teacli and the agent skills beside serving; the work
    // stops with the node.
    let provision = Provision::new(&state.dir);
    let _provision = provision.start();
    let mut offers = Offers::local(&state.agents, provision, state.server.is_some());
    offers.traffic = traffic;
    if let Some(home) = dirs::home_dir() {
        offers.skills = Skills::under(home);
    }
    offers.extra.push(Arc::new(Logs::service()));
    // Only the CLI updates itself; the Mac app's node updates with the app.
    #[cfg(feature = "cli")]
    offers
        .extra
        .push(Arc::new(crate::machine::update::Updates::new(
            offers.agents.clone(),
        )));
    // Files only when the owner shared folders in node.toml, or this node
    // is the user's environment.
    let shared: Vec<_> = workspace
        .map(Path::to_path_buf)
        .into_iter()
        .chain(state.files.iter().cloned())
        .collect();
    let folders = Folders::new(&shared);
    offers.agents.share_folders(shared.clone());
    // The agents' logs read once ahead, so the first file request for a
    // session started outside Beep does not wait for them.
    let searches = Searches::start(state.dir.clone(), offers.agents.clone(), &state.search);
    offers.extra.push(Arc::new(searches));
    // The plans read ahead too: Claude Code takes a while to start, and a
    // desktop opening its window would otherwise wait for it.
    let agents = offers.agents.clone();
    let _plans = AbortOnDropHandle::new(tokio::spawn(async move {
        agents.read_plans_at_start().await;
    }));
    // The workspace as the node resolved it, only once it is shared.
    let workspace = workspace
        .and_then(|dir| paths::canonicalize(dir).ok())
        .filter(|dir| folders.first() == Some(dir.as_path()))
        .map(|dir| dir.to_string_lossy().into_owned());
    let mut started = None;
    let mut served = None;
    // Transfers and sync run on every node: sync for the projects the mesh
    // names, wherever they are, and transfers for `send_files`. Other machines
    // reach transfers only with shared folders. Without its store the node
    // still serves the rest.
    match Transfers::start_shared(&endpoint, &state.dir.join("blobs"), folders.clone()).await {
        Ok((transfers, handlers)) => {
            served = Some(handlers);
            offers.extra.push(Arc::new(transfers.syncs()));
            offers.extra.push(Arc::new(transfers.syncs().controls()));
            offers.extra.push(Arc::new(transfers.syncs().rescans()));
            if !folders.is_empty() {
                offers.extra.push(Arc::new(transfers.clone()));
            }
            offers.transfers.set(transfers.clone());
            started = Some(transfers);
        }
        Err(e) => log::warn!("not offering transfers: {e:#}"),
    }
    if !folders.is_empty() {
        offers.extra.push(Arc::new(folders));
    }
    // The mesh: peers reach the node over `beep-peer/2` with or without a
    // server. The router runs from here, so a join takes effect at once.
    let peers = Peers::new(
        endpoint.clone(),
        state.dir.clone(),
        state.name.clone(),
        offers.clone(),
    );
    if let Err(e) = peers.open().await {
        log::warn!("not serving the mesh: {e:#}");
    }
    if state.server.is_none() && !peers.is_member() {
        log::info!("no server and no mesh: waiting for `beep-node mesh join <ticket>`");
    }
    let mut router = Router::builder(endpoint.clone())
        .accept(PEER_ALPN, peers.peer_handler())
        .accept(JOIN_ALPN, peers.join_handler())
        .accept(
            SESSIONS_ALPN,
            SessionsAlpn::new(offers.agents.clone(), offers.mesh.clone()),
        );
    if let Some(served) = served {
        router = served.add_to(router);
    }
    let router = router.spawn();
    let hello = Hello {
        name: &state.name,
        workspace: workspace.as_deref(),
        setup: None,
    };
    log::info!("offering {:?}", offers.names());
    // Claude Code's hooks report to the node over a socket in its folder.
    let agents = offers.agents.clone();
    let hooks_dir = state.dir.clone();
    let _hooks = AbortOnDropHandle::new(tokio::spawn(async move {
        if let Err(e) = agents.external().listen_hooks(&hooks_dir).await {
            log::warn!("not taking Claude Code hooks: {e:#}");
        }
    }));
    // The agents' Beep MCP server runs this binary against a socket in the
    // node's folder.
    let _mcp = mcp::start(&state.dir, &offers);
    // Logs when the async workers, or a step that hung before, stop moving.
    let _watchdog = watchdog::start();
    // `beep-node update` asks the node to hold still over another one.
    let agents = offers.agents.clone();
    let control_dir = state.dir.clone();
    let _control = AbortOnDropHandle::new(tokio::spawn(async move {
        if let Err(e) = restart::listen(&control_dir, agents).await {
            log::warn!("updates cannot wait for idle sessions: {e:#}");
        }
    }));
    // `beep-node mesh join` and `mesh devices` reach the node on a socket of
    // their own.
    let (mesh_dir, mesh_peers) = (state.dir.clone(), peers.clone());
    let _mesh_control = AbortOnDropHandle::new(tokio::spawn(async move {
        if let Err(e) = peers::listen(&mesh_dir, mesh_peers).await {
            log::warn!("the mesh commands cannot reach the node: {e:#}");
        }
    }));
    // What was open before a restart or crash opens again: terminals now,
    // sessions once the server sent the agent settings they start with.
    // Only then is the note kept to what is open, so a start that dies
    // before leaves it as it was.
    let reopen = Reopen::read(&state.dir);
    terminal::reopen(&state.dir, reopen.terminals).await;
    let agents = offers.agents.clone();
    let note_dir = state.dir.clone();
    let mut keeping = AbortOnDropHandle::new(tokio::spawn(async move {
        agents.reopen(reopen.sessions).await;
        restart::keep(&note_dir, &agents).await;
    }));
    let stay = async {
        match state.server {
            Some(server) => stay_as(&endpoint, server.into(), &hello, &offers).await,
            None => std::future::pending().await,
        }
    };
    tokio::select! {
        () = stay => {}
        () = stop => {}
    }
    // A temporary device, such as a container, leaves the mesh as it stops, while its links are
    // still up. Not when an update restarts it: an update holds prompts until then.
    if !offers.agents.held() {
        stop_step(
            "leaving the mesh if temporary",
            STOP_LEAVE,
            peers.leave_if_temporary(),
        )
        .await;
    }
    // Agents that end from here on ended with the node, whatever their exit.
    offers.agents.begin_stopping();
    // Stopped, the machine keeps no token: teacli's config, its shim and
    // the env vars go.
    if let Some(provision) = &offers.provision {
        stop_step("forgetting credentials", STOP_STEP, provision.forget()).await;
    }
    // Noted while they still run: the service manager ends them once the
    // node exits.
    let terminals = terminal::running();
    stop_step(
        "the agent sessions",
        STOP_SESSIONS,
        offers.agents.shutdown(),
    )
    .await;
    // The last word is the shutdown's: the keeper stops first, and its
    // write in flight lands before this one.
    keeping.abort();
    if let Some(Err(e)) = stop_step("the note of what is open", STOP_STEP, &mut keeping).await
        && !e.is_cancelled()
    {
        log::warn!("the note of what is open was not kept: {e}");
    }
    let reopen = Reopen {
        sessions: offers.agents.open_sessions(),
        terminals,
    };
    if let Err(e) = reopen.save(&state.dir) {
        log::warn!("what was open will not open again: {e:#}");
    }
    if let Some(Err(e)) = stop_step("the router", STOP_STEP, router.shutdown()).await {
        log::warn!("the router did not stop cleanly: {e:#}");
    }
    if let Some(transfers) = started
        && let Some(Err(e)) = stop_step("transfers", STOP_STEP, transfers.shutdown()).await
    {
        log::warn!("transfers did not stop cleanly: {e:#}");
    }
    log::info!("stopped");
    Ok(())
}

/// Runs one step of stopping the node, logged, for at most `limit`; `None`
/// when it took longer and was left.
async fn stop_step<T>(name: &str, limit: Duration, step: impl Future<Output = T>) -> Option<T> {
    log::info!("stopping: {name}");
    let done = timeout(limit, step).await.ok();
    if done.is_none() {
        log::warn!(
            "stopping: {name} took over {} s, going on without it",
            limit.as_secs()
        );
    }
    done
}

/// One connection to `server`, served until it closes: the hello, the
/// environment report, then sessions, in the node's own protocols and
/// `offers`. `Ok` when the server closed it. Shell sessions end with the
/// connection; agent sessions live in `offers.agents`.
pub async fn connect(
    endpoint: &Endpoint,
    server: EndpointAddr,
    name: &str,
    offers: &Offers,
) -> Result<()> {
    connect_as(endpoint, server, &Hello::named(name), offers).await
}

/// Stays connected to `server` as `name`, connecting again after each
/// drop. Never returns.
pub async fn stay(endpoint: &Endpoint, server: EndpointAddr, name: &str, offers: &Offers) {
    stay_as(endpoint, server, &Hello::named(name), offers).await
}

async fn stay_as(endpoint: &Endpoint, server: EndpointAddr, hello: &Hello<'_>, offers: &Offers) {
    let mut backoff = Backoff::default();
    loop {
        let link = async {
            let linked = open_as(endpoint, server.clone(), hello, offers).await?;
            // Welcomed: the link worked, however long it lasts, so a drop
            // is tried again at once and not after the last failure's wait.
            backoff.reset();
            linked.serve().await
        };
        match link.await {
            Ok(()) => log::info!("the server closed the connection"),
            Err(e) => log::warn!("connection lost: {e:#}"),
        }
        sleep(backoff.pause()).await;
    }
}

/// What the hello says about this node beside its offers.
struct Hello<'a> {
    name: &'a str,
    workspace: Option<&'a str>,
    /// A setup token to claim (`beep-node setup`).
    setup: Option<&'a NodeSetup>,
}

impl<'a> Hello<'a> {
    fn named(name: &'a str) -> Self {
        Self {
            name,
            workspace: None,
            setup: None,
        }
    }
}

/// What claiming a setup token gave.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Claimed {
    /// The account's name, such as mega-fox-trot.
    pub account: String,
    /// What the server calls this machine.
    pub name: String,
}

/// Connects to `server` once to claim `setup` with this endpoint's key,
/// then closes. `name` is the machine's name when neither `setup` nor the
/// token gives one. Errors say why in words for the user.
pub async fn claim(
    endpoint: &Endpoint,
    server: EndpointAddr,
    name: &str,
    setup: &NodeSetup,
) -> Result<Claimed> {
    let hello = Hello {
        name,
        workspace: None,
        setup: Some(setup),
    };
    let linked = open_as(endpoint, server, &hello, &Offers::default()).await?;
    linked
        .connection
        .close(VarInt::from_u32(0), b"set up; reconnecting");
    linked
        .claimed
        .context("the server welcomed this node without linking it")
}

async fn connect_as(
    endpoint: &Endpoint,
    server: EndpointAddr,
    hello: &Hello<'_>,
    offers: &Offers,
) -> Result<()> {
    open_as(endpoint, server, hello, offers)
        .await?
        .serve()
        .await
}

/// The node's transfers, set once its services are up and read by whoever
/// holds a copy of the offers.
#[derive(Clone, Default)]
pub(crate) struct Sharing(Arc<Mutex<Option<Transfers>>>);

impl Sharing {
    pub(crate) fn get(&self) -> Option<Transfers> {
        lock(&self.0).clone()
    }

    pub(crate) fn set(&self, transfers: Transfers) {
        *lock(&self.0) = Some(transfers);
    }
}

/// What a node serves beside its shell.
#[derive(Clone, Default)]
pub struct Offers {
    /// The coding agents and their sessions, made once so the sessions
    /// outlive each connection.
    pub agents: Arc<Agents>,
    /// Protocols the host adds, such as the Mac app's `desktop/1`.
    pub extra: Vec<Arc<dyn Protocol>>,
    /// The log `report/1` shares.
    pub log: Logs,
    /// Takes the user's credentials and installs the tools they are for.
    /// Without it the node does not ask for credentials.
    pub provision: Option<Arc<Provision>>,
    /// The server's connection for the Beep MCP server's calls, set while
    /// one that takes them is connected.
    pub tools: Upstream,
    /// What the node moved, reported over each link.
    pub traffic: Traffic,
    /// The mesh the node belongs to, once it does.
    pub(crate) mesh: Membership,
    /// The node's transfers and synced projects, once they started.
    /// Shared by every copy of the offers, so one made before the services
    /// started sees them once they do.
    pub(crate) transfers: Sharing,
    /// Set while the host does not lend this machine to other devices: they
    /// get its status, but no shell, task or screen.
    pub(crate) withheld: Arc<AtomicBool>,
    /// The user's coding agent skills, synced with the mesh when the host
    /// names their home folder.
    pub skills: Skills,
}

impl Offers {
    /// Whether other devices of the mesh may use this machine's shell,
    /// tasks and screen. On unless the host turned it off; what runs here for
    /// the person at this machine is not affected.
    pub fn lend_to_peers(&self, lent: bool) {
        self.withheld.store(!lent, Ordering::Relaxed);
    }

    pub(crate) fn lends_to_peers(&self) -> bool {
        !self.withheld.load(Ordering::Relaxed)
    }

    /// What every node serves, the CLI and the Mac app alike: the coding
    /// agents found on the PATH (with `agents` from `node.toml`) and their
    /// sessions, and `provision`. A host adds its own protocols to `extra`.
    /// `has_server` says whether the node has a server to link to; without
    /// one the memory tools are left out of what agents are told and see.
    pub fn local(agents: &[AgentKind], provision: Arc<Provision>, has_server: bool) -> Self {
        Self {
            agents: Arc::new(Agents::new(crate::agents::detect(agents))),
            extra: Vec::new(),
            log: Logs::service(),
            provision: Some(provision),
            tools: Upstream::new(has_server),
            traffic: Traffic::default(),
            mesh: Membership::default(),
            transfers: Sharing::default(),
            withheld: Arc::default(),
            skills: Skills::default(),
        }
    }

    /// Whether the node has a server, which holds the memory tools.
    pub fn has_server(&self) -> bool {
        self.tools.has_server()
    }

    /// Only `extra`, no agents or provisioning: a host that serves its own
    /// protocols, as tests do.
    pub fn with_protocols(extra: Vec<Arc<dyn Protocol>>) -> Self {
        Self {
            extra,
            ..Self::default()
        }
    }

    pub(crate) fn names(&self) -> Vec<String> {
        [
            shell::PROTOCOL.to_owned(),
            tasks::PROTOCOL.to_owned(),
            terminal::PROTOCOL.to_owned(),
        ]
        .into_iter()
        .chain(self.agents.offers())
        .chain(
            self.provision
                .iter()
                .flat_map(|_| [tool_update::PROTOCOL, tool_update::FLSH])
                .map(str::to_owned),
        )
        .chain(self.extra.iter().map(|protocol| protocol.name().to_owned()))
        .collect()
    }
}

/// A connection the server welcomed, not yet served.
pub struct Linked {
    connection: Connection,
    tx: LinkTx,
    rx: LinkRx,
    offers: Offers,
    /// What the hello's setup token gave, when it carried one.
    claimed: Option<Claimed>,
    /// The server takes `tools/1` calls.
    tools: bool,
    /// The workspace the hello named, measured first for free disk.
    workspace: Option<PathBuf>,
}

/// Connects to `server` and says hello; `Ok` once the server welcomed this
/// node.
pub async fn open(
    endpoint: &Endpoint,
    server: EndpointAddr,
    name: &str,
    offers: &Offers,
) -> Result<Linked> {
    open_as(endpoint, server, &Hello::named(name), offers).await
}

async fn open_as(
    endpoint: &Endpoint,
    server: EndpointAddr,
    hello: &Hello<'_>,
    offers: &Offers,
) -> Result<Linked> {
    let name = hello.name;
    // A server from before `beep-node/2` answers on `beep-node/1`, in maps.
    let options = ConnectOptions::new()
        .with_transport_config(reconnect::transport())
        .with_additional_alpns(vec![NODE_ALPN_NAMED.to_vec()]);
    let connect = async {
        anyhow::Ok(
            endpoint
                .connect_with_opts(server, NODE_ALPN, options)
                .await?
                .await?,
        )
    };
    let connection = timeout(CONNECT_TIMEOUT, connect)
        .await
        .context("connect timed out")??;
    offers.traffic.watch(&connection);
    let (send, recv) = connection.open_bi().await?;
    let (mut tx, mut rx) = link_on(&connection, send, recv);
    tx.send(&NodeHello {
        version: env!("CARGO_PKG_VERSION").to_owned(),
        name: name.to_owned(),
        offers: offers.names(),
        workspace: hello.workspace.map(str::to_owned),
        setup: hello.setup.cloned(),
        credentials: offers.provision.is_some(),
        checkouts: true,
        agent_settings: true,
        branch_checkouts: true,
    })
    .await?;
    let mut claimed = None;
    let tools = loop {
        match rx.recv::<Down>().await {
            Ok(Some(Down::Welcome { tools })) => break tools,
            Ok(Some(Down::Linked { account, name })) if hello.setup.is_some() => {
                claimed = Some(Claimed { account, name });
            }
            Ok(Some(other)) => bail!("expected a welcome, got {other:?}"),
            Ok(None) | Err(_) => match connection.close_reason() {
                Some(reason) => bail!("{}", refusal(&reason, hello.setup.is_some())),
                None => bail!("the server closed before its welcome"),
            },
        }
    };
    log::info!("connected as {name:?}");
    Ok(Linked {
        connection,
        tx,
        rx,
        offers: offers.clone(),
        claimed,
        tools,
        workspace: hello.workspace.map(PathBuf::from),
    })
}

/// Why the server closed before its welcome, in words for the user.
fn refusal(reason: &ConnectionError, setting_up: bool) -> String {
    if let ConnectionError::ApplicationClosed(close) = reason {
        let text = String::from_utf8_lossy(&close.reason);
        if close.error_code == VarInt::from_u32(CLOSE_SETUP_REFUSED) {
            return text.into_owned();
        }
        // A server from before setup tokens turns the unknown key away
        // without reading the hello.
        if setting_up && text == "not linked" {
            return "This server does not take setup tokens yet. Update it, or link the \
                    machine with `beep-node link`."
                .to_owned();
        }
        // Never linked, or removed from the account by one of its apps; the
        // server closes both the same way. Only a new setup token links it.
        if !setting_up && close.error_code == VarInt::from_u32(CLOSE_REVOKED) {
            return "This machine is not linked to a Beep account, or was removed from \
                    one. To use it, link it with `beep-node setup <token>`."
                .to_owned();
        }
    }
    format!("the server refused this node: {reason}")
}

impl Linked {
    /// Reports the machine and serves sessions until the connection closes.
    /// `Ok` when the server closed it.
    pub async fn serve(self) -> Result<()> {
        let Self {
            connection,
            mut tx,
            mut rx,
            offers,
            claimed: _,
            tools,
            workspace,
        } = self;
        let measured: Vec<PathBuf> = workspace.into_iter().chain(dirs::home_dir()).collect();
        if tools {
            offers.tools.set(Some(connection.clone()));
        }
        let served = serve_linked(&connection, &mut tx, &mut rx, &offers, &measured).await;
        offers.tools.set(None);
        forget_after_link(&offers).await;
        served
    }
}

/// What the node lets go of when its server's link ends. What the mesh owns is the mesh's.
async fn forget_after_link(offers: &Offers) {
    if !offers.mesh.owns(Family::Credentials) {
        // Revoked or only cut off, the node keeps no token it cannot hear
        // about being removed; the server sends them again on reconnecting.
        if let Some(provision) = &offers.provision {
            provision.forget().await;
        }
    }
    // The harnesses' env vars may be secrets too; the server sends the
    // settings again on reconnecting. Each half goes unless the mesh holds it.
    let mut settings = offers.agents.settings();
    if !offers.mesh.owns(Family::Harnesses) {
        settings.harnesses.clear();
    }
    if !offers.mesh.owns(Family::Flsh) {
        settings.flsh = None;
    }
    offers.agents.set_settings(settings);
}

async fn serve_linked(
    connection: &Connection,
    tx: &mut LinkTx,
    rx: &mut LinkRx,
    offers: &Offers,
    measured: &[PathBuf],
) -> Result<()> {
    let mut reprobe = interval(REPROBE);
    reprobe.tick().await;
    // A probe runs a dozen commands, which can take half a minute; it runs
    // beside the loop, the first one too, so the streams the server opens
    // on connecting are taken at once. Left waiting, they time out on the
    // server, and the sync list among them is not sent again until the
    // node reconnects.
    let mut reported: Option<EnvReport> = None;
    let mut probing = Some(AbortOnDropHandle::new(tokio::spawn(env::probe())));
    // The first tick is at once, so stats follow the report.
    let mut resample = interval(RESAMPLE);
    resample.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut sampling: Option<AbortOnDropHandle<StatsReport>> = None;
    let mut report_traffic = interval(REPORT_TRAFFIC);
    report_traffic.set_missed_tick_behavior(MissedTickBehavior::Delay);
    // Streams end with the connection: dropping the set stops them.
    let mut sessions = JoinSet::new();
    let woke = reconnect::woke();
    tokio::pin!(woke);
    loop {
        tokio::select! {
            stream = connection.accept_bi() => {
                let (send, recv) = stream?;
                if sessions.len() >= MAX_SESSIONS {
                    log::warn!("refused a session: {MAX_SESSIONS} already open");
                    continue;
                }
                let (tx, rx) = link_on(connection, send, recv);
                sessions.spawn(session(tx, rx, offers.clone(), Who::Everything(connection.remote_id())));
            }
            Some(ended) = sessions.join_next() => match ended {
                Ok(Ok(())) => {}
                Ok(Err(e)) => log::debug!("session ended: {e:#}"),
                Err(e) => log::warn!("session task failed: {e}"),
            },
            message = rx.recv::<Down>() => match message {
                Ok(Some(Down::Credentials(credentials))) => take_credentials(offers, credentials).await,
                Ok(Some(Down::AgentSettings(settings))) => take_settings(offers, settings),
                Ok(Some(message)) => log::debug!("server sent {message:?}"),
                Ok(None) => return Ok(()),
                Err(e) if connection.close_reason().is_none() => log::debug!("skipped a message: {e:#}"),
                Err(e) => return Err(e),
            },
            // The server dropped this link while the machine slept; this
            // side's timers stopped with it and have yet to notice.
            () = &mut woke => bail!("woke from sleep"),
            _ = reprobe.tick(), if probing.is_none() => {
                probing = Some(AbortOnDropHandle::new(tokio::spawn(env::probe())));
            }
            // A tool was installed: report it without waiting for the tick.
            () = tool_changed(offers.provision.as_deref()), if probing.is_none() => {
                probing = Some(AbortOnDropHandle::new(tokio::spawn(env::probe())));
            }
            _ = report_traffic.tick() => tx.send(&Up::Traffic(offers.traffic.report())).await?,
            _ = resample.tick(), if sampling.is_none() => {
                sampling = Some(AbortOnDropHandle::new(tokio::spawn(stats::sample(measured.to_vec()))));
            }
            Some(sampled) = async { sampling.as_mut()?.await.into() }, if sampling.is_some() => {
                sampling = None;
                match sampled {
                    Ok(report) => tx.send(&Up::Stats(report)).await?,
                    Err(e) => log::warn!("sampling the machine failed: {e}"),
                }
            }
            Some(probed) = async { probing.as_mut()?.await.into() }, if probing.is_some() => {
                probing = None;
                match probed {
                    Ok(now) if reported.as_ref().is_none_or(|before| changed(before, &now)) => {
                        tx.send(&Up::Env(now.clone())).await?;
                        reported = Some(now);
                    }
                    Ok(_) => {}
                    Err(e) => log::warn!("probing the machine failed: {e}"),
                }
            }
        }
    }
}

/// The server's credentials, unless the mesh owns them.
async fn take_credentials(offers: &Offers, credentials: MachineCredentials) {
    // Checked under the turn to apply, so the mesh's credentials, applied once the account
    // moved, are not overwritten by a push that passed the check before it.
    let _applying = offers.mesh.applying().await;
    if offers.mesh.owns(Family::Credentials) {
        log::info!("ignored credentials from the server: this node takes them from its mesh");
        return;
    }
    match &offers.provision {
        Some(provision) => provision.apply(credentials).await,
        None => log::debug!("credentials came to a node that does not take them"),
    }
}

/// The server's agent settings, except the parts the mesh owns.
fn take_settings(offers: &Offers, mut settings: AgentSettings) {
    let (harnesses, flsh) = (
        offers.mesh.owns(Family::Harnesses),
        offers.mesh.owns(Family::Flsh),
    );
    if harnesses || flsh {
        log::info!("ignored agent settings from the server: this node takes them from its mesh");
        let now = offers.agents.settings();
        if harnesses {
            settings.harnesses = now.harnesses;
        }
        if flsh {
            settings.flsh = now.flsh;
        }
    }
    offers.agents.set_settings(settings);
    offers.agents.settings_arrived();
}

/// Who opened a session, and so what it may use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Who {
    /// The server, or a desktop of the mesh, by its key: every protocol the
    /// node offers. A desktop in this process has the node's own key.
    Everything(EndpointId),
    /// Another node of the mesh: its status and agent tools, nothing else.
    Node {
        peer: EndpointId,
        /// This device is a desktop, read from the device list when the
        /// stream opened.
        desktop_here: bool,
    },
}

impl Who {
    /// The key of the caller.
    pub(crate) fn id(self) -> EndpointId {
        match self {
            Self::Everything(id) | Self::Node { peer: id, .. } => id,
        }
    }
}

/// Serves one session stream in the protocol it opens with, if `who` may use it.
pub(crate) async fn session(tx: LinkTx, mut rx: LinkRx, offers: Offers, who: Who) -> Result<()> {
    let open = read_open(&mut rx).await?;
    serve_open(tx, rx, open, offers, who).await
}

/// Reads the protocol a session stream opens with.
pub(crate) async fn read_open(rx: &mut LinkRx) -> Result<Open> {
    timeout(OPEN_TIMEOUT, rx.recv())
        .await
        .context("the session did not say its protocol")??
        .context("the session closed before its protocol")
}

/// Why a device of the mesh may not open `protocol` on a machine that does
/// not lend itself to others, or `None`. Status stays open. The agents are
/// withheld too: their sessions run on this machine's own accounts.
pub(crate) fn withheld(protocol: &str, lent: bool) -> Option<String> {
    let shared = [
        shell::PROTOCOL,
        terminal::PROTOCOL,
        tasks::PROTOCOL,
        desktop::PROTOCOL,
    ];
    let agent = protocol == agent::SESSIONS_PROTOCOL || agent::agent_of(protocol).is_some();
    (!lent && (shared.contains(&protocol) || agent))
        .then(|| format!("this machine is not shared with other devices: {protocol} is off"))
}

/// Serves a session stream that opened with `open`, if `who` may use it.
pub(crate) async fn serve_open(
    mut tx: LinkTx,
    rx: LinkRx,
    open: Open,
    offers: Offers,
    who: Who,
) -> Result<()> {
    if matches!(who, Who::Node { .. })
        && !matches!(
            open.protocol.as_str(),
            status::PROTOCOL | tools::PROTOCOL | skills::PROTOCOL | leave::PROTOCOL
        )
    {
        let reason = format!("a node may not open {}", open.protocol);
        return tx.send(&Opened::Refused { reason }).await;
    }
    match open.protocol.as_str() {
        status::PROTOCOL => {
            tx.send(&Opened::Ready {}).await?;
            peers::status(tx, rx, offers).await
        }
        version::PROTOCOL => {
            tx.send(&Opened::Ready {}).await?;
            tx.send(&version::Version {
                version: env!("CARGO_PKG_VERSION").to_owned(),
                // A clock before 1970 is left unsaid: the desktop warns of nothing.
                now_ms: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .ok()
                    .and_then(|since| u64::try_from(since.as_millis()).ok()),
            })
            .await
        }
        tools::PROTOCOL => {
            tx.send(&Opened::Ready {}).await?;
            let caller = match who {
                Who::Everything(_) => Caller::local(&offers),
                Who::Node { peer, desktop_here } => Caller {
                    table: tasks::Table::Peer(peer),
                    may_run_on_here: !desktop_here,
                    transfers: offers.transfers.get(),
                    agents: Some(offers.agents.clone()),
                },
            };
            peer_tools::serve(tx, rx, caller).await
        }
        shell::PROTOCOL => {
            tx.send(&Opened::Ready {}).await?;
            shell::serve(tx, rx).await
        }
        leave::PROTOCOL => {
            tx.send(&Opened::Ready {}).await?;
            peers::serve_leave(tx, rx, offers.mesh.get(), who.id()).await
        }
        skills::PROTOCOL if offers.skills.home().is_some() => {
            tx.send(&Opened::Ready {}).await?;
            crate::agents::skills::serve(tx, rx, offers.skills).await
        }
        tasks::PROTOCOL => {
            tx.send(&Opened::Ready {}).await?;
            tasks::serve(tx, rx).await
        }
        terminal::PROTOCOL => {
            tx.send(&Opened::Ready {}).await?;
            terminal::serve(tx, rx).await
        }
        tool_update::PROTOCOL | tool_update::FLSH if let Some(provision) = &offers.provision => {
            tx.send(&Opened::Ready {}).await?;
            provision.serve_updates(tx, rx).await
        }
        agent::SESSIONS_PROTOCOL if !offers.agents.offers().is_empty() => {
            tx.send(&Opened::Ready {}).await?;
            crate::agents::serve_sessions(tx, rx, offers.agents, who.id()).await
        }
        other => match offers.agents.kind(other).cloned() {
            Some(kind) => {
                tx.send(&Opened::Ready {}).await?;
                crate::agents::serve_session(tx, rx, offers.agents, kind).await
            }
            None => {
                if let Some(protocol) = offers.extra.iter().find(|p| p.name() == other) {
                    tx.send(&Opened::Ready {}).await?;
                    return protocol.serve(tx, rx).await;
                }
                let reason = format!("this node does not offer {other}");
                tx.send(&Opened::Refused { reason }).await
            }
        },
    }
}

/// Finishes when `provision` installed a tool; never without one.
async fn tool_changed(provision: Option<&Provision>) {
    match provision {
        Some(provision) => provision.changed().await,
        None => std::future::pending().await,
    }
}

pub(crate) fn changed(before: &EnvReport, now: &EnvReport) -> bool {
    before != now
}

#[cfg(test)]
mod tests {
    use super::*;
    use beep_proto::node::harness::HarnessConfig;

    #[test]
    fn a_machine_not_lent_keeps_its_status_but_gives_no_shell_task_or_screen() {
        let dir = tempfile::tempdir().expect("a temp folder");
        let offers = Offers::local(&[], Provision::new(dir.path()), false);
        assert!(offers.lends_to_peers(), "lent until the host says not");
        offers.lend_to_peers(false);
        // Offers made earlier from this one share the setting.
        let copy = offers.clone();
        for protocol in [
            "shell/1",
            "terminal/1",
            "process/1",
            "desktop/1",
            "agents/1",
            "claude/1",
            "codex/1",
        ] {
            assert!(
                withheld(protocol, copy.lends_to_peers()).is_some(),
                "{protocol}"
            );
        }
        for protocol in ["status/1", "version/1", "tools/1", "files/1"] {
            assert_eq!(
                withheld(protocol, copy.lends_to_peers()),
                None,
                "{protocol}"
            );
        }
        offers.lend_to_peers(true);
        assert_eq!(withheld("shell/1", copy.lends_to_peers()), None);
    }

    /// A node with no mesh takes whatever the server pushes, as it always did.
    #[tokio::test]
    async fn a_node_with_no_mesh_takes_the_servers_agent_settings() {
        let dir = tempfile::tempdir().expect("a temp folder");
        let offers = Offers::local(&[], Provision::new(dir.path()), true);
        assert!(offers.mesh.get().is_none());
        assert!(!offers.mesh.owns(Family::Harnesses));
        let off = HarnessConfig {
            enabled: false,
            ..HarnessConfig::new("codex")
        };
        take_settings(
            &offers,
            AgentSettings {
                harnesses: vec![off.clone()],
                flsh: None,
            },
        );
        assert_eq!(offers.agents.settings().harnesses, [off]);
    }

    use beep_mesh::{Key, Mesh, transport_config};
    use beep_proto::{
        SecretText,
        node::{EnvSecret, mesh_state::StateValue},
    };
    use iroh::{Endpoint, SecretKey, endpoint::presets};

    async fn mesh_in(dir: &std::path::Path) -> Mesh {
        let endpoint = Endpoint::builder(presets::Minimal)
            .secret_key(SecretKey::generate())
            .transport_config(transport_config())
            .bind()
            .await
            .expect("the endpoint binds");
        Mesh::create(dir, endpoint, "desktop")
            .await
            .expect("the mesh is made")
    }

    fn var(name: &str, value: &str) -> EnvSecret {
        EnvSecret {
            name: name.to_owned(),
            value: SecretText::new(value),
        }
    }

    fn codex(vars: Vec<EnvSecret>) -> HarnessConfig {
        HarnessConfig {
            env: vars,
            ..HarnessConfig::new("codex")
        }
    }

    /// The mesh has moved the account: `codex` holds one variable entered again, another is only a
    /// record to enter again.
    async fn moved(dir: &std::path::Path) -> (Offers, Mesh) {
        let offers = Offers::local(&[], Provision::new(dir), true);
        let mesh = mesh_in(dir).await;
        offers.mesh.set(mesh.clone());
        let entered = codex(vec![var("B_KEY", "from-the-mesh")]);
        mesh.set(
            Key::Harness("codex".into()),
            Some(entered.to_bytes().expect("encodes")),
        )
        .await
        .expect("written");
        mesh.set(Key::Unmoved("harness/codex/A_KEY".into()), Some(vec![1]))
            .await
            .expect("written");
        mesh.set(Key::Unmoved("credential/7".into()), Some(vec![1]))
            .await
            .expect("written");
        (offers, mesh)
    }

    #[tokio::test]
    async fn a_moved_account_cuts_off_the_servers_credentials_at_once() {
        // Applying and forgetting credentials touch process-wide state.
        let _env = crate::machine::provision::spawn_env::TEST_LOCK.lock().await;
        let dir = tempfile::tempdir().expect("a temp folder");
        let (offers, _mesh) = moved(dir.path()).await;
        assert!(offers.mesh.owns(Family::Credentials));
        assert!(offers.mesh.owns(Family::Harnesses));
        // The server's harness values do not reach the node, even with entries left to enter.
        let before = offers.agents.settings();
        take_settings(
            &offers,
            AgentSettings {
                harnesses: vec![codex(vec![var("A_KEY", "from-the-server")])],
                flsh: None,
            },
        );
        assert_eq!(offers.agents.settings().harnesses, before.harnesses);
        // The server's credentials are not applied either.
        let from_server = MachineCredentials {
            env: vec![var("A", "from-the-server")],
            forgejo: Vec::new(),
        };
        take_credentials(&offers, from_server.clone()).await;
        assert!(
            !crate::machine::provision::spawn_env::vars(
                crate::machine::provision::spawn_env::Scope::Shell
            )
            .iter()
            .any(|(name, _)| name == "A"),
            "the server's credential was applied"
        );
        // A node in a mesh whose account was not moved keeps the per-family rule.
        let plain_dir = tempfile::tempdir().expect("a temp folder");
        let plain = Offers::local(&[], Provision::new(plain_dir.path()), true);
        plain.mesh.set(mesh_in(plain_dir.path()).await);
        assert!(!plain.mesh.owns(Family::Credentials));
        assert!(!plain.mesh.owns(Family::Harnesses));
    }

    #[tokio::test]
    async fn a_server_push_waiting_on_an_apply_does_not_land_after_the_move() {
        let _env = crate::machine::provision::spawn_env::TEST_LOCK.lock().await;
        let dir = tempfile::tempdir().expect("a temp folder");
        let offers = Offers::local(&[], Provision::new(dir.path()), true);
        let mesh = mesh_in(dir.path()).await;
        offers.mesh.set(mesh.clone());
        // The mesh's credentials are being applied when the server's push arrives.
        let applying = offers.mesh.applying().await;
        let push = tokio::spawn({
            let offers = offers.clone();
            let server = MachineCredentials {
                env: vec![var("RACE_KEY", "from-the-server")],
                forgejo: Vec::new(),
            };
            async move { take_credentials(&offers, server).await }
        });
        // The account moves while the push waits.
        mesh.set(Key::Unmoved("credential/7".into()), Some(vec![1]))
            .await
            .expect("written");
        drop(applying);
        push.await.expect("the push ends");
        let applied = crate::machine::provision::spawn_env::vars(
            crate::machine::provision::spawn_env::Scope::Shell,
        );
        assert!(
            !applied.iter().any(|(name, _)| name == "RACE_KEY"),
            "the server's credential landed after the move"
        );
    }
}
