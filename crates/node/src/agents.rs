//! Coding agents (Claude Code, Codex, Gemini CLI, pi, flsh, OpenCode) run over ACP: the
//! table of agents this node can start, the sessions it runs, and the
//! streams the server follows them on (`beep_proto::node::agent`).

use crate::{
    claude::{self, External},
    found::{self, Agent, Stores},
    mcp::Launch,
    process,
    provision::spawn_env::{self, Scope},
};
use anyhow::{Context, Result};
use beep_proto::node::{
    LinkRx, LinkTx,
    agent::{
        self, CheckoutPlan, Event, ExternalSession, ModelList, Request, SESSIONS_PROTOCOL,
        SessionFolder, SessionInfo, SessionsReply, SessionsRequest, State, Status,
    },
    harness::{AgentSettings, HarnessReport},
    usage::{PlanUsage, SessionSpend},
};
use beep_proto::{AgentAccount, ModelId};
use futures::future::BoxFuture;
use serde::{Deserialize, Serialize};
use std::{
    cmp::Reverse,
    collections::{BTreeMap, HashMap, HashSet, hash_map::RandomState},
    env,
    fmt::Display,
    hash::BuildHasher,
    path::{Component, Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::{
    sync::oneshot,
    task::{JoinHandle, spawn_blocking},
    time::{Instant, MissedTickBehavior, interval},
};

mod acp;
mod alerts;
mod checkout;
mod compaction;
mod custom_agents;
mod harnesses;
mod image_files;
mod images;
mod live;
mod media_file;
mod models;
mod plans;
mod prices;
mod reach;
mod session;
mod spend;
mod subagent;
mod usage;
mod work_folder;

pub(crate) use acp::{Line, LineReader};
pub(crate) use images::{Picture, block_picture};
pub(crate) use live::cut;
pub(crate) use session::{Answered, Asking};
pub(crate) use subagent::{Run as SubagentRun, RunStep};

use acp::Prompt;
use alerts::AlertHub;
use compaction::Route;
use images::Images;
use models::ModelCache;
use session::{Command, Session, Start};
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
const MAX_FOLDERS: usize = 200;
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

/// An agent this node can start: a program that speaks ACP on its stdio.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentKind {
    /// What Beep calls it, such as `claude`; its protocol is `claude/1`.
    pub name: String,
    pub program: String,
    #[serde(default)]
    pub args: Vec<String>,
    /// Variables set for it on top of the node's own environment.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
}

/// Shows the names of its variables, never their values, which may be the
/// user's secrets.
impl std::fmt::Debug for AgentKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgentKind")
            .field("name", &self.name)
            .field("program", &self.program)
            .field("args", &self.args)
            .field("env", &self.env.keys().collect::<Vec<_>>())
            .finish()
    }
}

/// An agent the daemon knows how to start, when its CLI is installed.
/// Each starts with its own permission prompts off, so it never stops to
/// ask; a permission request that comes anyway is allowed by the session.
struct Known {
    name: &'static str,
    /// What people call it, such as Claude Code.
    title: &'static str,
    /// The CLI whose presence says the user has this agent.
    cli: &'static str,
    adapter: Adapter,
    /// Variables that turn its permission prompts off.
    env: &'static [(&'static str, &'static str)],
    /// The session mode that asks no permission, for an agent whose
    /// launch cannot set it, set once the session exists.
    bypass_mode: Option<&'static str>,
    /// It starts on a model given as `--model` on its command line; the
    /// others switch over ACP once the session exists.
    model_flag: bool,
    /// How it is asked to compact.
    compact: Route,
}

/// Now, in Unix seconds.
fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

/// What the node knows of the agent `name`, when it is a known one.
fn known(name: &str) -> Option<&'static Known> {
    KNOWN.iter().find(|known| known.name == name)
}

enum Adapter {
    /// An ACP adapter from npm: its binary when installed, or run with npx.
    Npm {
        bin: &'static str,
        package: &'static str,
    },
    /// The CLI itself speaks ACP with these arguments.
    Flags(&'static [&'static str]),
}

