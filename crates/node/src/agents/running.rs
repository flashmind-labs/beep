//! Starting, finding and stopping the sessions this node runs.

use super::{
    AgentKind, Agents, Answered, Asking, Command, MAX_KEPT, MAX_RUNNING, NodeMode, Session, Start,
    checkout, harnesses, models, plans,
};
use crate::{
    agents::mcp::Launch,
    machine::provision::spawn_env::{self, Scope},
    run::process,
};
use anyhow::{Context, Result};
use beep_proto::node::{
    agent::{self, CheckoutPlan, ModelList, SESSIONS_PROTOCOL, SessionInfo, Status},
    harness::{AgentSettings, HarnessReport},
    usage::PlanUsage,
};
use beep_proto::{AgentAccount, ModelId};
use std::{
    collections::{HashMap, hash_map::RandomState},
    env,
    hash::BuildHasher,
    path::{Component, Path, PathBuf},
    sync::{Arc, MutexGuard, atomic::Ordering},
};
use tokio::{sync::oneshot, task::spawn_blocking};

impl Agents {
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
    pub(super) async fn live_transcript(
        &self,
        session: &Session,
        last: usize,
    ) -> (Vec<agent::Entry>, u64) {
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

    /// The server sent the user's settings: sessions held over a restart
    /// may start.
    pub fn settings_arrived(&self) {
        self.settled.send_replace(true);
    }

    /// The models the agent `name` can run on here; `None` when this node
    /// has no such agent.
    pub(super) async fn models(&self, name: &str) -> Option<ModelList> {
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
    pub(super) async fn read_plans(&self) -> Vec<PlanUsage> {
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
    pub(super) async fn harnesses(&self, refresh: bool) -> HarnessReport {
        harnesses::report(&self.kinds, &self.settings, &self.latest, refresh).await
    }

    /// Closes every running session and waits until their agents have
    /// stopped: for the node shutting down.
    pub async fn shutdown(&self) {
        self.begin_stopping();
        let running: Vec<Arc<Session>> = self
            .lock()
            .values()
            .filter(|session| session.live.borrow().status() != Status::Ended)
            .cloned()
            .collect();
        futures::future::join_all(running.iter().map(|session| session.close())).await;
    }

    /// Marks the node as stopping: sessions that end from now on ended
    /// with it, even when the service manager stopped their agents first.
    pub fn begin_stopping(&self) {
        self.stopping.store(true, Ordering::Relaxed);
    }

    pub(super) fn lock(&self) -> MutexGuard<'_, HashMap<String, Arc<Session>>> {
        crate::lock(&self.sessions)
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

    /// The session `id` names: by the id it started under or by the
    /// agent's own id.
    pub(super) fn get(&self, id: &str) -> Option<Arc<Session>> {
        find_session(&self.lock(), id)
    }

    /// Every session, oldest first.
    pub(super) fn list(&self) -> Vec<SessionInfo> {
        let mut sessions: Vec<SessionInfo> = self.lock().values().map(|s| s.info()).collect();
        sessions.sort_by_key(|info| info.started_at);
        sessions
    }

    /// Starts a session in the branch or worktree `checkout` makes in
    /// `cwd`, or in `cwd` itself without one. A start that would be refused
    /// is refused before anything is made.
    pub(super) async fn start_in(
        &self,
        kind: &AgentKind,
        cwd: Option<&str>,
        checkout: Option<CheckoutPlan>,
        mut begin: Begin,
    ) -> Result<Arc<Session>> {
        self.prepare(kind).await?;
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

    /// Readies what `kind` reads before it starts: for flsh, the user's
    /// flsh settings in its config.
    pub(super) async fn prepare(&self, kind: &AgentKind) -> Result<()> {
        let configured = self.settings.configured(kind)?;
        self.settings.prepare(&configured).await
    }

    pub(super) fn start(
        &self,
        kind: &AgentKind,
        cwd: Option<&str>,
        begin: Begin,
    ) -> Result<Arc<Session>> {
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
        // A resumed session goes by the agent's id from the start; a new
        // one by a short id until the agent gives its own.
        let id = match &begin.resume {
            Some(resume) => {
                // Ended under that id before: the new run takes its place.
                sessions.remove(resume);
                resume.clone()
            }
            None => loop {
                let id = new_id();
                if !sessions.contains_key(&id) {
                    break id;
                }
            },
        };
        let resumed = begin.resume.is_some();
        let session = Arc::new(Session::start(Start {
            id: id.clone(),
            kind,
            cwd: &cwd,
            prompt: begin.prompt,
            resume: begin.resume,
            model: begin.model,
            mode: begin.mode,
            effort: begin.effort,
            note: begin.note,
            brief: begin.brief,
            mcp: self.mcp.get().map(|launch| launch.server(&id)),
            images: self.images.clone(),
            usage: self.usage.clone(),
            alerts: self.alerts.clone(),
            node: NodeMode {
                stopping: self.stopping.clone(),
                holding: self.holding.subscribe(),
            },
        })?);
        let how = if resumed { "took over" } else { "started" };
        log::info!("{how} {} session {id} in {}", kind.name, cwd.display());
        sessions.insert(id, session.clone());
        crate::link::restart::changed();
        Ok(session)
    }
}

/// The session `id` names in `sessions`, by its key or the agent's id.
fn find_session(sessions: &HashMap<String, Arc<Session>>, id: &str) -> Option<Arc<Session>> {
    if let Some(found) = sessions.get(id) {
        return Some(found.clone());
    }
    sessions
        .values()
        .filter(|session| session.live.borrow().agent_session.as_deref() == Some(id))
        .max_by_key(|session| {
            let live = session.live.borrow();
            (live.status() != Status::Ended, live.info.started_at)
        })
        .cloned()
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
#[derive(Default)]
pub(super) struct Begin {
    pub(super) prompt: Option<String>,
    pub(super) resume: Option<String>,
    pub(super) model: Option<ModelId>,
    pub(super) effort: Option<agent::Effort>,
    /// The session mode it was in before the node restarted.
    pub(super) mode: Option<String>,
    /// A first line for the transcript, such as the branch it started on.
    pub(super) note: Option<String>,
    /// Beep's word to the agent ahead of the first prompt.
    pub(super) brief: Option<&'static str>,
}

/// The folder a session runs in: `cwd`, with `~` for the home folder, or
/// the home folder itself.
pub(super) fn folder(cwd: Option<&str>) -> Result<PathBuf> {
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
pub(super) fn home_twins(path: &Path, home: &Path) -> Vec<PathBuf> {
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
