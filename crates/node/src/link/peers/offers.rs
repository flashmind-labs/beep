//! What a node serves (`Offers`), and who opened a session (`Who`).

use super::Reach;
use crate::{
    agents::{AgentKind, Agents, TRANSCRIPT_PROTOCOL, desktop_call::ProjectTools, skills::Skills},
    files::{search::Searches, transfer::Transfers},
    link::{
        EventSources, from_mesh::Membership, protocol::SessionProtocol, sampler::Sampler,
        state::Search, traffic::Traffic,
    },
    protocol::Protocol,
    machine::{provision::Provision, service::logs::Logs},
    run::{device, shell, tasks, terminal},
};
use anyhow::{Context, Result};
use beep_proto::node::{
    LinkRx, Open, agent, desktop, mcp_servers, setup, sign_in, tool_update,
};
use beep_proto::watched;
use iroh::EndpointId;
use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::{sync::watch, time::timeout};

/// How long a device has to name a session's protocol.
const OPEN_TIMEOUT: Duration = Duration::from_secs(10);

/// The node's transfers, set once its services are up and read by whoever
/// holds a copy of the offers.
#[derive(Clone)]
pub(crate) struct Sharing(Arc<watch::Sender<Option<Transfers>>>);

impl Default for Sharing {
    fn default() -> Self {
        Self(Arc::new(watch::Sender::new(None)))
    }
}

impl Sharing {
    pub(crate) fn get(&self) -> Option<Transfers> {
        watched::read_sent(&self.0, Clone::clone)
    }

    pub(crate) fn set(&self, transfers: Transfers) {
        self.0.send_replace(Some(transfers));
    }

    /// Changes when the transfers are set.
    pub(crate) fn subscribe(&self) -> watch::Receiver<Option<Transfers>> {
        self.0.subscribe()
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
    /// What the node moved, as its status reports it.
    pub traffic: Traffic,
    /// The machine's resources and traffic, sampled for the streams that follow.
    pub(crate) sampler: Sampler,
    /// The mesh the node belongs to, once it does.
    pub(crate) mesh: Membership,
    /// The node's transfers and synced projects, once they started.
    /// Shared by every copy of the offers, so one made before the services
    /// started sees them once they do.
    pub(crate) transfers: Sharing,
    /// The sources the event streams share, by topic.
    pub(crate) event_sources: Arc<EventSources>,
    /// Set while the host does not lend this machine to other devices: they
    /// get its status, but no shell, task or screen.
    pub(crate) withheld: Arc<AtomicBool>,
    /// Holds the next bind of a root, for a test.
    #[cfg(test)]
    pub(crate) bind_gates: Arc<std::sync::Mutex<Vec<crate::link::events::sources::Gate>>>,
    /// The user's coding agent skills, synced with the mesh when the host
    /// names their home folder.
    pub skills: Skills,
    /// The node these offers are served by, once it is made.
    pub(crate) reach: Reach,
    /// Answers the assistant's project tools, for the desktop app. Other
    /// nodes send them to a desktop of the mesh.
    pub project_tools: Option<Arc<dyn ProjectTools>>,
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
    pub fn local(agents: &[AgentKind], provision: Arc<Provision>) -> Self {
        Self {
            agents: Arc::new(Agents::new(crate::agents::detect(
                agents,
                Some(provision.dir()),
            ))),
            extra: Vec::new(),
            log: Logs::default(),
            provision: Some(provision),
            traffic: Traffic::default(),
            sampler: Sampler::default(),
            mesh: Membership::default(),
            transfers: Sharing::default(),
            event_sources: Arc::default(),
            withheld: Arc::default(),
            #[cfg(test)]
            bind_gates: Arc::default(),
            skills: Skills::default(),
            reach: Reach::default(),
            project_tools: None,
        }
    }

    /// Offers `search/1`: the checkouts saved in the node's folder `dir`,
    /// read ahead from the agents' logs. Needs a running Tokio runtime.
    pub fn offer_searches(&mut self, dir: &Path, search: &Search) {
        let searches = Searches::start(dir.to_owned(), self.agents.clone(), search);
        self.extra.push(Arc::new(searches));
    }

    pub(crate) fn names(&self) -> Vec<String> {
        [
            shell::PROTOCOL.to_owned(),
            tasks::PROTOCOL.to_owned(),
            terminal::PROTOCOL.to_owned(),
            device::PROTOCOL.to_owned(),
            device::video::PROTOCOL.to_owned(),
            sign_in::PROTOCOL.to_owned(),
            mcp_servers::PROTOCOL.to_owned(),
        ]
        .into_iter()
        .chain(self.agents.offers())
        .chain(
            self.provision
                .iter()
                .flat_map(|_| {
                    [
                        tool_update::PROTOCOL,
                        tool_update::FLSH,
                        tool_update::CHECK,
                        setup::PROTOCOL,
                    ]
                })
                .map(str::to_owned),
        )
        .chain(self.extra.iter().map(|protocol| protocol.name().to_owned()))
        .collect()
    }
}

/// Who opened a session, and so what it may use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Who {
    /// A desktop of the mesh, by its key: every protocol the node offers. A desktop in this process has the node's own key.
    Everything(EndpointId),
    /// Another node of the mesh: its status, agent tools and devices,
    /// nothing else.
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

/// Reads the protocol a session stream opens with.
pub(crate) async fn read_open(rx: &mut LinkRx) -> Result<SessionProtocol> {
    let open: Open = timeout(OPEN_TIMEOUT, rx.recv())
        .await
        .context("the session did not say its protocol")??
        .context("the session closed before its protocol")?;
    Ok(SessionProtocol::from(open.protocol.as_ref()))
}

/// Why a device of the mesh may not open `protocol` on a machine that does
/// not lend itself to others, or `None`. Status stays open. The agents are
/// withheld too: their sessions run on this machine's own accounts.
pub(crate) fn withheld(protocol: &str, lent: bool) -> Option<String> {
    let shared = [
        shell::PROTOCOL,
        terminal::PROTOCOL,
        device::PROTOCOL,
        device::video::PROTOCOL,
        tasks::PROTOCOL,
        desktop::PROTOCOL,
        sign_in::PROTOCOL,
        mcp_servers::PROTOCOL,
    ];
    let agent = matches!(
        protocol,
        agent::SESSIONS_PROTOCOL
            | agent::SESSION_FOLLOW_PROTOCOL
            | beep_proto::node::events::PROTOCOL
            | TRANSCRIPT_PROTOCOL
    ) || agent::agent_of(protocol).is_some();
    (!lent && (shared.contains(&protocol) || agent))
        .then(|| format!("this machine is not shared with other devices: {protocol} is off"))
}

#[cfg(test)]
#[path = "../../tests/link_peers_offers.rs"]
mod tests;
