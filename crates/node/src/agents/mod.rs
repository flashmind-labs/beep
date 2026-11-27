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
    use crate::{AsyncMutex, occupancy::Occupancy};
    use crate::logs::found;
    use std::{
        collections::HashMap,
        path::PathBuf,
        sync::{Arc, Mutex, OnceLock, atomic::AtomicBool},
    };
    use tokio::{sync::watch, time::Instant};

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
    pub(crate) use restart::{OpenSession, SavedSession};
    pub(crate) use session::{Answered, Asking};
    pub use session_stream::serve_session;

    pub(crate) use alerts::AlertHub;
    pub(crate) use processes::Demand as ProcessDemand;
    pub(crate) use processes::watch_ports;
    use images::Images;
    pub(crate) use session::Session;
    pub(crate) use session::Begin;
    use spend::Spend;
    use usage::UsageHub;

    pub(crate) use crate::logs::unix_now;

    /// The agents this node offers and the sessions it runs. Sessions belong
    /// to the daemon, so they outlive the streams and the connection.
    #[derive(Default)]
    pub struct Agents {
        /// Who works in which folder here, shared with sync.
        occupancy: Occupancy,
        pub events: crate::events::Hub,
        sources: Mutex<std::sync::Weak<follow::Sources>>,
        event_forward: OnceLock<tokio_util::task::AbortOnDropHandle<()>>,
        /// The agents it can start, how their adapters stand and the user's
        /// settings for them.
        pub(crate) catalog: Arc<catalog::Catalog>,
        pub(crate) sessions: sessions::Sessions,
        /// The pictures sessions' transcripts show, by session.
        images: Arc<Images>,
        /// What sessions spend and how much of the agents' plans is left.
        usage: Arc<UsageHub>,
        /// What sessions did that the user would want to hear about.
        alerts: Arc<AlertHub>,
        /// What git says of the folders sessions and peers read.
        checkouts: Arc<checkouts::Checkouts>,
        /// Lazily spawned language servers for editor go-to-definition.
        lsp: lsp::Pool,
        /// The CI runs this node tells, and how it reaches the hosts.
        pub(crate) ci: Arc<ci::Ci>,
        /// What sessions spent, from the agents' own logs.
        spend: Arc<Spend>,
        /// Set while the agents' plans are read.
        reading_plans: AtomicBool,
        /// When the last plan read started, so short-lived streams do not re-run it before `PLAN_EVERY`.
        last_plan_read: Mutex<Option<Instant>>,
        /// How the moves of sessions to this machine stand.
        moves: moving::Moves,
        /// The workspace and the folders shared in `node.toml`, which file
        /// requests may reach beside the sessions' folders.
        folders: OnceLock<Vec<PathBuf>>,
        /// The sessions that started sessions, and the children on other machines.
        pub(crate) links: Arc<family::links::Links>,
        /// The background children that closed for good, to delete.
        closed: family::links::Tidy,
        /// The reviews reviewers handed in, and the reviewers running.
        reviews: reviews::Reviews,
        /// The search index, once the mesh opened it.
        search: OnceLock<Arc<search::SearchIndex>>,
        /// Set once the sessions open before the last stop have been resumed, or
        /// there were none.
        reopened: watch::Sender<bool>,
        /// The checkouts found on disk under home, once `warm` read them.
        /// Written after a walk, by the one walk running.
        on_disk: Mutex<Option<Arc<Vec<found::Checkout>>>>,
        /// Held while a walk for checkouts runs, deep or quick; one at a
        /// time. `warm` waits for it, a rescan does not.
        rescanning: AsyncMutex<()>,
        /// The last folder listing for each limit; held while one runs, so
        /// asks for that limit that come meanwhile share it instead of each
        /// running git for every folder.
        listed: Mutex<HashMap<usize, Arc<AsyncMutex<Option<found_sessions::Listed>>>>>,
        /// What sessions were, ended ones too, from the restart note.
        saved: Mutex<Vec<restart::SavedSession>>,
    }

    impl Agents {
        pub fn new(kinds: Vec<AgentKind>, occupancy: Occupancy) -> Self {
            let events = crate::events::Hub::default();

            Self {
                events: events.clone(),
                catalog: Arc::new(catalog::Catalog::new(kinds)),
                occupancy,
                spend: Arc::new(Spend::locate()),
                reviews: reviews::Reviews::new(events),
                ..Self::default()
            }
        }
    }

    #[cfg(test)]
    #[path = "../tests/agents.rs"]
    mod tests;
}
