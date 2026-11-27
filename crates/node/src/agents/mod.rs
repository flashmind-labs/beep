//! Coding agents (Claude Code, Codex, Antigravity, pi, flsh, OpenCode) run
//! over ACP (`Agents`) and the streams the caller follows them on; sessions
//! started outside Beep (`claude`, `found`).

pub use crate::tags;
mod work_diff;

pub use work_diff::parse_git_patch;

host! {
    pub(crate) mod brief;
    pub(crate) mod child_reach;
    pub(crate) mod child_view;
    pub(crate) use crate::calls;
    use crate::occupancy::Occupancy;
    use std::{
        path::PathBuf,
        sync::{Arc, Mutex},
    };

    pub mod claude;
    mod claude_direct;
    mod codex_direct;
    pub(crate) mod mcp_launch;
    pub(crate) mod skill_disk;
    pub(crate) mod skill_library;
    pub mod skills;

    pub(crate) mod follow;
    mod acp;
    mod adapters;
    pub(crate) mod alerts;
    pub(crate) mod ci;
    pub(crate) mod catalog;
    pub(crate) mod checkout;
    pub(crate) mod checkouts;
    mod compaction;
    mod conversation;
    mod delete;
    pub(crate) mod family;
    mod flsh_config;
    mod found_sessions;
    mod harnesses;
    mod installs;
    mod image_files;
    mod image_index;
    mod images;
    pub(crate) mod kinds;
    mod launch;
    mod listings;
    mod limits;
    pub(crate) mod live;
    pub(crate) mod machine_list;
    pub(crate) mod projects;
    mod lsp;
    mod made_files;
    mod media_file;
    mod models;
    pub(crate) mod moving;
    mod pipe;
    pub(crate) mod reply;
    mod plans;
    mod presence_stream;
    mod processes;
    pub(crate) mod reach;
    pub mod search;
    mod requests;
    mod reopen;
    mod restart;
    pub(crate) mod reviews;
    mod running;
    mod session;
    mod session_stream;
    mod sessions;
    pub(crate) use crate::socket;
    pub(crate) mod spend;
    pub(crate) mod start;
    mod tool_diff;
    pub(crate) mod uploads;
    mod usage;
    pub use follow::serve_follow;
    pub(crate) mod work_folder;
    mod worktrees;

    pub use work_folder::{local_hashes, local_read, local_tree};


    pub use kinds::{AgentKind, Bridge, detect};
    pub use launch::{AgentIn, AgentOut, Running, Started};
    pub use start::start;

    pub use moving::SESSIONS_ALPN;
    pub use moving::SessionsAlpn;
    pub(crate) use moving::follow as follow_moves;
    pub(crate) use search::follow as follow_search;
    pub(crate) use moving::project_folder;
    pub use requests::serve_sessions;
    pub(crate) use requests::{TRANSCRIPT_PROTOCOL, serve_transcripts};
    pub use reopen::Reopen;
    pub(crate) use reopen::{OpenSession, SavedSession};
    pub(crate) use session::{Answered, Asking};
    pub use session_stream::serve_session;

    pub(crate) use alerts::AlertHub;
    pub(crate) use processes::Demand as ProcessDemand;
    pub(crate) use processes::watch_ports;
    use images::Images;
    pub(crate) use session::Session;
    pub(crate) use session::Begin;

    pub(crate) use crate::logs::unix_now;

    /// The agents this node offers and the sessions it runs. Sessions belong
    /// to the daemon, so they outlive the streams and the connection.
    pub struct Agents {
        /// Who works in which folder here, shared with sync.
        occupancy: Occupancy,
        pub events: crate::events::Hub,
        sources: Mutex<std::sync::Weak<follow::observers::Sources>>,
        /// The agents it can start, how their adapters stand and the user's
        /// settings for them.
        pub catalog: Arc<catalog::Catalog>,
        /// What sessions spend and how much of the agents' plans is left.
        pub(crate) usage: Arc<usage::Usage>,
        pub(crate) sessions: Arc<sessions::Sessions>,
        /// The pictures sessions' transcripts show, by session.
        images: Arc<Images>,
        /// What sessions did that the user would want to hear about.
        alerts: Arc<AlertHub>,
        /// What git says of the folders sessions and peers read.
        checkouts: Arc<checkouts::Checkouts>,
        /// Lazily spawned language servers for editor go-to-definition.
        lsp: lsp::Pool,
        /// The CI runs this node tells, and how it reaches the hosts.
        pub(crate) ci: Arc<ci::Ci>,
        /// How the moves of sessions to this machine stand.
        moves: moving::Moves,
        /// What file requests may reach: the shared folders and the sessions'.
        pub(crate) reach: Arc<reach::Reach>,
        /// The sessions that started sessions, and the children on other machines.
        pub(crate) links: Arc<family::links::Links>,
        /// The background children that closed for good, to delete.
        closed: family::links::Tidy,
        /// The reviews reviewers handed in, and the reviewers running.
        pub(crate) reviews: Arc<reviews::kept::Reviews>,
        /// The search index, once the mesh opened it.
        pub(crate) search: search::Opener,
        /// The sessions kept across the last stop, and whether they were resumed.
        pub reopen: Arc<reopen::Reopen>,
        /// What the node has read of the folders it lists sessions in.
        listings: listings::Listings,
    }

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

    impl Agents {
        pub fn new(parts: Parts) -> Self {
            let events = crate::events::Hub::default();
            let sessions = Arc::new(sessions::Sessions::default());
            let catalog = Arc::new(catalog::Catalog::new(parts.kinds, parts.launch, parts.skills_dir));
            let links = Arc::new(family::links::Links::new(parts.mesh));

            Self {
                events: events.clone(),
                usage: Arc::new(usage::Usage::new(catalog.clone())),
                occupancy: parts.occupancy.clone(),
                reviews: Arc::new(reviews::kept::Reviews::new(events, sessions.clone())),
                reopen: Arc::new(reopen::Reopen::new(sessions.clone(), links.clone())),
                links,
                reach: Arc::new(reach::Reach::new(parts.folders, sessions.clone(), parts.occupancy.clone())),
                sessions,
                catalog,
                search: search::Opener::new(parts.search),
                sources: Mutex::default(),
                images: Arc::default(),
                alerts: Arc::default(),
                checkouts: Arc::default(),
                lsp: lsp::Pool::default(),
                ci: Arc::default(),
                moves: moving::Moves::default(),
                closed: family::links::Tidy::default(),
                listings: listings::Listings::default(),
            }
        }
    }

    impl Default for Agents {
        /// An `Agents` with nothing configured, as a test or a bare node has.
        fn default() -> Self {
            Self::new(Parts::default())
        }
    }

    #[cfg(test)]
    #[path = "../tests/agents.rs"]
    mod tests;
}
