//! What a node serves (`Offers`), and who opened a session (`Who`).

use crate::{
    agents::{Agents, Parts, TRANSCRIPT_PROTOCOL, skills::Skills},
    files::transfer::Sharing,
    link::{
        EventSources, protocol::SessionProtocol, sampler::Sampler, search::Searches, state::Search,
        traffic::Traffic,
    },
    machine::{provision::Provision, service::logs::Logs},
    membership::Membership,
    occupancy::Occupancy,
    protocol::Protocol,
    run::{device, shell, tasks, terminal},
    status::Own,
    team::{home::ForgeCli, link::TeamLink},
    tools::{Node, ProjectTools},
};
use anyhow::{Context, Result};
use beep_mesh::DeviceKind;
use beep_proto::node::{LinkRx, Open, agent, desktop, mcp_servers, setup, sign_in, tool_update};
use iroh::{Endpoint, EndpointId};
use std::{
    path::Path,
    sync::Arc,
    time::{Duration, SystemTime},
};
use tokio::time::timeout;

/// How long a device has to name a session's protocol.
const OPEN_TIMEOUT: Duration = Duration::from_secs(10);

/// Offers made with the node's own values, which `Peers::new` takes. It
/// derefs to the offers.
#[derive(Clone)]
pub struct Bound {
    pub(crate) offers: Offers,
    pub(crate) own: Own,
    pub(crate) team: TeamLink,
}

impl std::ops::Deref for Bound {
    type Target = Offers;

    fn deref(&self) -> &Offers {
        &self.offers
    }
}

impl std::ops::DerefMut for Bound {
    fn deref_mut(&mut self) -> &mut Offers {
        &mut self.offers
    }
}

#[cfg(test)]
impl Node {
    /// A node of `dir` in no mesh, with no agents, for a test.
    pub(crate) async fn test(dir: &Path) -> Self {
        use iroh::endpoint::presets;

        let endpoint = Endpoint::builder(presets::Minimal)
            .bind()
            .await
            .expect("the endpoint binds");
        let bound =
            Offers::local(Parts::default(), Provision::new(dir)).bound(&endpoint, dir, "build-box");
        Self::of(&bound)
    }
}

#[cfg(test)]
impl Node {
    /// A node of `link`, with no agents and in no mesh, for a test of the
    /// team link's server.
    pub(crate) fn of_link(link: &TeamLink) -> Self {
        let own = Own {
            endpoint: link.endpoint().clone(),
            dir: std::path::PathBuf::new(),
            name: "build-box".to_owned(),
            started: SystemTime::now(),
        };

        Offers::default().node(&own, link)
    }
}

impl Node {
    /// What the tools read of the node `bound` made.
    #[must_use]
    pub fn of(bound: &Bound) -> Self {
        bound.offers.node(&bound.own, &bound.team)
    }
}

/// What a node serves beside its shell.
#[derive(Clone)]
pub struct Offers {
    /// The coding agents and their sessions, made once so the sessions
    /// outlive each connection.
    pub agents: Arc<Agents>,
    /// Who works in which folder here, which the agents and sync share.
    pub occupancy: Occupancy,
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
    /// Whether the host lends this machine to other devices. While not, they
    /// get its status, but no shell, task or screen. Set only by
    /// `lend_to_peers`; open streams follow it.
    pub(crate) lending: Arc<tokio::sync::watch::Sender<bool>>,
    /// Holds the next bind of a root, for a test.
    #[cfg(test)]
    pub(crate) bind_gates: Arc<std::sync::Mutex<Vec<crate::link::events::sources::Gate>>>,
    /// The user's coding agent skills, synced with the mesh when the host
    /// names their home folder.
    pub skills: Skills,
    /// The node's team link, once the node is made; the team topics read it.
    pub(crate) team: Option<TeamLink>,
    /// Answers the assistant's project tools, for the desktop app. Other
    /// nodes send them to a desktop of the mesh.
    pub project_tools: Option<Arc<dyn ProjectTools>>,
}

impl Default for Offers {
    /// Offers with no agents, whose sessions and sync share one occupancy.
    fn default() -> Self {
        Self::of(Parts::default(), None)
    }
}

impl Offers {
    /// Whether other devices of the mesh may use this machine's shell,
    /// tasks and screen. On unless the host turned it off; what runs here for
    /// the person at this machine is not affected.
    pub fn lend_to_peers(&self, lent: bool) {
        self.lending.send_replace(lent);
    }