const KNOWN: &[Known] = &[
    // The session switches it to `bypassPermissions` once it has one.
    Known {
        name: "claude",
        title: "Claude Code",
        cli: "claude",
        adapter: Adapter::Npm {
            bin: "claude-agent-acp",
            package: "@agentclientprotocol/claude-agent-acp",
        },
        env: &[],
        bypass_mode: Some("bypassPermissions"),
        model_flag: false,
        compact: Route::Advertised,
    },
    // Full access: approval policy `never` and sandbox `danger-full-access`,
    // for new and resumed sessions alike.
    Known {
        name: "codex",
        title: "Codex",
        cli: "codex",
        adapter: Adapter::Npm {
            bin: "codex-acp",
            package: "@agentclientprotocol/codex-acp",
        },
        env: &[("INITIAL_AGENT_MODE", "agent-full-access")],
        bypass_mode: None,
        model_flag: false,
        compact: Route::Advertised,
    },
    // YOLO approves every tool. Gemini CLI drops back to `default` in a
    // folder it does not trust, so the folder is trusted too.
    Known {
        name: "gemini",
        title: "Gemini CLI",
        cli: "gemini",
        adapter: Adapter::Flags(&["--acp", "--approval-mode", "yolo"]),
        env: &[("GEMINI_CLI_TRUST_WORKSPACE", "true")],
        bypass_mode: None,
        model_flag: true,
        compact: Route::OwnTrigger,
    },
    // pi has no permission prompts; only its extensions ask.
    Known {
        name: "pi",
        title: "pi",
        cli: "pi",
        adapter: Adapter::Npm {
            bin: "pi-acp",
            package: "pi-acp",
        },
        env: &[],
        bypass_mode: None,
        model_flag: false,
        compact: Route::Advertised,
    },
    // flsh never asks permission.
    Known {
        name: "flsh",
        title: "flsh",
        cli: "flsh",
        adapter: Adapter::Flags(&["acp"]),
        env: &[],
        bypass_mode: None,
        model_flag: true,
        compact: Route::OwnTrigger,
    },
    // Merged over its config as the last rule, which wins over the defaults
    // that ask (`doom_loop`, `external_directory`, reading `.env` files).
    Known {
        name: "opencode",
        title: "OpenCode",
        cli: "opencode",
        adapter: Adapter::Flags(&["acp"]),
        env: &[("OPENCODE_PERMISSION", r#"{"*":"allow"}"#)],
        bypass_mode: None,
        model_flag: false,
        compact: Route::Always,
    },
];

/// The agents installed here, from `KNOWN`, then `extra` from `node.toml`
/// (which replace a known one of the same name).
pub fn detect(extra: &[AgentKind]) -> Vec<AgentKind> {
    let found = |program: &str| which::which(program).is_ok();
    let mut kinds: Vec<AgentKind> = KNOWN
        .iter()
        .filter_map(|known| launch(known, &found))
        .collect();
    for kind in extra {
        kinds.retain(|known| known.name != kind.name);
        kinds.push(kind.clone());
    }
    kinds
}

/// How the node starts `known`, when `found` says its CLI, and the adapter
/// or npx it needs, are installed.
fn launch(known: &Known, found: &dyn Fn(&str) -> bool) -> Option<AgentKind> {
    if !found(known.cli) {
        return None;
    }
    let (program, args) = match &known.adapter {
        Adapter::Npm { bin, .. } if found(bin) => ((*bin).to_owned(), Vec::new()),
        Adapter::Npm { package, .. } if found("npx") => (
            "npx".to_owned(),
            vec!["-y".to_owned(), (*package).to_owned()],
        ),
        Adapter::Npm { .. } => return None,
        Adapter::Flags(flags) => (
            known.cli.to_owned(),
            flags.iter().map(|flag| (*flag).to_owned()).collect(),
        ),
    };
    let env = known
        .env
        .iter()
        .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
        .collect();
    Some(AgentKind {
        name: known.name.to_owned(),
        program,
        args,
        env,
    })
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
    mcp: OnceLock<Arc<Launch>>,
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
        }
    }

    /// Gives the sessions started from now on the Beep MCP server, as
    /// `launch` starts it. Only the first call counts.
    pub fn serve_mcp(&self, launch: Arc<Launch>) {
        // A second launch would give the same server.
        let _ = self.mcp.set(launch);
    }

    /// Whether `session` is one of this node's and still runs.
    pub(crate) fn runs(&self, session: &str) -> bool {
        self.get(session)
            .is_some_and(|session| session.live.borrow().status() != Status::Ended)
    }

    /// Puts `question` to the user in `session` and waits for the answer.
    /// Fails when the session ends or its turn is cancelled first.
    pub(crate) async fn ask(&self, session: &str, question: Asking) -> Result<Answered> {
        let session = self
            .get(session)
            .with_context(|| format!("there is no session {session} here"))?;
        let (reply, answer) = oneshot::channel();
        session
            .order(Command::Ask { question, reply })
            .await
            .map_err(anyhow::Error::msg)?;
        answer
            .await
            .context("the question was dropped: the turn was cancelled or the session ended")
    }

    /// The last `last` entries of a session this node runs and how many
    /// came before them, each Codex subagent among them with the steps
    /// its rollout holds: codex-acp sends none of them.
    async fn live_transcript(&self, session: &Session, last: usize) -> (Vec<agent::Entry>, u64) {
        let (mut entries, earlier) = session.live.borrow().transcript(last);
        let children = session.live.borrow().codex_children(earlier);
        if children.is_empty() {
            return (entries, earlier);
        }
        let stores = self.stores.clone();
        let read = spawn_blocking(move || {
            children
                .into_iter()
                .filter_map(|child| Some((child.call, stores.codex_run(&child.threads)?)))
                .collect::<Vec<_>>()
        })
        .await;
        let runs = match read {
            Ok(runs) => runs,
            Err(e) => {
                log::warn!("subagent rollouts not read: {e}");
                return (entries, earlier);
            }
        };
        let live = session.live.borrow();
        for (call, run) in runs {
            let Some((at, text)) = live.with_run(&call, &run) else {
                continue;
            };
            let index = at
                .checked_sub(earlier)
                .and_then(|i| usize::try_from(i).ok());
            if let Some(entry) = index.and_then(|i| entries.get_mut(i)) {
                entry.text = text;
            }
        }
        (entries, earlier)
    }

    /// Takes the user's settings for the agents in place of the ones
    /// before; sessions start with them from now on.
    pub fn set_settings(&self, settings: AgentSettings) {
        self.settings.set(settings);
    }

    /// The models the agent `name` can run on here; `None` when this node
    /// has no such agent.
    async fn models(&self, name: &str) -> Option<ModelList> {
        let kind = self.kinds.iter().find(|kind| kind.name == name)?;
        let kind = self
            .settings
            .configured(kind)
            .unwrap_or_else(|_| kind.clone());
        Some(self.models.get(name, models::find(kind)).await)
    }

    /// The CLI of `agent`, with `args`, as a plan read starts it: signed
    /// in as the agent's sessions are, with the user's settings for it.
    /// `None` when this node does not run it or the user turned it off.
    fn cli(&self, agent: AgentAccount, args: &[&str]) -> Option<tokio::process::Command> {
        let name = agent.slug();
        let kind = self.kinds.iter().find(|kind| kind.name == name)?;
        let kind = self.settings.configured(kind).ok()?;
        let program = harnesses::find(name)?;
        let mut command = process::command(program);
        spawn_env::apply(&mut command, Scope::Agent(Some(agent)));
        command
            .args(args)
            .envs(&kind.env)
            .current_dir(env::temp_dir());
        Some(command)
    }

    /// How much of each agent's plan is left, read from the CLIs of the
    /// agents this node runs. One that cannot say is left out.
    async fn read_plans(&self) -> Vec<PlanUsage> {
        let claude = self.cli(AgentAccount::Claude, plans::CLAUDE_ARGS);
        let codex = self.cli(AgentAccount::Codex, plans::CODEX_ARGS);
        let (claude, codex) = tokio::join!(
            async {
                match claude {
                    Some(command) => plans::read_claude(command).await,
                    None => Ok(None),
                }
            },
            async {
                match codex {
                    Some(command) => plans::read_codex(command).await,
                    None => Ok(None),
                }
            },
        );
        [(AgentAccount::Claude, claude), (AgentAccount::Codex, codex)]
            .into_iter()
            .filter_map(|(agent, read)| {
                read.unwrap_or_else(|e| {
                    log::debug!("{} plan usage not read: {e:#}", agent.slug());
                    None
                })
            })
            .collect()
    }

    /// What this node finds of each agent harness it knows.
    async fn harnesses(&self, refresh: bool) -> HarnessReport {
        harnesses::report(&self.kinds, &self.settings, &self.latest, refresh).await
    }

    /// The Claude Code sessions started outside Beep.
    pub fn external(&self) -> &External {
        &self.external
    }

    /// The Claude Code sessions started outside Beep, newest first.
    pub async fn external_sessions(
        &self,
        limit: u32,
    ) -> Result<Vec<beep_proto::node::agent::ExternalSession>> {
        self.external.list(limit, None, self.owned()).await
    }

    /// Every session the agents' stores hold, with the folder it ran in.
    async fn sightings(&self) -> Result<Vec<found::Seen>> {
        let stores = self.stores.clone();
        let others = spawn_blocking(move || stores.sessions(None));
        let (claude, others) = tokio::join!(self.external.sightings(), others);
        let mut seen = claude.unwrap_or_else(|e| {
            log::warn!("Claude Code sessions not read: {e:#}");
            Vec::new()
        });
        seen.extend(others.context("reading the agents' sessions")?);
        Ok(seen)
    }

    /// The checkouts coding agents worked in, newest activity first.
    pub async fn found_folders(&self, limit: u32) -> Result<Vec<SessionFolder>> {
        let limit = match usize::try_from(limit).unwrap_or(MAX_FOLDERS) {
            0 => DEFAULT_FOLDERS,
            limit => limit.min(MAX_FOLDERS),
        };
        let seen = self.sightings().await?;
        let home = dirs::home_dir();
        spawn_blocking(move || found::folders(&seen, home.as_deref(), limit, found::checkout))
            .await
            .context("looking up the folders' checkouts")
    }

    /// The sessions that ran in `folders` or below, newest first.
    pub async fn sessions_in(
        &self,
        folders: Vec<String>,
        limit: u32,
    ) -> Result<Vec<ExternalSession>> {
        let limit = match usize::try_from(limit).unwrap_or(MAX_IN_FOLDERS) {
            0 => DEFAULT_IN_FOLDERS,
            limit => limit.min(MAX_IN_FOLDERS),
        };
        let folders: Vec<PathBuf> = folders
            .into_iter()
            .take(MAX_ASKED_FOLDERS)
            .map(PathBuf::from)
            .filter(|folder| folder.is_absolute())
            .collect();
        let picked = found::in_folders(self.sightings().await?, &folders, limit);
        let (claude, others): (Vec<found::Seen>, Vec<found::Seen>) = picked
            .into_iter()
            .partition(|seen| seen.agent == Agent::Claude);
        let ids: HashSet<String> = claude.into_iter().map(|seen| seen.id).collect();
        let owned = self.owned();
        let stores = self.stores.clone();
        let described = spawn_blocking(move || {
            let now = SystemTime::now();
            others
                .into_iter()
                .map(|seen| stores.describe(seen, now))
                .collect::<Vec<_>>()
        });
        let (claude, others) = tokio::join!(self.external.named(ids, owned.clone()), described);
        let mut sessions = claude?;
        sessions.extend(others.context("reading the sessions' titles")?);
        for session in &mut sessions {
            if session.taken_over.is_none() {
                session.taken_over = owned.get(&session.id).cloned();
            }
        }
        sessions.sort_by_key(|session| Reverse(session.last_activity));
        Ok(sessions)
    }

    /// The sessions this node runs, by the agent's own id, to the node's id.
    fn owned(&self) -> HashMap<String, String> {
        self.lock()
            .values()
            .filter_map(|session| {
                let agent_session = session.live.borrow().agent_session.clone()?;
                Some((agent_session, session.id.clone()))
            })
            .collect()
    }

    /// Takes over the Claude Code session `id` started outside Beep, once
    /// nothing runs it; one this node already runs is returned as it is.
    /// `id` may also be one of this node's sessions that ended: it goes on
    /// from the agent's own session.
    async fn resume(
        &self,
        kind: &AgentKind,
        id: &str,
        prompt: Option<String>,
    ) -> Result<Arc<Session>> {
        let ours = self.get(id);
        if let Some(session) = &ours
            && session.agent != kind.name
        {
            anyhow::bail!("session {id} runs {}", session.agent);
        }
        let ended = ours.map(|session| session.live.borrow().agent_session.clone());
        let id = match ended {
            Some(Some(agent_session)) => agent_session,
            Some(None) => anyhow::bail!("session {id} ended before the agent gave it an id"),
            None => id.to_owned(),
        };
        let id = id.as_str();
        let agent = Agent::from_name(&kind.name).with_context(|| {
            format!(
                "{} sessions started outside Beep cannot be taken over",
                kind.name
            )
        })?;
        let owned = self.owned();
        if let Some(session) = owned.get(id).and_then(|node_id| self.get(node_id))
            && session.agent == kind.name
            && session.live.borrow().status() != Status::Ended
        {
            if let Some(text) = prompt {
                session
                    .order(Command::Prompt(text.into()))
                    .await
                    .map_err(anyhow::Error::msg)?;
            }
            return Ok(session);
        }
        let found = match agent {
            Agent::Claude => self
                .external
                .list(1, Some(id.to_owned()), owned)
                .await?
                .into_iter()
                .next(),
            other => {
                let stores = self.stores.clone();
                let id = id.to_owned();
                spawn_blocking(move || stores.find(other, &id)).await?
            }
        }
        .with_context(|| format!("there is no {} session {id} on this machine", kind.name))?;
        if let Some(reason) = claude::refuse_takeover(&found) {
            anyhow::bail!("{reason}");
        }
        self.start(
            kind,
            Some(&found.cwd),
            Begin {
                prompt,
                resume: Some(id.to_owned()),
                model: None,
                effort: None,
                note: None,
                brief: None,
            },
        )
    }

    /// The picture `key` of `session`'s transcript: kept, or else found
    /// again in the agent's own file of the session and kept. `session` is
    /// one of this node's sessions or one started outside Beep.
    pub(crate) async fn image(&self, session: &str, key: &str) -> Result<Option<Arc<Picture>>> {
        if let Some(picture) = self.images.get(session, key) {
            return Ok(Some(picture));
        }
        let Some(file) = self.session_file(session).await? else {
            return Ok(None);
        };
        let wanted = key.to_owned();
        let found = spawn_blocking(move || images::find_in_file(&file, &wanted))
            .await?
            .context("reading the session's file")?;
        let Some(picture) = found else {
            return Ok(None);
        };
        self.images.keep(session, picture.clone());
        Ok(Some(Arc::new(picture)))
    }

    /// The file the agent keeps `session` in: for one of this node's, the
    /// agent's own session it runs.
    async fn session_file(&self, session: &str) -> Result<Option<PathBuf>> {
        let (agent, id) = match self.get(session) {
            Some(running) => {
                let Some(id) = running.live.borrow().agent_session.clone() else {
                    return Ok(None);
                };
                (Agent::from_name(&running.agent), id)
            }
            None => (None, session.to_owned()),
        };
        if agent.is_none_or(|agent| agent == Agent::Claude)
            && let Some(file) = self.external.file(&id).await?
        {
            return Ok(Some(file));
        }
        if agent == Some(Agent::Claude) {
            return Ok(None);
        }
        let stores = self.stores.clone();
        Ok(spawn_blocking(move || stores.file(agent, &id)).await?)
    }

    /// Closes every running session and waits until their agents have
    /// stopped: for the node shutting down.
    pub async fn shutdown(&self) {
        let running: Vec<Arc<Session>> = self
            .lock()
            .values()
            .filter(|session| session.live.borrow().status() != Status::Ended)
            .cloned()
            .collect();
        futures::future::join_all(running.iter().map(|session| session.close())).await;
    }

    fn lock(&self) -> MutexGuard<'_, HashMap<String, Arc<Session>>> {
        self.sessions.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The protocols to offer: one per agent, and `agents/1` when there are any.
    pub fn offers(&self) -> Vec<String> {
        let mut offers: Vec<String> = self
            .kinds
            .iter()
            .map(|kind| agent::protocol(&kind.name))
            .collect();
        if !offers.is_empty() {
            offers.push(SESSIONS_PROTOCOL.to_owned());
        }
        offers
    }

    /// The agent `protocol` names, when this node has it.
    pub fn kind(&self, protocol: &str) -> Option<&AgentKind> {
        let name = agent::agent_of(protocol)?;
        self.kinds.iter().find(|kind| kind.name == name)
    }

    fn get(&self, id: &str) -> Option<Arc<Session>> {
        self.lock().get(id).cloned()
    }

    /// Every session, oldest first.
    fn list(&self) -> Vec<SessionInfo> {
        let mut sessions: Vec<SessionInfo> = self.lock().values().map(|s| s.info()).collect();
        sessions.sort_by_key(|info| info.started_at);
        sessions
    }

    /// Starts a session in the branch or worktree `checkout` makes in
    /// `cwd`, or in `cwd` itself without one. A start that would be refused
    /// is refused before anything is made.
    async fn start_in(
        &self,
        kind: &AgentKind,
        cwd: Option<&str>,
        checkout: Option<CheckoutPlan>,
        mut begin: Begin,
    ) -> Result<Arc<Session>> {
        if checkout.is_some() {
            // Refused starts must not leave a branch or worktree behind.
            self.settings.configured(kind)?;
            ensure_room(&self.lock())?;
        }
        let prompt = begin.prompt.as_deref().unwrap_or_default();
        let place = checkout::place(cwd.map(str::to_owned), checkout, prompt).await?;
        begin.note = place.note;
        begin.brief = place.brief;
        self.start(kind, place.folder.as_deref(), begin)
    }

    fn start(&self, kind: &AgentKind, cwd: Option<&str>, begin: Begin) -> Result<Arc<Session>> {
        let kind = &self.settings.configured(kind)?;
        let cwd = folder(cwd)?;
        let mut sessions = self.lock();
        // Checked under the lock: two takeovers of one session must not
        // both start it.
        if let Some(resume) = &begin.resume
            && let Some(running) = sessions.values().find(|session| {
                let live = session.live.borrow();
                live.status() != Status::Ended && live.agent_session.as_ref() == Some(resume)
            })
        {
            anyhow::bail!("session {resume} already runs here as {}", running.id);
        }
        ensure_room(&sessions)?;
        if sessions.len() >= MAX_KEPT {
            let oldest = sessions
                .values()
                .filter(|session| session.live.borrow().status() == Status::Ended)
                .min_by_key(|session| session.live.borrow().info.started_at)
                .map(|session| session.id.clone());
            if let Some(oldest) = oldest {
                sessions.remove(&oldest);
            }
        }
        let id = loop {
            let id = new_id();
            if !sessions.contains_key(&id) {
                break id;
            }
        };
        let resumed = begin.resume.is_some();
        let session = Arc::new(Session::start(Start {
            id: id.clone(),
            kind,
            cwd: &cwd,
            prompt: begin.prompt,
            resume: begin.resume,
            model: begin.model,
            effort: begin.effort,
            note: begin.note,
            brief: begin.brief,
            mcp: self.mcp.get().map(|launch| launch.server(&id)),
            images: self.images.clone(),
            usage: self.usage.clone(),
            alerts: self.alerts.clone(),
        })?);
        let how = if resumed { "took over" } else { "started" };
        log::info!("{how} {} session {id} in {}", kind.name, cwd.display());
        sessions.insert(id, session.clone());
        Ok(session)
    }
}

