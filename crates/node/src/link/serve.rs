//! The node at work: its endpoint, the mesh, the agents and their MCP
//! server, and the sessions devices of the mesh open, until it stops.

use crate::{
    agents::{AgentKind, Parts, SESSIONS_ALPN, detect, skills::Skills},
    files::Folders,
    files::transfer::Transfers,
    link::Protocol,
    link::from_mesh,
    link::peers::{self, Bound, Inner, Offers, Own, Peers, Who, withheld},
    link::protocol::SessionProtocol,
    link::restart::{self, Reopen},
    link::state::State,
    link::traffic::Traffic,
    machine::provision::Provision,
    machine::service::logs::Logs,
    run::device,
    run::shell,
    run::tasks,
    run::terminal,
    tools::{self, Caller, Node, peer_tools},
    watchdog,
};
use anyhow::{Context, Result, bail};
use beep_mesh::{JOIN_ALPN, PEER_ALPN};
use beep_proto::logging::RequestTrace;
use beep_proto::node::{LinkRx, LinkTx, Opened, version};
use beep_proto::watched;
use futures::future::BoxFuture;
use iroh::{Endpoint, endpoint::presets};
use std::{
    collections::BTreeSet,
    future::Future,
    path::Path,
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::sync::watch;
use tokio::time::timeout;
use tokio_util::task::AbortOnDropHandle;

/// Set, the node binds with no relay and no discovery service. For tests
/// that run nodes side by side; a real node must not set it.
const NO_RELAY: &str = "BEEP_NODE_NO_RELAY";
/// How long a stopping node waits for its agent sessions to close. The stop
/// steps add up to under the 10 s the command line gives a stop before it
/// exits anyway.
const STOP_SESSIONS: Duration = Duration::from_secs(3);
/// How long a stopping node gives each other step.
const STOP_STEP: Duration = Duration::from_secs(1);

/// Serves the mesh until `stop` finishes, then closes the agent sessions and
/// returns. `workspace`, when given, is shared through `files/1` before the
/// folders in `node.toml`.
pub async fn serve(
    state: State,
    workspace: Option<&Path>,
    stop: impl Future<Output = ()>,
) -> Result<()> {
    let traffic = Traffic::default();
    let relays_off = std::env::var_os(NO_RELAY).is_some();
    crate::agents::watch_ports();

    if relays_off {
        log::warn!("{NO_RELAY} is set: no relay, no discovery (for tests only)");
    }

    let builder = || {
        // Tests run nodes side by side on one machine, without relays.
        let preset = if relays_off {
            Endpoint::builder(presets::Minimal)
        } else {
            Endpoint::builder(presets::N0).dns_resolver(beep_proto::dns::resolver())
        };
        preset
            .secret_key(state.key.clone())
            .transport_config(beep_mesh::transport_config())
            .hooks(traffic.clone())
    };
    let endpoint = beep_proto::port::bind_on(builder, state.port).await?;
    traffic.measure_endpoint(endpoint.metrics().clone());
    let diagnostics = super::diagnostics::install().and_then(|recorder| {
        super::diagnostics::start(recorder, &endpoint, &state.dir.join("metrics.log"))
    });
    let _diagnostics = match diagnostics {
        Ok(task) => Some(task),
        Err(e) => {
            log::warn!("metrics unavailable: {e:#}");
            None
        }
    };
    log::info!("node key {}", endpoint.id());
    // Made once, so agent sessions outlive each connection.
    // Installs gh, teacli and the agent skills beside serving; the work
    // stops with the node.
    let provision = Provision::new(&state.dir);
    let _provision = provision.start();
    let _sccache = provision.start_sccache();
    // Files only when the owner shared folders in node.toml or a workspace.
    let shared: Vec<_> = workspace
        .map(Path::to_path_buf)
        .into_iter()
        .chain(state.files.iter().cloned())
        .collect();
    // The agents' Beep MCP server runs this binary against a socket in the
    // node's folder.
    let mcp = tools::bind(&state.dir);
    let parts = Parts {
        kinds: detect(&state.agents, Some(provision.dir())),
        launch: mcp.as_ref().map(tools::Socket::launch),
        skills_dir: crate::machine::provision::skills_dir(),
        folders: shared.clone(),
        search: Some(state.dir.join("search.sqlite")),
        ..Parts::default()
    };
    let mut offers = Offers::local(parts, provision).bound(&endpoint, &state.dir, &state.name);
    let _upkeep = offers.agents.start_upkeep();
    let _events = offers.agents.events.forward_system();
    let _uploads = crate::agents::uploads::maintain(offers.agents.sessions.subscribe());
    // The ACP adapters an agent found here lacks, installed in the node's
    // folder; the work stops with the node.
    let _adapters = offers
        .agents
        .catalog
        .install_adapters(state.dir.clone(), state.agents.clone());
    offers.traffic = traffic;
    if let Some(home) = dirs::home_dir() {
        offers.skills = Skills::under(home);
    }
    offers.extra.push(Arc::new(Logs::default()));
    // Only the CLI updates itself; the Mac app's node updates with the app.
    #[cfg(feature = "cli")]
    {
        let updates = crate::upkeep::update::Updates::new(offers.agents.reopen.clone());
        offers.extra.push(Arc::new(updates));
    }
    let folders = Folders::new(&shared);
    // The agents' logs read once ahead, so the first file request for a
    // session started outside Beep does not wait for them.
    offers.offer_searches(&state.dir, &state.search);
    // The plans read ahead too: Claude Code takes a while to start, and a
    // desktop opening its window would otherwise wait for it.
    let usage = offers.agents.usage.clone();
    let _plans = AbortOnDropHandle::new(tokio::spawn(async move {
        usage.read_plans_at_start().await;
    }));
    let mut started = None;
    let mut served = None;
    // Transfers and sync run on every node: sync for the projects the mesh
    // names, wherever they are, and transfers for `send_files`. Other machines
    // reach transfers only with shared folders. Without its store the node
    // still serves the rest.
    match Transfers::start_shared(
        &endpoint,
        &state.dir.join("blobs"),
        folders.clone(),
        offers.mesh.clone(),
        offers.occupancy.clone(),
    )
    .await
    {
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
    // The mesh: peers reach the node over `beep-peer/2`. The router runs
    // from here, so a join takes effect at once.
    let peers = Peers::new(offers.clone()).await;

    if let Err(e) = peers.open().await {
        log::warn!("not serving the mesh: {e:#}");
    }

    if !peers.is_member() {
        log::info!("not in a mesh: waiting for `beep-node mesh join <ticket>`");
    }

    let mut router = beep_mesh::router(&endpoint)
        .accept(PEER_ALPN, peers.peer_handler())
        .accept(JOIN_ALPN, peers.join_handler())
        .accept(SESSIONS_ALPN, peers.sessions_handler())
        .accept(
            beep_proto::node::commands::ALPN,
            crate::link::commands::CommandsAlpn::new(&peers),
        )
        .accept(
            beep_proto::node::subscriptions::ALPN,
            crate::link::subscriptions::SubscriptionsAlpn::new(&peers),
        )
        .accept(
            beep_proto::node::forward::ALPN,
            crate::link::forward::ForwardAlpn::new(&peers),
        )
        .accept(beep_proto::team::ALPN, peers.team_handler());
    if let Some(served) = served {
        router = served.add_to(router);
    }

    let router = router.spawn();
    log::info!("offering {:?}", offers.names());
    // Claude Code's hooks report to the node over a socket in its folder.
    let occupancy = offers.occupancy.clone();
    let hooks_dir = state.dir.clone();
    let _hooks = AbortOnDropHandle::new(tokio::spawn(async move {
        if let Err(e) = occupancy.listen_hooks(&hooks_dir).await {
            log::warn!("not taking Claude Code hooks: {e:#}");
        }
    }));
    let occupancy = offers.occupancy.clone();
    let presence_dir = state.dir.clone();
    let _presence = AbortOnDropHandle::new(tokio::spawn(async move {
        if let Err(e) = occupancy.listen_presence_in(&presence_dir).await {
            log::warn!("not taking session presence: {e:#}");
        }
    }));
    let _mcp = mcp.map(|socket| tools::serve(socket, Node::of(&offers)));
    // Desktops read the device list when adb sees a phone come or go.
    let _devices = AbortOnDropHandle::new(tokio::spawn(device::track_phones()));
    // Logs when the async workers, or a step that hung before, stop moving.
    let _watchdog = watchdog::start();
    // `beep-node update` asks the node to hold still over another one.
    let reopen = offers.agents.reopen.clone();
    let control_dir = state.dir.clone();
    let _control = AbortOnDropHandle::new(tokio::spawn(async move {
        if let Err(e) = restart::listen(&control_dir, reopen).await {
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
    // sessions once the mesh gave the agent settings they start with.
    // Only then is the note kept to what is open, so a start that dies
    // before leaves it as it was.
    let reopen = Reopen::read(&state.dir);
    offers.agents.reopen.keep_saved(reopen.saved);
    terminal::reopen(&state.dir, reopen.terminals).await;
    let agents = offers.agents.clone();
    let note_dir = state.dir.clone();
    let mut keeping = AbortOnDropHandle::new(tokio::spawn(async move {
        agents.reopen(reopen.sessions).await;
        restart::keep(&note_dir, &agents).await;
    }));
    stop.await;
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

    let reopen = Reopen::new(
        offers.agents.reopen.open_sessions(),
        terminals,
        offers.agents.reopen.saved_sessions(),
    );

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

/// Why another node may not run commands or start agents here, if
/// "Let Beep use this computer" is off.
fn commands_off(offers: &Offers) -> Option<String> {
    if offers.lends_to_peers() {
        return None;
    }

    let name = offers
        .mesh
        .get()
        .and_then(|mesh| {
            let me = mesh.me();
            mesh.devices()
                .into_iter()
                .find(|status| status.device.id == me)
                .map(|status| status.device.name)
        })
        .unwrap_or_else(|| "This machine".to_owned());
    Some(format!(
        "{name} doesn't run commands: Let Beep use this computer is off in its Settings."
    ))
}

/// Why a desktop's agent may not run commands or start agents here. The
/// desktop in this process, which has the node's own key, is the user at
/// this machine and is never held back; a desktop of the mesh, a phone
/// included, is when this machine is not lent.
fn desktop_commands_off(offers: &Offers, peer: iroh::EndpointId, own: iroh::EndpointId) -> Option<String> {
    (peer != own).then(|| commands_off(offers)).flatten()
}

/// Whether another node of the mesh may open `protocol` here: status and
/// agent tools, and the devices its agents boot and drive (`device/1`, kept
/// off by `withheld` while this machine is not lent). A node reads only some
/// topics of `peer-subscriptions/1` (see `events::Reach`).
fn node_may_open(protocol: &SessionProtocol) -> bool {
    matches!(
        protocol,
        SessionProtocol::Status
            | SessionProtocol::Tools
            | SessionProtocol::Skills
            | SessionProtocol::SkillLibrary
            | SessionProtocol::Transcripts
            | SessionProtocol::Subscriptions
            | SessionProtocol::Device
            | SessionProtocol::DeviceVideo
    )
}

/// Serves a session stream that opened with `protocol`, if `who` may use it.
pub(super) async fn serve_open(
    mut tx: LinkTx,
    rx: LinkRx,
    protocol: SessionProtocol,
    offers: Offers,
    who: Who,
    compress: bool,
) -> Result<()> {
    if matches!(who, Who::Node { .. }) && !node_may_open(&protocol) {
        let reason = format!("a node may not open {}", protocol);
        return tx.send(&Opened::Refused { reason }).await;
    }

    let Some(route) = route(&protocol, &offers) else {
        let reason = format!("this node does not offer {protocol}");
        return tx.send(&Opened::Refused { reason }).await;
    };

    if compress && protocol != SessionProtocol::DeviceVideo {
        tx.send(&Opened::ReadyCompressed {}).await?;
        tx.enable_zstd();
    } else {
        tx.send(&Opened::Ready {}).await?;
    }

    match route {
        Route::Agent(kind) => crate::agents::serve_session(tx, rx, offers.agents, kind).await,
        Route::Extra(extra) => extra.serve(tx, rx).await,
        Route::Provision(provision) => match protocol {
            SessionProtocol::ToolCheck => provision.serve_tool_checks(tx, rx).await,
            SessionProtocol::Setup => provision.serve_setup(tx, rx).await,
            _ => provision.serve_updates(tx, rx).await,
        },
        Route::BuiltIn => match protocol {
            SessionProtocol::Status => peers::status(tx, rx, offers).await,
            SessionProtocol::Version => {
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
            SessionProtocol::Tools => {
                let node = offers.made_node().context("this node is not made yet")?;
                let caller = match who {
                    Who::Everything(peer) => Caller {
                        from: Some(peer),
                        commands_off: desktop_commands_off(&offers, peer, node.own.endpoint.id()),
                        ..Caller::of(&node)
                    },
                    Who::Node { peer, desktop_here } => Caller {
                        table: tasks::Table::Peer(peer),
                        from: Some(peer),
                        desktop_here,
                        commands_off: commands_off(&offers),
                        ..Caller::of(&node)
                    },
                };
                peer_tools::serve(tx, rx, caller).await
            }
            SessionProtocol::Shell => shell::serve(tx, rx).await,
            SessionProtocol::Skills => {
                let desktop = matches!(who, Who::Everything(_));
                let mesh = offers.mesh.get().context("this node is not in a mesh")?;
                crate::agents::skills::serve(tx, rx, offers.skills, mesh, desktop).await
            }
            SessionProtocol::SkillLibrary => {
                crate::agents::skill_library::serve(
                    tx,
                    rx,
                    offers.skills.home().cloned(),
                    offers.skills.libraries().clone(),
                )
                .await
            }
            SessionProtocol::Tasks => tasks::serve(tx, rx).await,
            SessionProtocol::Terminal => terminal::serve(tx, rx).await,
            SessionProtocol::Device => offers.agents.reach.serve_device(tx, rx).await,
            SessionProtocol::DeviceVideo => device::video::serve(tx, rx).await,
            SessionProtocol::SignIn => crate::machine::sign_in::serve(tx, rx).await,
            SessionProtocol::McpServers => tools::configured::serve(offers.agents, tx, rx).await,
            SessionProtocol::Commands => {
                super::commands::serve_open(tx, rx, offers, who.id()).await
            }
            SessionProtocol::Subscriptions => {
                let reach = match who {
                    Who::Everything(_) => super::events::Reach::Whole,
                    Who::Node { .. } => super::events::Reach::Node,
                };
                super::subscriptions::serve_open(tx, rx, offers, who.id(), reach).await
            }
            SessionProtocol::Events => super::events::serve(tx, rx, offers, who.id()).await,
            SessionProtocol::AgentFollow => {
                crate::agents::serve_follow(tx, rx, offers.agents).await
            }
            SessionProtocol::Transcripts => {
                crate::agents::serve_transcripts(tx, rx, offers.agents).await
            }
            SessionProtocol::Agents => {
                let transfers = offers.transfers.get();
                crate::agents::serve_sessions(tx, rx, offers.agents, who.id(), transfers).await
            }
            _ => bail!("this node does not offer {protocol}"),
        },
    }
}

enum Route {
    BuiltIn,
    Provision(Arc<Provision>),
    Agent(AgentKind),
    Extra(Arc<dyn Protocol>),
}

/// How the node serves `protocol`, if it offers it: itself, with what it
/// needs, else an agent or an extra protocol.
fn route(protocol: &SessionProtocol, offers: &Offers) -> Option<Route> {
    match protocol {
        SessionProtocol::Skills if offers.skills.home().is_some() => return Some(Route::BuiltIn),
        SessionProtocol::ToolCheck
        | SessionProtocol::ToolUpdate
        | SessionProtocol::FlshUpdate
        | SessionProtocol::Setup
            if let Some(provision) = &offers.provision =>
        {
            return Some(Route::Provision(provision.clone()));
        }
        SessionProtocol::Status
        | SessionProtocol::Version
        | SessionProtocol::Tools
        | SessionProtocol::Shell
        | SessionProtocol::SkillLibrary
        | SessionProtocol::Tasks
        | SessionProtocol::Terminal
        | SessionProtocol::Device
        | SessionProtocol::DeviceVideo
        | SessionProtocol::SignIn
        | SessionProtocol::McpServers
        | SessionProtocol::Commands
        | SessionProtocol::Subscriptions
        | SessionProtocol::Events
        | SessionProtocol::AgentFollow
        | SessionProtocol::Transcripts
        | SessionProtocol::Agents => return Some(Route::BuiltIn),
        _ => {}
    }

    if let Some(kind) = offers.agents.kind(protocol.as_ref()) {
        return Some(Route::Agent(kind));
    }

    offers
        .extra
        .iter()
        .find(|p| p.name() == protocol.as_ref())
        .map(|extra| Route::Extra(extra.clone()))
}

impl Peers {
    /// The node's membership over `offers`, made by `Offers::bound`: that
    /// is where the node's key and team link are made, before any copy of
    /// the offers goes elsewhere.
    pub async fn new(bound: Bound) -> Self {
        let Bound { offers, own, team } = bound;
        let inner = Arc::new(Inner {
            own,
            team,
            offers: Mutex::new(offers),
            later: watch::channel(BTreeSet::new()).0,
            active: Mutex::new(None),
            follower: Mutex::new(None),
            _following: OnceLock::new(),
            serve_protocol: serve_stream,
        });
        // Started once the node exists, which it upgrades to read.
        let following = tokio::spawn(from_mesh::run(Arc::downgrade(&inner)));
        inner._following.set(AbortOnDropHandle::new(following)).ok();

        Self { inner }
    }
}

/// Serves a stream that opened with `protocol`, then ends its trace.
fn serve_stream(
    inner: Arc<Inner>,
    tx: LinkTx,
    rx: LinkRx,
    protocol: SessionProtocol,
    who: Who,
    compress: bool,
    mut trace: RequestTrace,
) -> BoxFuture<'static, Result<()>> {
    Box::pin(async move {
        let result = serve_protocol(&inner, tx, rx, protocol, who, compress, &mut trace).await;
        trace.end(&result);
        result
    })
}

async fn serve_protocol(
    inner: &Inner,
    tx: LinkTx,
    rx: LinkRx,
    protocol: SessionProtocol,
    who: Who,
    compress: bool,
    trace: &mut beep_proto::logging::RequestTrace,
) -> Result<()> {
    // A protocol still on its way is waited for; the others are not held up.
    let mut later = inner.later.subscribe();

    if watched::wait_for(&mut later, |later| {
        !later.contains::<str>(protocol.as_ref())
    })
    .await
    .is_err()
    {
        bail!("the node stopped");
    }

    trace.phase("serving");
    trace.long_lived();
    let offers = inner.offers().clone();
    // Diagnostics are open to every machine of the mesh, even when not lent.
    if Own::is_diagnostic(&protocol) {
        let node = offers.node(&inner.own, &inner.team);
        return inner
            .own
            .serve_diagnostic(protocol, tx, rx, node, compress)
            .await;
    }

    // The person at this machine is not held to the setting; a device of the mesh is.
    if who.id() != inner.own.endpoint.id()
        && let Some(reason) = withheld(protocol.as_ref(), offers.lends_to_peers())
    {
        let mut tx = tx;
        return tx.send(&Opened::Refused { reason }).await;
    }

    serve_open(tx, rx, protocol, offers, who, compress).await
}

#[cfg(test)]
#[path = "../tests/link_serve.rs"]
mod tests;