    pub(crate) fn lends_to_peers(&self) -> bool {
        beep_proto::watched::read_sent(&self.lending, |lent| *lent)
    }

    /// Follows the lending setting, for a stream that ends when it goes off.
    pub(crate) fn lending(&self) -> tokio::sync::watch::Receiver<bool> {
        self.lending.subscribe()
    }

    /// Makes the node's own values, its key, name, folder and team link,
    /// before any copy of the offers is handed out: every copy shares the
    /// team link, and `Peers::new` takes both from the result.
    #[must_use]
    pub fn bound(mut self, endpoint: &Endpoint, dir: &Path, name: &str) -> Bound {
        let team = TeamLink::new(endpoint.clone(), Arc::new(ForgeCli));
        let own = Own {
            endpoint: endpoint.clone(),
            dir: dir.to_owned(),
            name: name.to_owned(),
            started: SystemTime::now(),
        };
        self.team = Some(team.clone());

        Bound {
            offers: self,
            own,
            team,
        }
    }

    /// What the Beep MCP server's tools read of this node: the sessions,
    /// the mesh and the node's own values.
    pub(crate) fn node(&self, own: &Own, team: &TeamLink) -> Node {
        Node {
            agents: self.agents.clone(),
            mesh: self.mesh.clone(),
            transfers: self.transfers.clone(),
            team: team.clone(),
            project_tools: self.project_tools.clone(),
            own: own.clone(),
            log: self.log.clone(),
        }
    }

    /// What every node serves, the CLI and the Mac app alike: the coding
    /// agents in `parts.kinds` and their sessions, and `provision`. The
    /// agents share the occupancy `local` makes with the offers, so `parts`
    /// names the rest. A host adds its own protocols to `extra`.
    pub fn local(parts: Parts, provision: Arc<Provision>) -> Self {
        let parts = Parts {
            occupancy: Occupancy::new(),
            ..parts
        };

        Self::of(parts, Some(provision))
    }

    /// Offers with no provision over agents made of `parts`, for a test.
    #[cfg(test)]
    pub(crate) fn from_parts(parts: Parts) -> Self {
        Self::of(parts, None)
    }

    /// Offers whose agents, sync and mesh share `parts`' occupancy and mesh.
    fn of(parts: Parts, provision: Option<Arc<Provision>>) -> Self {
        let (occupancy, mesh) = (parts.occupancy.clone(), parts.mesh.clone());

        Self {
            agents: Arc::new(Agents::new(parts)),
            occupancy,
            extra: Vec::new(),
            log: Logs::default(),
            provision,
            traffic: Traffic::default(),
            sampler: Sampler::default(),
            mesh,
            transfers: Sharing::default(),
            event_sources: Arc::default(),
            lending: Arc::new(tokio::sync::watch::channel(true).0),
            #[cfg(test)]
            bind_gates: Arc::default(),
            skills: Skills::default(),
            team: None,
            project_tools: None,
        }
    }

    /// Offers `search/1`: the checkouts saved in the node's folder `dir`,
    /// read ahead from the agents' logs. Needs a running Tokio runtime.
    pub fn offer_searches(&mut self, dir: &Path, search: &Search) {
        let searches = Searches::start(dir.to_owned(), self.agents.listings.clone(), search);
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
        .chain(self.agents.catalog.offers())
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
    /// Who a device of `kind` is. `desktop_here` says whether this device is
    /// a desktop, which only a node caller carries.
    pub(crate) fn of(kind: DeviceKind, peer: EndpointId, desktop_here: bool) -> Self {
        match kind {
            DeviceKind::Desktop => Self::Everything(peer),
            DeviceKind::Node => Self::Node { peer, desktop_here },
        }
    }

    /// The key of the caller.
    pub(crate) fn id(self) -> EndpointId {
        match self {
            Self::Everything(id) | Self::Node { peer: id, .. } => id,
        }
    }
}

/// Reads the protocol a session stream opens with and whether it asked for compression.
pub(crate) async fn read_open(rx: &mut LinkRx) -> Result<(SessionProtocol, bool)> {
    let open: Open = timeout(OPEN_TIMEOUT, rx.recv())
        .await
        .context("the session did not say its protocol")??
        .context("the session closed before its protocol")?;
    Ok((SessionProtocol::from(open.protocol.as_ref()), open.compress))
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
        beep_proto::node::subscriptions::PROTOCOL,
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
