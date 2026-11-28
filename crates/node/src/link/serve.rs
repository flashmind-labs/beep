//! The node at work: its endpoint, the mesh, the agents and their MCP
//! server, and the sessions devices of the mesh open, until it stops.

use crate::{
    agents::{Parts, detect, skills::Skills},
    files::Folders,
    files::transfer::Transfers,
    link::channels,
    link::from_mesh,
    link::peers::{self, Bound, Inner, Offers, Peers},
    link::restart::{self, Reopen},
    link::state::State,
    link::streams,
    link::traffic::Traffic,
    machine::provision::Provision,
    machine::service::logs::Logs,
    run::device,
    run::terminal,
    tools::{self, Node},
    watchdog,
};
use anyhow::Result;
use iroh::{Endpoint, endpoint::presets};
use std::{
    collections::BTreeSet,
    future::Future,
    path::Path,
    sync::{Arc, Mutex, OnceLock},
    time::Duration,
};
use tokio::sync::{mpsc, watch};
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

    let mut router = channels::mount(&peers, beep_mesh::router(&endpoint));
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
        agents.starter.reopen(reopen.sessions).await;
        restart::keep(&note_dir, &agents).await;
    }));
    stop.await;
    // Agents that end from here on ended with the node, whatever their exit.
    offers.agents.sessions.begin_stopping();
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

impl Peers {
    /// The node's membership over `offers`, made by `Offers::bound`: that
    /// is where the node's key and team link are made, before any copy of
    /// the offers goes elsewhere.
    pub async fn new(bound: Bound) -> Self {
        let Bound { offers, own, team } = bound;
        let (installed, incoming) = mpsc::unbounded_channel();
        let inner = Arc::new(Inner {
            own,
            team,
            offers: Mutex::new(offers),
            later: watch::channel(BTreeSet::new()).0,
            active: Mutex::new(None),
            follower: Mutex::new(None),
            installed,
            _following: OnceLock::new(),
            _serving: OnceLock::new(),
        });
        // Started once the node exists, which they upgrade to read.
        let following = tokio::spawn(from_mesh::run(Arc::downgrade(&inner)));
        inner._following.set(AbortOnDropHandle::new(following)).ok();
        let membership = inner.offers().mesh.subscribe();
        let serving = tokio::spawn(streams::serve_meshes(
            Arc::downgrade(&inner),
            incoming,
            membership,
        ));
        inner._serving.set(AbortOnDropHandle::new(serving)).ok();

        Self { inner }
    }
}
