//! Coding agents (Claude Code, Codex, Antigravity, pi, flsh, OpenCode) run
//! over ACP (`Agents`) and the streams the caller follows them on; sessions
//! started outside Beep (`claude`, `found`).

pub use crate::tags;
mod work_diff;

pub use work_diff::parse_git_patch;

#[cfg(feature = "host")]
pub(crate) mod brief;
#[cfg(feature = "host")]
pub(crate) mod child_reach;
#[cfg(feature = "host")]
pub(crate) mod child_view;
#[cfg(feature = "host")]
pub(crate) use crate::calls;
#[cfg(feature = "host")]
use crate::occupancy::Occupancy;
#[cfg(feature = "host")]
use beep_proto::node::agent::Status;
#[cfg(feature = "host")]
use beep_proto::watched;
#[cfg(feature = "host")]
use std::{path::PathBuf, sync::Arc, time::Duration};

#[cfg(feature = "host")]
pub mod claude;
#[cfg(feature = "host")]
mod claude_direct;
#[cfg(feature = "host")]
mod codex_direct;
#[cfg(feature = "host")]
pub(crate) mod mcp_launch;
#[cfg(feature = "host")]
pub(crate) mod skill_disk;
#[cfg(feature = "host")]
pub(crate) mod skill_library;
#[cfg(feature = "host")]
pub mod skills;

#[cfg(feature = "host")]
mod acp;
#[cfg(feature = "host")]
mod adapters;
#[cfg(feature = "host")]
pub(crate) mod alerts;
#[cfg(feature = "host")]
pub(crate) mod catalog;
#[cfg(feature = "host")]
pub(crate) mod checkout;
#[cfg(feature = "host")]
pub(crate) mod checkouts;
#[cfg(feature = "host")]
pub(crate) mod ci;
#[cfg(feature = "host")]
mod compaction;
#[cfg(feature = "host")]
mod conversation;
#[cfg(feature = "host")]
mod delete;
#[cfg(feature = "host")]
pub(crate) mod family;
#[cfg(feature = "host")]
mod flsh_config;
#[cfg(feature = "host")]
pub(crate) mod follow;
#[cfg(feature = "host")]
mod found_sessions;
#[cfg(feature = "host")]
mod harnesses;
#[cfg(feature = "host")]
mod image_files;
#[cfg(feature = "host")]
mod image_index;
#[cfg(feature = "host")]
mod images;
#[cfg(feature = "host")]
mod installs;
#[cfg(feature = "host")]
pub(crate) mod kinds;
#[cfg(feature = "host")]
mod launch;
#[cfg(feature = "host")]
mod limits;
#[cfg(feature = "host")]
pub(crate) mod listings;
#[cfg(feature = "host")]
pub(crate) mod live;
#[cfg(feature = "host")]
mod lsp;
#[cfg(feature = "host")]
pub(crate) mod machine_list;
#[cfg(feature = "host")]
mod made_files;
#[cfg(feature = "host")]
mod media_file;
#[cfg(feature = "host")]
mod models;
#[cfg(feature = "host")]
pub(crate) mod moving;
#[cfg(feature = "host")]
mod pipe;
#[cfg(feature = "host")]
mod plans;
#[cfg(feature = "host")]
mod presence_stream;
#[cfg(feature = "host")]
mod processes;
#[cfg(feature = "host")]
pub(crate) mod projects;
#[cfg(feature = "host")]
pub(crate) mod reach;
#[cfg(feature = "host")]
mod reopen;
#[cfg(feature = "host")]
pub(crate) mod reply;
#[cfg(feature = "host")]
mod requests;
#[cfg(feature = "host")]
mod restart;
#[cfg(feature = "host")]
pub(crate) mod reviews;
#[cfg(feature = "host")]
pub(crate) mod running;
#[cfg(feature = "host")]
pub mod search;
#[cfg(feature = "host")]
mod session;
#[cfg(feature = "host")]
mod session_stream;
#[cfg(feature = "host")]
pub(crate) mod sessions;
#[cfg(feature = "host")]
pub(crate) use crate::socket;
#[cfg(feature = "host")]
pub(crate) mod spend;
#[cfg(feature = "host")]
pub(crate) mod start;
#[cfg(feature = "host")]
mod tool_diff;
#[cfg(feature = "host")]
pub(crate) mod transcripts;
#[cfg(feature = "host")]
pub(crate) mod uploads;
#[cfg(feature = "host")]
mod usage;
#[cfg(feature = "host")]
pub use follow::serve_follow;
#[cfg(feature = "host")]
pub(crate) mod work_folder;
#[cfg(feature = "host")]
mod worktrees;

#[cfg(feature = "host")]
pub use work_folder::{local_hashes, local_read, local_tree};