/// Refuses a start once `MAX_RUNNING` of `sessions` run.
fn ensure_room(sessions: &HashMap<String, Arc<Session>>) -> Result<()> {
    let running = sessions
        .values()
        .filter(|session| session.live.borrow().status() != Status::Ended)
        .count();
    anyhow::ensure!(
        running < MAX_RUNNING,
        "{MAX_RUNNING} sessions already run here; close one first"
    );
    Ok(())
}

/// How a session begins: on a prompt, and as a new session (on a model
/// and thinking level when given) or an earlier one resumed by the agent's
/// id.
struct Begin {
    prompt: Option<String>,
    resume: Option<String>,
    model: Option<ModelId>,
    effort: Option<agent::Effort>,
    /// A first line for the transcript, such as the branch it started on.
    note: Option<String>,
    /// Beep's word to the agent ahead of the first prompt.
    brief: Option<&'static str>,
}

/// The folder a session runs in: `cwd`, with `~` for the home folder, or
/// the home folder itself.
fn folder(cwd: Option<&str>) -> Result<PathBuf> {
    let home = dirs::home_dir();
    let path = match cwd.map(str::trim) {
        None | Some("") | Some("~") => home.clone().context("this machine has no home folder")?,
        Some(rest) if rest.starts_with("~/") => home
            .as_deref()
            .context("this machine has no home folder")?
            .join(&rest[2..]),
        Some(path) => PathBuf::from(path),
    };
    anyhow::ensure!(
        path.is_absolute(),
        "the folder must be an absolute path, not {}",
        path.display()
    );
    if path.is_dir() {
        return Ok(path);
    }
    // A folder named on another machine: `/Users/<name>/x` there is
    // `~/x` here, and `~/Projects/x` there is often `~/x` here and the other
    // way round, as project groups match them.
    if let Some(home) = home.as_deref()
        && let Some(found) = home_twins(&path, home).into_iter().find(|p| p.is_dir())
    {
        return Ok(found);
    }
    anyhow::bail!("there is no folder {}", path.display())
}

