//! How the node serves the streams that open on it: a peer's `beep-peer/2`
//! stream, a channel's session stream, or a caller in this process. Each
//! opens with a protocol; it is waited for if still on its way, held back
//! while the machine is not lent, and routed to what serves it.

use crate::{
    agents::AgentKind,
    link::Protocol,
    link::peers::{self, Inner, Installed, Offers, Own, Peers, Who, read_open, withheld},
    link::protocol::SessionProtocol,
    machine::provision::Provision,
    run::device,
    run::shell,
    run::tasks,
    run::terminal,
    tools::{self, Caller, Node, peer_tools},
};
use anyhow::{Context, Result, bail};
use beep_mesh::{Mesh, PeerStream, SessionFn};
use beep_proto::logging::RequestTrace;
use beep_proto::node::{LinkRx, LinkTx, Opened, agent, link, version};
use beep_proto::watched;
use std::{
    sync::{Arc, Weak},
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::{
    sync::{mpsc, watch},
    task::JoinSet,
};

/// Serves the streams of each mesh the node installs, until the mesh is
/// replaced or cleared, then the next. Ends with the node.
pub(super) async fn serve_meshes(
    node: Weak<Inner>,
    mut installed: mpsc::UnboundedReceiver<Installed>,
    mut membership: watch::Receiver<Option<Mesh>>,
) {
    while let Some(Installed { mesh, incoming }) = installed.recv().await {
        // Read and marked seen together: a change after this ends the serving,
        // and a mesh already replaced or cleared is not served.
        let current = watched::read_and_update(&mut membership, |current| {
            current
                .as_ref()
                .is_some_and(|current| current.id() == mesh.id())
        });
        drop(mesh);

        if current {
            serve_incoming(node.clone(), incoming, &mut membership).await;
        }
    }
}

/// Serves each stream a peer opens, by what the peer is now, until the
/// membership changes; the streams being served end with it.
async fn serve_incoming(
    node: Weak<Inner>,
    mut incoming: mpsc::Receiver<PeerStream>,
    membership: &mut watch::Receiver<Option<Mesh>>,
) {
    let mut serving = JoinSet::new();

    loop {
        tokio::select! {
            _ = membership.changed() => {
                serving.shutdown().await;
                return;
            }
            stream = incoming.recv() => match stream {
                Some(stream) => {
                    serving.spawn(serve_peer_stream(node.clone(), stream));
                }
                // The mesh gave its streams to a later install of itself:
                // that one is the next to read.
                None => {
                    serving.shutdown().await;
                    return;
                }
            },
            Some(_) = serving.join_next() => {}
            else => return,
        }
    }
}

/// Serves one stream a peer opened.
async fn serve_peer_stream(node: Weak<Inner>, stream: PeerStream) {
    let Some(inner) = node.upgrade() else {
        return;
    };
    let desktop_here = inner.own.is_desktop(&inner.node());
    // Held to the end of the function: the link counts the stream open
    // while it is served.
    let PeerStream {
        peer,
        kind,
        send,
        recv,
        lease: _lease,
    } = stream;
    let who = Who::of(kind, peer, desktop_here);
    let (tx, rx) = link(send, recv);

    if let Err(e) = serve_stream(&inner, tx, rx, who).await {
        log::debug!("a stream from {peer} ended: {e:#}");
    }
}

/// The node's session for a caller in this process, which is a desktop by
/// being the user at this machine: no network, every protocol.
pub fn local_session(peers: &Peers) -> SessionFn {
    let peers = peers.clone();
    Arc::new(move |tx: LinkTx, rx: LinkRx| {
        let peers = peers.clone();
        Box::pin(async move {
            let who = Who::Everything(peers.inner.own.endpoint.id());

            if let Err(e) = serve_stream(&peers.inner, tx, rx, who).await {
                log::debug!("a local session ended: {e:#}");
            }
        })
    })
}

/// Serves a session stream a channel's peer opened with `protocol`.
pub(super) async fn serve_channel_session(
    peers: &Peers,
    tx: LinkTx,
    rx: LinkRx,
    peer: iroh::EndpointId,
    mesh: iroh::EndpointId,
    protocol: &str,
    compress: bool,
) -> Result<()> {
    let protocol = protocol.parse::<SessionProtocol>()?;
    anyhow::ensure!(
        protocol != SessionProtocol::Events,
        "subscriptions use their own ALPN"
    );
    let trace = RequestTrace::new(protocol.to_string());
    let who = peers.channel_who(peer, mesh).context("not authorized")?;

    serve_traced(&peers.inner, tx, rx, protocol, who, compress, trace).await
}

/// Serves a stream a peer, or this process, opened.
pub(super) async fn serve_stream(
    inner: &Inner,
    tx: LinkTx,
    mut rx: LinkRx,
    who: Who,
) -> Result<()> {
    let (protocol, compress) = read_open(&mut rx).await?;
    let mut trace = RequestTrace::new(protocol.to_string());
    trace.phase("waiting for protocol readiness");

    serve_traced(inner, tx, rx, protocol, who, compress, trace).await
}

/// Serves a stream that opened with `protocol`, then ends its trace.
async fn serve_traced(
    inner: &Inner,
    tx: LinkTx,
    rx: LinkRx,
    protocol: SessionProtocol,
    who: Who,
    compress: bool,
    mut trace: RequestTrace,
) -> Result<()> {
    let result = serve_protocol(inner, tx, rx, protocol, who, compress, &mut trace).await;
    trace.end(&result);

    result
}

async fn serve_protocol(
    inner: &Inner,
    tx: LinkTx,
    rx: LinkRx,
    protocol: SessionProtocol,
    who: Who,
    compress: bool,
    trace: &mut RequestTrace,
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
    let node = offers.node(&inner.own, &inner.team);
    // Diagnostics are open to every machine of the mesh, even when not lent.
    if Own::is_diagnostic(&protocol) {
        return inner
            .own
            .serve_diagnostic(protocol, tx, rx, node, compress)
            .await;
    }

    // The person at this machine is not held to the setting; a device of the mesh is.
    let local = who.id() == node.own.endpoint.id();
    if !local && let Some(reason) = withheld(protocol.as_ref(), offers.lends_to_peers()) {
        let mut tx = tx;
        return tx.send(&Opened::Refused { reason }).await;
    }

    serve_open(tx, rx, protocol, offers, node, who, compress).await
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
fn desktop_commands_off(
    offers: &Offers,
    peer: iroh::EndpointId,
    own: iroh::EndpointId,
) -> Option<String> {
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
    node: Node,
    who: Who,
    compress: bool,
) -> Result<()> {
    // The person at this machine is not held to the setting.
    let local = who.id() == node.own.endpoint.id();

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
            SessionProtocol::McpServers => {
                tools::configured::serve(offers.agents.catalog.clone(), tx, rx).await
            }
            SessionProtocol::Commands => {
                super::commands::serve_open(tx, rx, offers, who.id()).await
            }
            SessionProtocol::Subscriptions => {
                let reach = match who {
                    Who::Everything(_) => super::events::Reach::Whole,
                    Who::Node { .. } => super::events::Reach::Node,
                };
                super::subscriptions::serve_open(tx, rx, offers, who.id(), local, reach).await
            }
            SessionProtocol::Events => super::events::serve(tx, rx, offers, who.id(), local).await,
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

    if let Some(kind) =
        agent::agent_of(protocol.as_ref()).and_then(|name| offers.agents.catalog.named(name))
    {
        return Some(Route::Agent(kind));
    }

    offers
        .extra
        .iter()
        .find(|p| p.name() == protocol.as_ref())
        .map(|extra| Route::Extra(extra.clone()))
}

#[cfg(test)]
#[path = "../tests/link_streams.rs"]
mod tests;