#[cfg(feature = "host")]
pub use kinds::{AgentKind, Bridge, detect};
#[cfg(feature = "host")]
pub use launch::{AgentIn, AgentOut, Running, Started};
#[cfg(feature = "host")]
pub use start::start;

#[cfg(feature = "host")]
pub use moving::SESSIONS_ALPN;
#[cfg(feature = "host")]
pub use moving::SessionsAlpn;
#[cfg(feature = "host")]
pub(crate) use moving::follow as follow_moves;
#[cfg(feature = "host")]
pub(crate) use moving::project_folder;
#[cfg(feature = "host")]
pub use reopen::Reopen;
#[cfg(feature = "host")]
pub(crate) use reopen::{OpenSession, SavedSession};
#[cfg(feature = "host")]
pub use requests::serve_sessions;
#[cfg(feature = "host")]
pub(crate) use requests::{TRANSCRIPT_PROTOCOL, serve_transcripts, transcript_reply};
#[cfg(feature = "host")]
pub(crate) use search::follow as follow_search;
#[cfg(feature = "host")]
pub(crate) use session::{Answered, Asking};
#[cfg(feature = "host")]
pub use session_stream::serve_session;

#[cfg(feature = "host")]
pub(crate) use alerts::AlertHub;
#[cfg(feature = "host")]
use images::Images;
#[cfg(feature = "host")]
pub(crate) use processes::Demand as ProcessDemand;
#[cfg(feature = "host")]
pub(crate) use processes::watch_ports;
#[cfg(feature = "host")]
pub(crate) use session::Begin;
#[cfg(feature = "host")]
pub(crate) use session::Session;

#[cfg(feature = "host")]
pub(crate) use crate::logs::unix_now;

#[cfg(feature = "host")]
/// The agents this node offers and the sessions it runs. Sessions belong
/// to the daemon, so they outlive the streams and the connection.
pub struct Agents {
    /// Who works in which folder here, shared with sync.
    occupancy: Occupancy,
    pub events: crate::events::Hub,
    /// The agents it can start, how their adapters stand and the user's
    /// settings for them.
    pub catalog: Arc<catalog::Catalog>,
    /// What sessions spend and how much of the agents' plans is left.
    pub(crate) usage: Arc<usage::Usage>,
    pub(crate) sessions: Arc<sessions::Sessions>,
    /// What sessions did that the user would want to hear about.
    alerts: Arc<AlertHub>,
    /// What git says of the folders sessions and peers read.
    checkouts: Arc<checkouts::Checkouts>,
    /// Lazily spawned language servers for editor go-to-definition.
    lsp: lsp::Pool,
    /// The CI runs this node tells, and how it reaches the hosts.
    pub(crate) ci: Arc<ci::Ci>,
    /// How the moves of sessions to this machine stand.
    pub(crate) moves: Arc<moving::Moves>,
    /// What file requests may reach: the shared folders and the sessions'.
    pub(crate) reach: Arc<reach::Reach>,
    /// The sessions that started sessions, and the children on other machines.
    pub(crate) links: Arc<family::links::Links>,
    /// The reviews reviewers handed in, and the reviewers running.
    pub(crate) reviews: Arc<reviews::kept::Reviews>,
    /// The search index, once the mesh opened it.
    pub(crate) search: Arc<search::Opener>,
    /// The sessions kept across the last stop, and whether they were resumed.
    pub reopen: Arc<reopen::Reopen>,
    /// What the node has read of the folders it lists sessions in.
    pub listings: Arc<listings::Listings>,
    /// Deletes ended sessions and the background children that closed.
    pub(crate) delete: Arc<delete::Delete>,
    /// Who is whose child, and what the children tell their parents.
    pub(crate) family: Arc<family::Family>,
    /// Reads a session's transcript, live or found.
    pub(crate) transcripts: Arc<transcripts::Transcripts>,
    /// Starts sessions and puts each in its family.
    pub(crate) starter: Arc<running::Starter>,
    /// The watchers of the agents' stores, while clients follow them.
    pub(crate) bridge: Arc<follow::sources::EventBridge>,
}

#[cfg(feature = "host")]
/// What an `Agents` is made of that the node decides before it exists.
#[derive(Default)]
pub struct Parts {
    /// The agents it can start.
    pub kinds: Vec<AgentKind>,
    /// Who works in which folder here, shared with sync.
    pub occupancy: Occupancy,
    /// The mesh the node belongs to, once it does.
    pub mesh: crate::membership::Membership,
    /// How sessions start the Beep MCP server, when the node serves it.
    pub launch: Option<Arc<mcp_launch::Launch>>,
    /// Beep's own skills folder, when the node lists it for sessions.
    pub skills_dir: Option<PathBuf>,
    /// The workspace and the folders shared in `node.toml`.
    pub folders: Vec<PathBuf>,
    /// The search index's file, which the node opens when it joins a mesh.
    pub search: Option<PathBuf>,
}