/// Where `path`, named under some home folder, may be under `home`: the
/// same place, then with a leading `Projects/` taken away or added.
fn home_twins(path: &Path, home: &Path) -> Vec<PathBuf> {
    let rest = path.strip_prefix(home).ok().or_else(|| {
        let mut parts = path.components();
        let root = parts.next()?;
        let top = parts.next()?;
        let under = matches!(top.as_os_str().to_str(), Some("Users" | "home"));
        (matches!(root, Component::RootDir) && under && parts.next().is_some())
            .then_some(parts.as_path())
    });
    let Some(rest) = rest else {
        return Vec::new();
    };
    let toggled = match rest.strip_prefix("Projects") {
        Ok(inner) if !inner.as_os_str().is_empty() => home.join(inner),
        _ => home.join("Projects").join(rest),
    };
    [home.join(rest), toggled]
        .into_iter()
        .filter(|p| p != path)
        .collect()
}

/// Six hex digits, random enough to tell sessions apart.
fn new_id() -> String {
    let random = RandomState::new().hash_one(std::time::SystemTime::now());
    format!("{:06x}", random & 0xff_ffff)
}

/// Serves one agent stream: starts or attaches to a session, then passes
/// orders to it and its changes back until either side ends.
pub async fn serve_session(
    mut tx: LinkTx,
    mut rx: LinkRx,
    agents: Arc<Agents>,
    kind: AgentKind,
) -> Result<()> {
    let first: Request = rx
        .recv()
        .await?
        .context("the stream closed before its first request")?;
    let session = match first {
        Request::Start {
            model: Some(model), ..
        } if !agent::valid_model(&model) => Err(format!("{model:?} is not a model name")),
        Request::Start {
            cwd,
            prompt,
            model,
            effort,
            checkout,
        } => {
            let begin = Begin {
                prompt: Some(prompt),
                resume: None,
                model: model.map(ModelId::new),
                effort,
                note: None,
                brief: None,
            };
            agents
                .start_in(&kind, cwd.as_deref(), checkout, begin)
                .await
                .map_err(|e| format!("{e:#}"))
        }
        Request::Resume { session, prompt } => agents
            .resume(&kind, &session, prompt)
            .await
            .map_err(|e| format!("{e:#}")),
        Request::Attach { session } => match agents.get(&session) {
            Some(found) if found.agent == kind.name => Ok(found),
            Some(found) => Err(format!("session {session} runs {}", found.agent)),
            None => Err(format!("there is no session {session}")),
        },
        other => Err(format!("expected Start, Attach or Resume, got {other:?}")),
    };
    let session = match session {
        Ok(session) => session,
        Err(reason) => return tx.send(&Event::Refused { reason }).await,
    };
    let mut live = session.live.clone();
    let mut seen = Seen::new(&live.borrow_and_update().info);
    tx.send(&Event::Attached {
        session: Box::new(session.info()),
    })
    .await?;
    if seen.state.status == Status::Ended {
        return tx
            .send(&Event::Ended {
                reason: seen.ended(),
            })
            .await;
    }
    loop {
        tokio::select! {
            request = rx.recv::<Request>() => {
                let command = match request {
                    Ok(Some(Request::Prompt { text })) => Command::Prompt(text.into()),
                    Ok(Some(Request::PromptImages { text, images })) => {
                        match Prompt::with_images(text, images) {
                            Ok(prompt) => Command::Prompt(prompt),
                            Err(error) => {
                                tx.send(&Event::Ack { error: Some(error) }).await?;
                                continue;
                            }
                        }
                    }
                    Ok(Some(Request::Answer { question, reply })) => Command::Answer { question, reply },
                    Ok(Some(Request::Cancel {})) => Command::Cancel,
                    Ok(Some(Request::Close {})) => Command::Close,
                    Ok(Some(Request::Configure { model, effort })) => Command::Configure { model, effort },
                    Ok(Some(other)) => {
                        let error = Some(format!("{other:?} is not for an open session"));
                        tx.send(&Event::Ack { error }).await?;
                        continue;
                    }
                    // The session keeps running for the next stream.
                    Ok(None) => return Ok(()),
                    Err(e) => {
                        log::debug!("skipped a request: {e:#}");
                        tx.send(&Event::Ack { error: Some("unknown request".into()) }).await?;
                        continue;
                    }
                };
                let error = session.order(command).await.err();
                tx.send(&Event::Ack { error }).await?;
            }
            changed = live.changed() => {
                let events = seen.events(&live.borrow_and_update());
                let mut ended = false;
                for event in events {
                    ended |= matches!(event, Event::Ended { .. });
                    tx.send(&event).await?;
                }
                if ended {
                    return Ok(());
                }
                if changed.is_err() {
                    let reason = "the session stopped".to_owned();
                    return tx.send(&Event::Ended { reason }).await;
                }
            }
        }
    }
}

