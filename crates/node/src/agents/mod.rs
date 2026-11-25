//! Coding agents (Claude Code, Codex, Antigravity, pi, flsh, OpenCode) run
//! over ACP (`Agents`) and the streams the caller follows them on; sessions
//! started outside Beep (`claude`, `found`) and the Beep MCP server (`mcp`).

pub mod tags;
mod work_diff;

pub use work_diff::parse_git_patch;

host! {
    mod brief;
    mod text;
    pub(crate) use text::cut;
    use crate::{AsyncMutex, agents::claude::External, agents::found::Stores, agents::mcp::Launch};
    use beep_proto::SessionId;
    use std::{
        collections::HashMap,
        path::PathBuf,
        sync::{Arc, Mutex, OnceLock, atomic::AtomicBool},
        time::{Duration, SystemTime, UNIX_EPOCH},
    };
    use tokio::{sync::watch, time::Instant};

    pub mod claude;
    mod claude_direct;
    mod codex_direct;
    pub(crate) mod found;
    pub mod mcp;
    pub(crate) mod skill_library;
    pub mod skills;

    pub(crate) mod follow;
    mod acp;
    mod adapters;
    mod alerts;
    mod checkout;
    pub(crate) mod checkouts;
    mod compaction;
    mod conversation;
    pub(crate) mod custom_agents;
    mod delete;
    mod family;
    mod flsh_config;
    mod found_sessions;
    mod harnesses;
    mod holders;
    mod image_files;
    mod image_index;
    mod images;
    mod kinds;
    mod launch;
    mod live;
    pub(crate) mod locate;
    mod lsp;
    mod made_files;
    mod media_file;
    mod models;
    mod moving;
    mod pipe;
    mod place;
    mod plans;
    pub mod presence;
    mod prices;
    mod processes;
    pub(crate) mod reach;
    mod repository_recovery;
    pub mod search;
    mod tool_details;
    mod requests;
    mod restart;
    mod reviews;
    mod running;
    mod session;
    mod session_stream;
    mod sessions;
    mod spend;
    mod subagent;
    mod tool_diff;
    pub(crate) mod uploads;
    mod usage;
    mod window;
    pub use follow::serve_follow;
    mod work_folder;
    mod worktrees;

    pub use work_folder::{local_hashes, local_read, local_tree};

    pub(crate) use acp::{Line, LineReader, MAX_MESSAGE};
    pub(crate) use images::{Picture, block_picture};
    pub use kinds::{AgentKind, Bridge, detect};
    pub use launch::{AgentIn, AgentOut, Running, Started, start};
    pub(crate) use kinds::adapter_bins;
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
    pub(crate) use subagent::{Run as SubagentRun, RunStep};

    use acp::Prompt;
    pub(crate) use alerts::AlertHub;
    pub(crate) use processes::Demand as ProcessDemand;
    pub(crate) use processes::watch_ports;
    use compaction::Route;
    use images::Images;
    use kinds::{Adapter, KNOWN, known, route};
    use models::ModelCache;
    use running::{Begin, folder};
    use session::{Command, Session, Start};
    use spend::Spend;
    use usage::UsageHub;

    /// The most sessions kept, ended ones included; the oldest ended go first.
    const MAX_KEPT: usize = 64;
    /// The most transcript entries one request reads.
    const MAX_TRANSCRIPT: u32 = 200;
    /// The most folders `Folders` answers, and how many when the caller names
    /// no number.
    const MAX_FOLDERS: usize = 800;
    const DEFAULT_FOLDERS: usize = 100;
    /// The most sessions `InFolders` answers, and how many by default.
    const MAX_IN_FOLDERS: usize = 100;
    const DEFAULT_IN_FOLDERS: usize = 50;
    /// The most folders one `InFolders` names.
    const MAX_ASKED_FOLDERS: usize = 32;
    /// The largest picture `Image` answers, under the link's frame limit.
    const MAX_IMAGE: usize = 7 * 1024 * 1024;
    /// How often the plans' usage is read while the caller follows it, and
    /// the least time between two reads it asks for.
    const PLAN_EVERY: Duration = Duration::from_secs(5 * 60);
    const PLAN_REFRESH_GAP: Duration = Duration::from_secs(30);
    /// The least time between two reads of the agents' logs for what sessions
    /// spent: a busy agent changes its log on every line.
    const SPEND_GAP: Duration = Duration::from_secs(10);
    /// How long a file request may wait for a look in the agents' logs.
    const SIGHTING_WAIT: Duration = Duration::from_secs(5);

    /// Now, in Unix seconds.
    fn unix_now() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_secs())
    }

    /// Records how long `stage` of a transcript read took since `started`:
    /// finding the file, reading, parsing, subagents, encoding or sending.
    fn transcript_stage(stage: &'static str, started: std::time::Instant) {
        metrics::histogram!("beep_transcript_stage_seconds", "stage" => stage)
            .record(started.elapsed().as_secs_f64());
    }

    /// The agents this node offers and the sessions it runs. Sessions belong
    /// to the daemon, so they outlive the streams and the connection.
    #[derive(Default)]
    pub struct Agents {
        /// Checkouts temporarily held for local Git recovery.
        recovering: Mutex<std::collections::HashSet<PathBuf>>,
        pub events: crate::events::Hub,
        pub(crate) event_sources: crate::link::EventSources,
        event_watcher: Mutex<std::sync::Weak<follow::Sources>>,
        event_forward: OnceLock<tokio_util::task::AbortOnDropHandle<()>>,
        /// The agents it can start, replaced when detection finds others.
        kinds: watch::Sender<Arc<Vec<AgentKind>>>,
        /// The adapters the node installs itself.
        adapters: adapters::Installs,
        sessions: sessions::Sessions,
        /// Serializes changes to a stored session against resuming it, one
        /// lock for each session by the agent's own id; see `store_of`.
        session_store: Mutex<HashMap<SessionId, std::sync::Weak<AsyncMutex<()>>>>,
        /// Claude Code sessions started outside Beep.
        external: External,
        /// Sessions held open by a local CLI, including idle prompts.
        presence: presence::Registry,
        /// Where Codex, pi, flsh and OpenCode keep their sessions.
        stores: Stores,
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
        pub(crate) ci: Arc<crate::machine::ci::Ci>,
        /// What sessions spent, from the agents' own logs.
        spend: Arc<Spend>,
        /// Set while the agents' plans are read.
        reading_plans: AtomicBool,
        /// How the moves of sessions to this machine stand.
        moves: moving::Moves,
        /// The models each agent offers, found lately.
        models: ModelCache,
        /// The user's settings for each agent, applied when one starts.
        settings: harnesses::Settings,
        /// Saved Beep instructions, supplied to sessions without editing agent files.
        instructions: Mutex<Option<String>>,
        /// The newest versions of the agents, found lately.
        latest: harnesses::Latest,
        /// The workspace and the folders shared in `node.toml`, which file
        /// requests may reach beside the sessions' folders.
        folders: OnceLock<Vec<PathBuf>>,
        /// The sessions that started sessions, and the children on other machines.
        family: family::Family,
        /// The reviews reviewers handed in, and the reviewers running.
        reviews: reviews::Reviews,
        /// How sessions start the Beep MCP server, once the node serves it.
        mcp: OnceLock<Arc<Launch>>,
        /// Beep's own skills folder, listed for sessions once the node
        /// serves: one made without it, as in tests, lists none.
        skills_dir: OnceLock<PathBuf>,
        /// The search index, once the mesh opened it.
        search: OnceLock<Arc<search::SearchIndex>>,
        /// Set once the node is stopping.
        stopping: Arc<AtomicBool>,
        /// Set once the mesh gave the user's agent settings, which resumed
        /// sessions start with.
        settled: watch::Sender<bool>,
        /// Set once the sessions open before the last stop have been resumed, or
        /// there were none.
        reopened: watch::Sender<bool>,
        /// The read of every store now running, shared by whoever asks while
        /// it does. Held only to join or start one, never across a read.
        sighting: Mutex<std::sync::Weak<ReadInFlight>>,
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
        /// Sessions started outside Beep whose process this node ended or
        /// saw exit lately, by id, with when.
        closed: Arc<Mutex<HashMap<SessionId, Instant>>>,
        /// Which processes hold the sessions' files, checked once for all.
        holders: holders::Holders,
        /// What sessions were, ended ones too, from the restart note.
        saved: Mutex<Vec<restart::SavedSession>>,
    }

    /// The outcome of one read of every store, set once by the caller that runs it.
    type ReadInFlight = tokio::sync::OnceCell<Result<Sighted, Arc<anyhow::Error>>>;

    /// What the agents' logs hold: the sessions, and the folders they ran in.
    #[derive(Clone)]
    struct Sighted {
        seen: Arc<Vec<found::Seen>>,
        folders: Arc<Vec<PathBuf>>,
    }

    impl Sighted {
        /// `seen`, with the folders it names.
        fn of(seen: Vec<found::Seen>) -> Self {
            let mut folders: Vec<PathBuf> = seen.iter().map(|seen| seen.cwd.clone()).collect();
            folders.sort();
            folders.dedup();

            Self {
                seen: Arc::new(seen),
                folders: Arc::new(folders),
            }
        }
    }

    impl Agents {
        pub fn new(kinds: Vec<AgentKind>) -> Self {
            let events = crate::events::Hub::default();

            Self {
                events: events.clone(),
                kinds: watch::Sender::new(Arc::new(kinds)),
                external: External::locate(),
                stores: Stores::locate(),
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