#[cfg(feature = "host")]
impl Agents {
    pub fn new(parts: Parts) -> Self {
        let events = crate::events::Hub::default();
        let sessions = Arc::new(sessions::Sessions::default());
        let catalog = Arc::new(catalog::Catalog::new(
            parts.kinds,
            parts.launch,
            parts.skills_dir,
        ));
        let links = Arc::new(family::links::Links::new(parts.mesh));

        let reviews = Arc::new(reviews::kept::Reviews::new(
            events.clone(),
            sessions.clone(),
        ));
        let reopen = Arc::new(reopen::Reopen::new(sessions.clone(), links.clone()));
        let images = Arc::<Images>::default();
        let reach = Arc::new(reach::Reach::new(
            parts.folders,
            sessions.clone(),
            parts.occupancy.clone(),
        ));

        let listings = Arc::new(listings::Listings::new(
            sessions.clone(),
            parts.occupancy.clone(),
            images.clone(),
            events.clone(),
            reviews.clone(),
            reopen.clone(),
        ));

        let moves = Arc::new(moving::Moves::new(
            sessions.clone(),
            reopen.clone(),
            listings.clone(),
        ));

        let bridge = Arc::new(follow::sources::EventBridge::new(
            events.clone(),
            parts.occupancy.clone(),
            listings.clone(),
            reach.clone(),
            sessions.clone(),
        ));

        let search = Arc::new(search::Opener::new(
            parts.search,
            listings.clone(),
            sessions.clone(),
            bridge.clone(),
            events.clone(),
        ));

        let delete = Arc::new(delete::Delete::new(
            sessions.clone(),
            reopen.clone(),
            listings.clone(),
            parts.occupancy.clone(),
            search.clone(),
        ));
        let family = Arc::new(family::Family::new(
            sessions.clone(),
            links.clone(),
            delete.clone(),
        ));

        let transcripts = Arc::new(transcripts::Transcripts::new(
            sessions.clone(),
            listings.clone(),
            family.clone(),
        ));

        let usage = Arc::new(usage::Usage::new(catalog.clone()));
        let alerts = Arc::<AlertHub>::default();
        let checkouts = Arc::<checkouts::Checkouts>::default();
        let starter = Arc::new(running::Starter::new(
            catalog.clone(),
            sessions.clone(),
            links.clone(),
            family.clone(),
            parts.occupancy.clone(),
            usage.clone(),
            images.clone(),
            alerts.clone(),
            checkouts.clone(),
            events.clone(),
            reopen.clone(),
            listings.clone(),
            reviews.clone(),
            delete.clone(),
        ));

        Self {
            bridge,
            delete,
            family,
            transcripts,
            starter,
            listings,
            events: events.clone(),
            usage,
            occupancy: parts.occupancy.clone(),
            reviews,
            reopen,
            links,
            reach,
            sessions,
            catalog,
            search,
            alerts,
            checkouts,
            lsp: lsp::Pool::default(),
            ci: Arc::default(),
            moves,
        }
    }

    /// While the node runs: checks installed harness releases once an hour
    /// and deletes background children as they close.
    pub fn start_upkeep(self: &Arc<Self>) -> tokio_util::task::AbortOnDropHandle<()> {
        let (catalog, delete) = (self.catalog.clone(), self.delete.clone());
        tokio_util::task::AbortOnDropHandle::new(tokio::spawn(async move {
            let harness_checks = async {
                let mut interval = tokio::time::interval(Duration::from_secs(60 * 60));
                interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

                loop {
                    interval.tick().await;
                    harnesses::refresh_versions(&catalog.kinds(), &catalog.latest).await;
                }
            };
            let background_children = async {
                if let Some(closed) = delete.take_closed() {
                    delete.delete_closed_background(closed).await;
                }
            };
            tokio::join!(harness_checks, background_children);
        }))
    }

    /// Closes every running session and waits until their agents have
    /// stopped: for the node shutting down.
    pub async fn shutdown(&self) {
        self.sessions.begin_stopping();
        let running: Vec<Arc<Session>> = self
            .sessions
            .read()
            .values()
            .filter(|session| watched::read(&session.live, |live| live.status() != Status::Ended))
            .cloned()
            .collect();
        futures::future::join_all(running.iter().map(|session| session.close())).await;
        self.lsp.shutdown().await;
    }
}

#[cfg(feature = "host")]
impl Default for Agents {
    /// An `Agents` with nothing configured, as a test or a bare node has.
    fn default() -> Self {
        Self::new(Parts::default())
    }
}

#[cfg(feature = "host")]
#[cfg(test)]
#[path = "../tests/agents.rs"]
mod tests;