/// What a stream has told the server, to send only what changed.
struct Seen {
    state: State,
    question: Option<u32>,
    turns: u32,
}

impl Seen {
    fn new(info: &SessionInfo) -> Self {
        Self {
            state: info.state.clone(),
            question: info.question.as_ref().map(|question| question.id),
            turns: info.turns,
        }
    }

    fn ended(&self) -> String {
        self.state.error.clone().unwrap_or_else(|| "closed".into())
    }

    /// The events that bring the server up to `live`, state first.
    fn events(&mut self, live: &live::Live) -> Vec<Event> {
        let info = &live.info;
        let mut events = Vec::new();
        if info.state != self.state {
            self.state = info.state.clone();
            events.push(Event::State {
                state: self.state.clone(),
            });
        }
        let question = info.question.as_ref().map(|question| question.id);
        if question != self.question {
            self.question = question;
            if let Some(question) = &info.question {
                events.push(Event::Question {
                    question: question.clone(),
                });
            }
        }
        if info.turns > self.turns {
            self.turns = info.turns;
            events.push(Event::TurnDone {
                turn: info.turns,
                stop_reason: live.last_stop.unwrap_or(agent::StopReason::Other),
                last_message: info.last_message.clone(),
            });
        }
        if info.state.status == Status::Ended {
            events.push(Event::Ended {
                reason: self.ended(),
            });
        }
        events
    }
}

/// Serves one `agents/1` request.
pub async fn serve_sessions(mut tx: LinkTx, mut rx: LinkRx, agents: Arc<Agents>) -> Result<()> {
    let Some(request) = rx.recv::<SessionsRequest>().await? else {
        return Ok(());
    };
    let reply = match request {
        SessionsRequest::List {} => SessionsReply::Sessions {
            sessions: agents.list(),
        },
        SessionsRequest::Transcript { session, last } => {
            let last = usize::try_from(last.min(MAX_TRANSCRIPT)).unwrap_or(usize::MAX);
            let read = match agents.get(&session) {
                Some(found) => Ok(Some(agents.live_transcript(&found, last).await)),
                // A Claude Code session started outside Beep.
                None => match agents.external.transcript(&session, last).await {
                    Ok(Some(read)) => {
                        for picture in read.pictures {
                            agents.images.keep(&session, picture);
                        }
                        Ok(Some((read.entries, read.earlier)))
                    }
                    Ok(None) => Ok(None),
                    Err(e) => Err(e),
                },
            };
            match read {
                Ok(Some((entries, earlier))) => SessionsReply::Transcript {
                    entries,
                    earlier: u32::try_from(earlier).unwrap_or(u32::MAX),
                },
                Ok(None) => SessionsReply::Error {
                    message: format!("there is no session {session}"),
                },
                Err(e) => SessionsReply::Error {
                    message: format!("could not read session {session}: {e:#}"),
                },
            }
        }
        SessionsRequest::External {
            agent,
            limit,
            session,
        } if agent == "claude" => {
            match agents.external.list(limit, session, agents.owned()).await {
                Ok(sessions) => SessionsReply::External { sessions },
                Err(e) => failed(e),
            }
        }
        SessionsRequest::External { agent, .. } => SessionsReply::Error {
            message: format!("this node lists no {agent} sessions started outside Beep"),
        },
        SessionsRequest::Folders { limit } => match agents.found_folders(limit).await {
            Ok(folders) => SessionsReply::Folders { folders },
            Err(e) => failed(e),
        },
        SessionsRequest::InFolders { folders, limit } => {
            match agents.sessions_in(folders, limit).await {
                Ok(sessions) => SessionsReply::External { sessions },
                Err(e) => failed(e),
            }
        }
        SessionsRequest::Watch { sessions } => {
            return agents.external.watch(tx, rx, sessions).await;
        }
        SessionsRequest::FollowImages {} => return send_images(tx, rx, &agents.images).await,
        SessionsRequest::FollowAlerts {} => return send_alerts(tx, rx, &agents.alerts).await,
        SessionsRequest::FollowUsage { history } => {
            return send_usage(tx, rx, &agents, history).await;
        }
        SessionsRequest::Image { session, key } => match agents.image(&session, &key).await {
            Ok(Some(picture)) if picture.data.len() <= MAX_IMAGE => SessionsReply::Image {
                mime: picture.mime.clone(),
                data: picture.data.clone(),
            },
            Ok(Some(_)) => SessionsReply::Error {
                message: format!("image {key} is too large to send"),
            },
            Ok(None) => SessionsReply::Error {
                message: format!("session {session} has no image {key}"),
            },
            Err(e) => SessionsReply::Error {
                message: format!("could not read session {session}: {e:#}"),
            },
        },
        SessionsRequest::File { path, offset } => {
            match agents.reachable(&path, reach::Access::Read).await {
                Ok(real) => {
                    let read = spawn_blocking(move || {
                        let real = real.to_str().context("the file's path is not UTF-8")?;
                        media_file::read_chunk(real, offset, None)
                    })
                    .await;
                    match read {
                        Ok(Ok(Some(chunk))) => file_reply(chunk),
                        Ok(Ok(None)) => SessionsReply::NoFile {},
                        Ok(Err(e)) => failed(e),
                        Err(e) => SessionsReply::Error {
                            message: format!("the file read stopped: {e}"),
                        },
                    }
                }
                Err(e) => failed(e),
            }
        }
        SessionsRequest::Folder {
            root,
            path,
            ignored,
        } => match agents.reachable(&root, reach::Access::Read).await {
            Ok(real) => {
                work_request(move || {
                    let root = work_folder::WorkRoot::at(&real)?;
                    let folder = work_folder::list(&root, &path, ignored)?;
                    Ok(SessionsReply::Folder { folder })
                })
                .await
            }
            Err(e) => failed(e),
        },
        SessionsRequest::ReadFile {
            root,
            path,
            offset,
            len,
        } => match agents.reachable(&root, reach::Access::Read).await {
            Ok(real) => {
                work_request(move || {
                    let root = work_folder::WorkRoot::at(&real)?;
                    Ok(match work_folder::read(&root, &path, offset, len)? {
                        Some(chunk) => file_reply(chunk),
                        None => SessionsReply::NoFile {},
                    })
                })
                .await
            }
            Err(e) => failed(e),
        },
        SessionsRequest::WriteFile {
            root,
            path,
            offset,
            size,
            data,
        } => match agents.reachable(&root, reach::Access::Write).await {
            Ok(real) => {
                work_request(move || {
                    let root = work_folder::WorkRoot::at(&real)?;
                    let path = work_folder::write(&root, &path, offset, size, &data)?;
                    Ok(SessionsReply::Written { path })
                })
                .await
            }
            Err(e) => failed(e),
        },
        SessionsRequest::CustomAgents { folder } => {
            match agents.reachable(&folder, reach::Access::Work).await {
                Ok(real) => {
                    let home = dirs::home_dir();
                    work_request(move || {
                        let agents = custom_agents::list(&real, home.as_deref())?;
                        Ok(SessionsReply::CustomAgents { agents })
                    })
                    .await
                }
                Err(e) => failed(e),
            }
        }
        SessionsRequest::Models { agent } => match agents.models(&agent).await {
            Some(models) => SessionsReply::Models { models },
            None => SessionsReply::Error {
                message: format!("this node has no agent {agent}"),
            },
        },
        SessionsRequest::Harnesses { refresh } => SessionsReply::Harnesses {
            report: agents.harnesses(refresh).await,
        },
        SessionsRequest::UpdateHarness { agent } => {
            match harnesses::update(&agents.kinds, &agents.latest, &agent).await {
                Ok(()) => SessionsReply::Harnesses {
                    report: agents.harnesses(false).await,
                },
                Err(e) => failed(e),
            }
        }
        other => SessionsReply::Error {
            message: format!("this node does not serve {other:?}"),
        },
    };
    tx.send(&reply).await
}

