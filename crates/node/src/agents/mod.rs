//! Coding agents (Claude Code, Codex, Gemini CLI, pi, flsh, OpenCode) run
//! over ACP (`Agents`) and the streams the server follows them on; sessions
//! started outside Beep (`claude`, `found`) and the Beep MCP server (`mcp`).

use crate::{
    AsyncMutex,
    agents::claude::External,
    agents::found::Stores,
    agents::mcp::{Launch, Upstream},
};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex, OnceLock, atomic::AtomicBool},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::{sync::watch, time::Instant};

pub mod claude;
pub(crate) mod found;
pub mod mcp;

mod acp;
mod alerts;
mod checkout;
mod compaction;
mod custom_agents;
mod flsh_config;
mod found_sessions;
mod harnesses;
mod holders;
mod image_files;
mod image_index;
mod images;
mod kinds;
mod live;
mod media_file;
mod models;
#[allow(dead_code)]
mod moving;
mod place;
mod plans;
mod prices;
mod reach;
mod requests;
mod restart;
mod running;
mod session;
mod session_stream;
mod spend;
mod subagent;
mod tool_diff;
mod usage;
mod work_diff;
mod work_folder;
mod worktrees;

pub use work_folder::{local_read, local_tree};

pub(crate) use acp::{Line, LineReader};
pub(crate) use images::{Picture, block_picture};
pub use kinds::{AgentKind, detect};
pub(crate) use live::cut;
pub use requests::serve_sessions;
pub(crate) use restart::OpenSession;
pub(crate) use session::{Answered, Asking};
pub use session_stream::serve_session;
pub(crate) use subagent::{Run as SubagentRun, RunStep};

use acp::Prompt;
use alerts::AlertHub;
use compaction::Route;
use images::Images;
use kinds::{Adapter, KNOWN, known, route};
use models::ModelCache;
use running::{Begin, folder};
use session::{Command, NodeMode, Session, Start};
use spend::Spend;
use usage::{Plans, UsageHub};

/// The most sessions running at once.
const MAX_RUNNING: usize = 8;
/// The most sessions kept, ended ones included; the oldest ended go first.
const MAX_KEPT: usize = 64;
/// The most transcript entries one request reads.
const MAX_TRANSCRIPT: u32 = 200;
/// The most folders `Folders` answers, and how many when the server names
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
/// How often the plans' usage is read while the server follows it, and
/// the least time between two reads it asks for.
const PLAN_EVERY: Duration = Duration::from_secs(5 * 60);
const PLAN_REFRESH_GAP: Duration = Duration::from_secs(30);
/// How often the agents' logs are read for what sessions spent, while the
/// server follows it.
const SPEND_EVERY: Duration = Duration::from_secs(60);
/// How long a new session's stream waits for the agent's own id before it
/// tells the server about the session, under the server's wait.
const AGENT_ID_WAIT: Duration = Duration::from_secs(6);
/// How long the folders found in the agents' logs are kept.
const SIGHTED_FOR: Duration = Duration::from_secs(60);
/// How long a file request may wait for a look in the agents' logs.
const SIGHTING_WAIT: Duration = Duration::from_secs(5);

/// Now, in Unix seconds.
fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

/// The agents this node offers and the sessions it runs. Sessions belong
/// to the daemon, so they outlive the streams and the connection.
#[derive(Default)]
pub struct Agents {
    kinds: Vec<AgentKind>,
    sessions: Mutex<HashMap<String, Arc<Session>>>,
    /// Claude Code sessions started outside Beep.
    external: External,
    /// Where Codex, pi, flsh and OpenCode keep their sessions.
    stores: Stores,
    /// The pictures sessions' transcripts show, by session.
    images: Arc<Images>,
    /// What sessions spend and how much of the agents' plans is left.
    usage: Arc<UsageHub>,
    /// What sessions did that the user would want to hear about.
    alerts: Arc<AlertHub>,
    /// What sessions spent, from the agents' own logs.
    spend: Arc<Spend>,
    /// The models each agent offers, found lately.
    models: ModelCache,
    /// The user's settings for each agent, applied when one starts.
    settings: harnesses::Settings,
    /// The newest versions of the agents, found lately.
    latest: harnesses::Latest,
    /// The workspace and the folders shared in `node.toml`, which file
    /// requests may reach beside the sessions' folders.
    folders: OnceLock<Vec<PathBuf>>,
    /// How sessions start the Beep MCP server, once the node serves it.
    mcp: OnceLock<(Arc<Launch>, Upstream)>,
    /// Set once the node is stopping.
    stopping: Arc<AtomicBool>,
    /// True while an update waits for the sessions to be idle: prompts
    /// wait instead of starting turns.
    holding: watch::Sender<bool>,
    /// Set once the server sent the user's agent settings, which resumed
    /// sessions start with.
    settled: watch::Sender<bool>,
    /// The folders sessions started outside Beep worked in, from the
    /// agents' logs, with when they were read; one read at a time.
    sighted: AsyncMutex<Option<Sighted>>,
    /// The checkouts found on disk under home, once `warm` read them;
    /// held while it does.
    on_disk: AsyncMutex<Option<Arc<Vec<found::Checkout>>>>,
}

/// The folders found in the agents' logs, and when.
struct Sighted {
    at: Instant,
    folders: Arc<Vec<PathBuf>>,
}

impl Agents {
    pub fn new(kinds: Vec<AgentKind>) -> Self {
        Self {
            kinds,
            sessions: Mutex::default(),
            external: External::locate(),
            stores: Stores::locate(),
            images: Arc::default(),
            usage: Arc::default(),
            alerts: Arc::default(),
            spend: Arc::new(Spend::locate()),
            models: ModelCache::default(),
            settings: harnesses::Settings::default(),
            latest: harnesses::Latest::default(),
            folders: OnceLock::new(),
            mcp: OnceLock::new(),
            stopping: Arc::default(),
            holding: watch::Sender::new(false),
            settled: watch::Sender::new(false),
            sighted: AsyncMutex::default(),
            on_disk: AsyncMutex::default(),
        }
    }
}

#[cfg(test)]
mod tests;