/// The reply saying `e`, with its causes.
fn failed(e: impl Display) -> SessionsReply {
    SessionsReply::Error {
        message: format!("{e:#}"),
    }
}

/// The reply carrying a chunk of a file.
fn file_reply(chunk: media_file::Chunk) -> SessionsReply {
    SessionsReply::File {
        size: chunk.size,
        mime: chunk.mime.to_owned(),
        data: chunk.data,
    }
}

/// Runs `job`, which blocks on a session's working folder, off the async
/// workers; its error becomes the `Error` reply.
async fn work_request(
    job: impl FnOnce() -> Result<SessionsReply> + Send + 'static,
) -> SessionsReply {
    match spawn_blocking(job).await {
        Ok(Ok(reply)) => reply,
        Ok(Err(e)) => failed(e),
        Err(e) => SessionsReply::Error {
            message: format!("the folder work stopped: {e}"),
        },
    }
}

/// Sends the server each picture a running session newly shows, until it
/// closes the stream. One too large for a frame is left out: the server
/// cannot ask for it either.
async fn send_images(mut tx: LinkTx, mut rx: LinkRx, images: &Images) -> Result<()> {
    loop {
        tokio::select! {
            unsent = images.next_unsent() => {
                if unsent.picture.data.len() > MAX_IMAGE {
                    continue;
                }
                tx.send(&SessionsReply::SessionImage {
                    session: unsent.session,
                    mime: unsent.picture.mime.clone(),
                    data: unsent.picture.data.clone(),
                })
                .await?;
            }
            // The server sends nothing more; anything is its goodbye.
            _ = rx.recv::<SessionsRequest>() => return Ok(()),
        }
    }
}

/// Sends the server each alert as it comes, until it closes the stream.
async fn send_alerts(mut tx: LinkTx, mut rx: LinkRx, alerts: &AlertHub) -> Result<()> {
    loop {
        tokio::select! {
            alert = alerts.next() => {
                tx.send(&SessionsReply::Alert { alert }).await?;
            }
            // The server sends nothing more; anything is its goodbye.
            _ = rx.recv::<SessionsRequest>() => return Ok(()),
        }
    }
}

/// Sends the server what finished turns spent and each agent's plan
/// usage, reading the plans every `PLAN_EVERY` and when the server asks,
/// until the server closes the stream. With `history`, also what sessions
/// spent as the agents' logs tell it, read every `SPEND_EVERY`.
async fn send_usage(mut tx: LinkTx, mut rx: LinkRx, agents: &Agents, history: bool) -> Result<()> {
    let mut plans = agents.usage.plans();
    let mut sent = Plans::new();
    let mut every = interval(PLAN_EVERY);
    every.set_missed_tick_behavior(MissedTickBehavior::Delay);
    // A read runs a CLI or two; it runs beside the loop, not in it.
    let mut reading: Option<BoxFuture<'_, Vec<PlanUsage>>> = None;
    let mut last_read: Option<Instant> = None;
    let mut every_scan = interval(SPEND_EVERY);
    every_scan.set_missed_tick_behavior(MissedTickBehavior::Delay);
    // A pass reads files; it runs on the blocking pool beside the loop.
    // Dropped with the stream, it still finishes and saves what it read.
    let mut scanning: Option<JoinHandle<Vec<SessionSpend>>> = None;
    loop {
        tokio::select! {
            turn = agents.usage.next_turn() => {
                tx.send(&SessionsReply::TurnUsage { usage: turn }).await?;
            }
            Ok(()) = plans.changed() => {
                let now = plans.borrow_and_update().clone();
                for plan in now.values().filter(|plan| sent.get(&plan.agent) != Some(plan)) {
                    tx.send(&SessionsReply::PlanUsage { usage: plan.clone() }).await?;
                }
                sent = now;
            }
            _ = every.tick(), if reading.is_none() => {
                reading = Some(Box::pin(agents.read_plans()));
            }
            Some(read) = async { Some(reading.as_mut()?.await) }, if reading.is_some() => {
                reading = None;
                last_read = Some(Instant::now());
                for plan in read {
                    agents.usage.set_plan(plan);
                }
            }
            _ = every_scan.tick(), if history && scanning.is_none() => {
                let spend = agents.spend.clone();
                scanning = Some(spawn_blocking(move || spend.pass(unix_now())));
            }
            Some(read) = async { Some(scanning.as_mut()?.await) }, if scanning.is_some() => {
                scanning = None;
                match read {
                    Ok(reports) => {
                        for report in reports {
                            tx.send(&SessionsReply::SessionSpend { usage: report.clone() }).await?;
                            agents.spend.sent(&report);
                        }
                    }
                    Err(e) => log::warn!("reading the agents' logs failed: {e}"),
                }
            }
            message = rx.recv::<SessionsRequest>() => match message {
                Ok(Some(SessionsRequest::RefreshPlanUsage {})) => {
                    let recent = last_read.is_some_and(|at| at.elapsed() < PLAN_REFRESH_GAP);
                    if reading.is_none() && !recent {
                        reading = Some(Box::pin(agents.read_plans()));
                    }
                }
                // Anything else is the server's goodbye.
                _ => return Ok(()),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A Claude Code session id, as its transcript file is named.
    #[cfg(unix)]
    const CLAUDE_ID: &str = "0f3c2a9e-1111-4222-8333-944455556666";

    /// An adapter that, loading a session, replays a prompt with `$1`
    /// inlined as a data URL, as Codex does, then waits.
    #[cfg(unix)]
    const REPLAYING_ADAPTER: &str = r#"
id() { printf '%s\n' "$line" | sed -nE 's/.*"id":("[^"]*"|[0-9]+).*/\1/p'; }
reply() { printf '{"jsonrpc":"2.0","id":%s,"result":%s}\n' "$(id)" "$1"; }
read -r line
reply '{"protocolVersion":1,"agentCapabilities":{"loadSession":true}}'
read -r line
printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s","update":{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"what is this [@shot.png](data:image/png;base64,%s)"}}}}\n' "$1"
reply '{}'
while read -r line; do reply '{}'; done
"#;

    #[cfg(unix)]
    fn png(fill: u8) -> Vec<u8> {
        let mut data = b"\x89PNG\r\n\x1a\n".to_vec();
        data.resize(48, fill);
        data
    }

    /// Agents that read Claude Code's sessions in `root`.
    #[cfg(unix)]
    fn agents_at(root: &std::path::Path) -> Agents {
        Agents {
            external: External::at(root),
            ..Agents::default()
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_replayed_image_is_served_and_found_again_once_evicted() {
        use base64::{Engine, engine::general_purpose::STANDARD};
        use beep_proto::image_key;
        let dir = std::env::temp_dir().join(format!("beep-agent-images-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let projects = dir.join("claude/projects/-srv-project");
        std::fs::create_dir_all(&projects).unwrap();
        let shot = png(1);
        let encoded = STANDARD.encode(&shot);
        // Claude Code's own file of the session holds the image too.
        let line = serde_json::json!({"type": "user", "message": {"role": "user", "content": [
            {"type": "text", "text": "what is this"},
            {"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": encoded}}
        ]}});
        std::fs::write(
            projects.join(format!("{CLAUDE_ID}.jsonl")),
            format!("{line}\n"),
        )
        .unwrap();
        let agents = agents_at(&dir.join("claude"));
        let kind = AgentKind {
            name: "claude".into(),
            program: "sh".into(),
            args: vec![
                "-c".into(),
                REPLAYING_ADAPTER.into(),
                "fake-adapter".into(),
                encoded,
            ],
            env: BTreeMap::new(),
        };
        let begin = Begin {
            prompt: None,
            resume: Some(CLAUDE_ID.into()),
            model: None,
            effort: None,
            note: None,
            brief: None,
        };
        let session = agents
            .start(&kind, Some(dir.to_str().unwrap()), begin)
            .unwrap();
        let mut live = session.live.clone();
        tokio::time::timeout(
            std::time::Duration::from_secs(10),
            live.wait_for(|live| matches!(live.status(), Status::Idle | Status::Ended)),
        )
        .await
        .expect("the session loads")
        .unwrap();
        let key = image_key(&shot);
        let (entries, _) = session.live.borrow().transcript(10);
        assert_eq!(
            entries[0].text,
            format!("what is this\n[image: image-{key}]")
        );
        let served = agents.image(&session.id, &key).await.unwrap();
        assert_eq!(
            served.map(|picture| picture.data.clone()),
            Some(shot.clone())
        );
        // Newer pictures push it out; the session's file still has it.
        for fill in 2..40 {
            agents.images.keep(
                &session.id,
                Picture {
                    mime: "image/png".into(),
                    data: png(fill),
                },
            );
        }
        assert!(agents.images.get(&session.id, &key).is_none(), "evicted");
        let found = agents.image(&session.id, &key).await.unwrap();
        assert_eq!(
            found.map(|picture| picture.data.clone()),
            Some(shot.clone())
        );
        assert!(agents.images.get(&session.id, &key).is_some(), "kept again");
        // A session started outside Beep is read from its file by its id.
        let outside = agents.image(CLAUDE_ID, &key).await.unwrap();
        assert_eq!(outside.map(|picture| picture.data.clone()), Some(shot));
        let missing = agents.image(CLAUDE_ID, &image_key(b"other")).await;
        assert!(missing.unwrap().is_none());
        session.close().await;
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn only_claude_code_needs_its_mode_set() {
        assert_eq!(
            known("claude").and_then(|known| known.bypass_mode),
            Some("bypassPermissions")
        );
        for agent in ["codex", "gemini", "pi", "flsh", "opencode", "custom"] {
            assert_eq!(known(agent).and_then(|k| k.bypass_mode), None, "{agent}");
        }
    }

    #[test]
    fn flsh_and_gemini_take_the_model_on_their_command_line() {
        let flag = |agent| known(agent).is_some_and(|known| known.model_flag);
        assert!(flag("flsh") && flag("gemini"));
        for agent in ["claude", "codex", "pi", "opencode", "custom"] {
            assert!(!flag(agent), "{agent}");
        }
    }

    #[test]
    fn extra_agents_replace_known_ones() {
        let custom = AgentKind {
            name: "claude".into(),
            program: "/opt/acp/claude".into(),
            args: vec![],
            env: BTreeMap::new(),
        };
        let kinds = detect(std::slice::from_ref(&custom));
        assert_eq!(kinds.iter().filter(|kind| kind.name == "claude").count(), 1);
        assert!(kinds.contains(&custom));
    }

    #[test]
    fn pi_and_flsh_are_known() {
        let known = |name: &str| KNOWN.iter().find(|known| known.name == name);
        assert!(matches!(
            known("pi").map(|known| &known.adapter),
            Some(Adapter::Npm { bin: "pi-acp", .. })
        ));
        assert!(matches!(
            known("flsh").map(|known| &known.adapter),
            Some(Adapter::Flags(["acp"]))
        ));
    }

    #[test]
    fn opencode_runs_its_own_acp_server() {
        let known = KNOWN
            .iter()
            .find(|known| known.name == "opencode")
            .expect("opencode is known");
        let installed = |program: &str| program == "opencode";
        assert_eq!(
            launch(known, &installed),
            Some(AgentKind {
                name: "opencode".into(),
                program: "opencode".into(),
                args: vec!["acp".into()],
                env: BTreeMap::from([(
                    "OPENCODE_PERMISSION".to_owned(),
                    r#"{"*":"allow"}"#.to_owned()
                )]),
            })
        );
        assert_eq!(launch(known, &|_: &str| false), None);
        assert_eq!(Agent::from_name("opencode"), Some(Agent::OpenCode));
    }

    #[test]
    fn every_known_agent_starts_without_permission_prompts() {
        let launched = |name: &str| {
            let known = KNOWN
                .iter()
                .find(|known| known.name == name)
                .expect("a known agent");
            let kind = launch(known, &|_: &str| true).expect("installed");
            let env: Vec<(String, String)> = kind.env.into_iter().collect();
            (kind.args, env)
        };
        let pair = |name: &str, value: &str| (name.to_owned(), value.to_owned());
        // Claude Code is switched to bypassPermissions by the session.
        assert_eq!(launched("claude"), (vec![], vec![]));
        assert_eq!(
            launched("codex"),
            (
                vec![],
                vec![pair("INITIAL_AGENT_MODE", "agent-full-access")]
            )
        );
        assert_eq!(
            launched("gemini"),
            (
                vec!["--acp".into(), "--approval-mode".into(), "yolo".into()],
                vec![pair("GEMINI_CLI_TRUST_WORKSPACE", "true")]
            )
        );
        assert_eq!(
            launched("opencode"),
            (
                vec!["acp".into()],
                vec![pair("OPENCODE_PERMISSION", r#"{"*":"allow"}"#)]
            )
        );
        // Neither has permission prompts to turn off.
        assert_eq!(launched("pi"), (vec![], vec![]));
        assert_eq!(launched("flsh"), (vec!["acp".into()], vec![]));
        let rules: serde_json::Value =
            serde_json::from_str(&launched("opencode").1[0].1).expect("JSON");
        assert_eq!(rules, serde_json::json!({"*": "allow"}));
    }

    #[test]
    fn node_toml_agents_may_set_variables() {
        let kind: AgentKind = toml::from_str(
            "name = \"codex\"\nprogram = \"/opt/acp/codex-acp\"\n\
             env = { INITIAL_AGENT_MODE = \"agent-full-access\" }",
        )
        .expect("an agent");
        assert_eq!(
            kind.env.get("INITIAL_AGENT_MODE").map(String::as_str),
            Some("agent-full-access")
        );
        let plain: AgentKind = toml::from_str("name = \"x\"\nprogram = \"x\"").expect("an agent");
        assert!(plain.env.is_empty());
        assert!(!toml::to_string(&plain).expect("toml").contains("env"));
    }

    #[test]
    fn an_npm_adapter_runs_its_binary_else_npx() {
        let known = KNOWN
            .iter()
            .find(|known| known.name == "pi")
            .expect("pi is known");
        let launched = |installed: &[&str]| {
            let installed = |program: &str| installed.contains(&program);
            launch(known, &installed).map(|kind| (kind.program, kind.args))
        };
        assert_eq!(launched(&["pi", "pi-acp"]), Some(("pi-acp".into(), vec![])));
        assert_eq!(
            launched(&["pi", "npx"]),
            Some(("npx".into(), vec!["-y".into(), "pi-acp".into()]))
        );
        assert_eq!(launched(&["pi"]), None);
    }

    #[test]
    fn offers_name_each_agent_and_the_session_list() {
        let agents = Agents::new(vec![AgentKind {
            name: "fake".into(),
            program: "true".into(),
            args: vec![],
            env: BTreeMap::new(),
        }]);
        assert_eq!(agents.offers(), ["fake/1", SESSIONS_PROTOCOL]);
        assert!(agents.kind("fake/1").is_some());
        assert!(agents.kind("claude/1").is_none());
        assert!(Agents::new(vec![]).offers().is_empty());
    }

    #[test]
    fn a_folder_must_exist_and_be_absolute() {
        assert!(folder(Some(if cfg!(windows) { "C:\\" } else { "/" })).is_ok());
        assert!(folder(None).is_ok());
        assert!(folder(Some("relative/dir")).is_err());
        assert!(folder(Some("/no/such/folder/here")).is_err());
    }

    #[test]
    fn another_machines_home_folder_maps_here() {
        let home = Path::new("/home/you");
        assert_eq!(
            home_twins(Path::new("/Users/you/Projects/work/tool"), home),
            [
                PathBuf::from("/home/you/Projects/work/tool"),
                PathBuf::from("/home/you/work/tool"),
            ]
        );
        assert_eq!(
            home_twins(Path::new("/home/you/work/tool"), home),
            [PathBuf::from("/home/you/Projects/work/tool")]
        );
        assert!(home_twins(Path::new("/srv/data"), home).is_empty());
        assert!(
            home_twins(Path::new("/home/you"), home).is_empty()
                || home_twins(Path::new("/home/you"), home)
                    == [PathBuf::from("/home/you/Projects")]
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn one_session_cannot_be_taken_over_twice() {
        // An adapter that never answers: the session stays starting.
        let kind = AgentKind {
            name: "claude".into(),
            program: "sh".into(),
            args: vec!["-c".into(), "cat > /dev/null".into()],
            env: BTreeMap::new(),
        };
        let agents = Agents::new(vec![kind.clone()]);
        let id = "0f3c2a9e-1111-4222-8333-944455556666";
        let begin = || Begin {
            prompt: None,
            resume: Some(id.to_owned()),
            model: None,
            effort: None,
            note: None,
            brief: None,
        };
        let first = agents.start(&kind, Some("/"), begin()).unwrap();
        let second = agents.start(&kind, Some("/"), begin()).err().unwrap();
        assert!(
            second
                .to_string()
                .contains(&format!("already runs here as {}", first.id)),
            "{second}"
        );
        assert_eq!(agents.owned().get(id), Some(&first.id));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn an_ended_node_session_resumes_from_the_agents_session() {
        let kind = AgentKind {
            name: "claude".into(),
            program: "sh".into(),
            args: vec!["-c".into(), "cat > /dev/null".into()],
            env: BTreeMap::new(),
        };
        let agents = Agents::new(vec![kind.clone()]);
        let id = "0f3c2a9e-1111-4222-8333-944455556666";
        let begin = Begin {
            prompt: None,
            resume: Some(id.to_owned()),
            model: None,
            effort: None,
            note: None,
            brief: None,
        };
        let first = agents.start(&kind, Some("/"), begin).unwrap();
        first.close().await;
        // Nothing on this machine has the agent's session, so it is the
        // agent's id that the node looks for, not its own.
        let err = agents.resume(&kind, &first.id, None).await.err().unwrap();
        assert!(err.to_string().contains(id), "{err}");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_refused_start_makes_no_branch() {
        let (_dir, work) = checkout::tests::repo("refused");
        let kind = AgentKind {
            name: "codex".into(),
            program: "sh".into(),
            args: vec!["-c".into(), "cat > /dev/null".into()],
            env: BTreeMap::new(),
        };
        let agents = Agents::new(vec![kind.clone()]);
        agents.set_settings(AgentSettings {
            harnesses: vec![beep_proto::node::harness::HarnessConfig {
                enabled: false,
                ..beep_proto::node::harness::HarnessConfig::new("codex")
            }],
        });
        let begin = Begin {
            prompt: Some("Fix the login bug".into()),
            resume: None,
            model: None,
            effort: None,
            note: None,
            brief: None,
        };
        let plan = CheckoutPlan::NewBranch { base: None };
        let refused = agents
            .start_in(&kind, work.to_str(), Some(plan), begin)
            .await
            .err();
        assert!(refused.is_some_and(|e| e.to_string().contains("turned off")));
        assert_eq!(checkout::tests::current(&work), "main");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_session_resumes_only_on_its_own_agent() {
        let kind = |name: &str| AgentKind {
            name: name.into(),
            program: "sh".into(),
            args: vec!["-c".into(), "cat > /dev/null".into()],
            env: BTreeMap::new(),
        };
        let (claude, codex) = (kind("claude"), kind("codex"));
        let agents = Agents::new(vec![claude.clone(), codex.clone()]);
        let begin = Begin {
            prompt: None,
            resume: Some("0199a1b2-3c4d-7e5f-8a9b-0c1d2e3f4a5b".to_owned()),
            model: None,
            effort: None,
            note: None,
            brief: None,
        };
        let running = agents.start(&codex, Some("/"), begin).unwrap();
        let err = agents.resume(&claude, &running.id, None).await.err();
        assert!(
            err.is_some_and(|e| e.to_string().contains("runs codex")),
            "a claude stream took over a codex session"
        );
        running.close().await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn models_are_listed_for_the_agents_this_node_has() {
        // Answers one `session/new` with a model; a second start would
        // fail, so a second listing must come from the cache.
        let script = r#"
id() { printf '%s\n' "$line" | sed -nE 's/.*"id":("[^"]*"|[0-9]+).*/\1/p'; }
reply() { printf '{"jsonrpc":"2.0","id":%s,"result":%s}\n' "$(id)" "$1"; }
[ -e "$1" ] && exit 1
: > "$1"
read -r line
reply '{"protocolVersion":1,"agentCapabilities":{}}'
read -r line
reply '{"sessionId":"s","models":{"availableModels":[{"modelId":"deep"}],"currentModelId":"deep"}}'
cat > /dev/null
"#;
        let dir = tempfile::tempdir().unwrap();
        let kind = AgentKind {
            name: "codex".into(),
            program: "sh".into(),
            args: vec![
                "-c".into(),
                script.into(),
                "fake-adapter".into(),
                dir.path().join("started").display().to_string(),
            ],
            env: BTreeMap::new(),
        };
        let agents = Agents::new(vec![kind]);
        let first = agents.models("codex").await.expect("a known agent");
        assert_eq!(first.pick, agent::ModelPick::Listed);
        assert_eq!(first.current.as_deref(), Some("deep"));
        assert_eq!(agents.models("codex").await, Some(first));
        assert_eq!(agents.models("claude").await, None);
    }

    #[test]
    fn ids_are_six_hex_digits() {
        let id = new_id();
        assert_eq!(id.len(), 6);
        assert!(id.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
