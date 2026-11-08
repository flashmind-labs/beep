//! Starting, finding and stopping the sessions this node runs.

use super::{
    AgentKind, Agents, Answered, Asking, Command, MAX_KEPT, NodeMode, Prompt, Session, Start,
    checkout, harnesses, live::Live, models, plans, restart::SETTINGS_WAIT,
};
use crate::{
    agents::mcp::{self, Launch},
    machine::provision::spawn_env::{self, Scope},
    run::process,
};
use anyhow::{Context, Result};
use beep_proto::node::{
    agent::{self, CheckoutPlan, ModelList, ParentLink, SESSIONS_PROTOCOL, SessionInfo, Status},
    harness::{AgentSettings, HarnessReport},
    usage::PlanUsage,
};
use beep_proto::{AgentAccount, ModelId, Upload};
use std::{
    borrow::Cow,
    collections::{HashMap, hash_map::RandomState},
    env,
    hash::BuildHasher,
    path::{Component, Path, PathBuf},
    sync::{Arc, MutexGuard, atomic::Ordering},
};
use tokio::{
    sync::{oneshot, watch},
    task::spawn_blocking,
    time::timeout,
};

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
        // Dropped with the wait (a timeout, a call the agent gave up on, even while the order
        // is still being handed over), it takes the question back from the session.
        let mut waiting = Withdraw(Some(session.clone()));
        session
            .order(Command::Ask { question, reply })
            .await
            .map_err(anyhow::Error::msg)?;
        let answered = answer
            .await
            .context("the question was dropped: the turn was cancelled or the session ended");
        waiting.0 = None;
        answered
    }

    /// The last `last` entries of a session this node runs and how many
    /// came before them, each Codex subagent among them with the steps
    /// its rollout holds: codex-acp sends none of them.
    pub(super) async fn live_transcript(
        &self,
        session: &Session,
        last: usize,
        before: Option<u64>,
    ) -> (Vec<agent::Entry>, u64) {
        let (mut entries, earlier) = self.codex_transcript(session, last, before).await;
        if session.agent == "claude" {
            let (id, calls) = {
                let live = session.live.borrow();
                (
                    live.agent_session.clone(),
                    live.claude_children(earlier, entries.len()),
                )
            };
            if let Some(id) = id.filter(|_| !calls.is_empty()) {
                match self
                    .external
                    .attach_children(&id, calls, entries.clone())
                    .await
                {
                    Ok(linked) => entries = linked,
                    Err(e) => log::debug!("native child conversations not linked: {e}"),
                }
            }
        }
        mcp::children::show_children(self, session, &mut entries);
        (entries, earlier)
    }

    async fn codex_transcript(
        &self,
        session: &Session,
        last: usize,
        before: Option<u64>,
    ) -> (Vec<agent::Entry>, u64) {
        let (mut entries, earlier) = session.live.borrow().transcript_before(last, before);
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

    /// The settings now in force.
    pub fn settings(&self) -> AgentSettings {
        self.settings.current()
    }

    /// The mesh gave the user's settings: sessions held over a restart
    /// may start.
    pub fn settings_arrived(&self) {
        self.settled.send_replace(true);
    }

    /// The models the agent `name` can run on here; `None` when this node
    /// has no such agent.
    pub(super) async fn models(&self, name: &str) -> Option<ModelList> {
        let kind = self.named(name)?;
        let kind = self
            .settings
            .configured(&kind)
            .unwrap_or_else(|_| kind.clone());
        let default_model = kind.default_model.clone();
        let mut list = self.models.get(name, models::find(kind)).await;
        if let Some(model) = default_model {
            list.current = Some(model.to_string());
        }
        Some(list)
    }

    /// The CLI of `agent`, with `args`, as a plan read starts it: signed
    /// in as the agent's sessions are, with the user's settings for it.
    /// `None` when this node does not run it or the user turned it off.
    fn cli(&self, agent: AgentAccount, args: &[&str]) -> Option<tokio::process::Command> {
        let name = agent.as_ref();
        let kind = self.named(name)?;
        let kind = self.settings.configured(&kind).ok()?;
        let program = harnesses::find(name)?;
        let mut command = process::command(program);
        spawn_env::apply(&mut command, Scope::Agent(Some(agent)));
        command
            .args(args)
            .envs(&kind.env)
            .current_dir(env::temp_dir());
        Some(command)
    }

    /// Starts reading the plans into `usage`, unless a read runs. The read
    /// outlives the stream that asked: a desktop listens only briefly, and a
    /// CLI that answers after it left still has its answer kept for the next
    /// look. A read ends within `plans::READ_TIMEOUT`.
    pub(crate) fn read_plans_soon(self: &Arc<Self>) {
        if self.reading_plans.swap(true, Ordering::Relaxed) {
            return;
        }
        let agents = self.clone();
        tokio::spawn(async move {
            agents.read_plans().await;
            agents.reading_plans.store(false, Ordering::Relaxed);
        });
    }

    /// Reads the plans once the node starts, so the first desktop to look
    /// finds them read instead of waiting for Claude Code to start. It
    /// waits for the user's agent settings, which say how each CLI signs in,
    /// for `SETTINGS_WAIT` at most.
    pub(crate) async fn read_plans_at_start(self: &Arc<Self>) {
        let mut settled = self.settled.subscribe();
        if timeout(SETTINGS_WAIT, settled.wait_for(|settled| *settled))
            .await
            .is_err()
        {
            log::debug!("reading the plans without the agent settings");
        }
        self.read_plans_soon();
    }

    /// Reads how much of each agent's plan is left from the CLIs of the
    /// agents this node runs, into `usage` as each answers: a slow CLI
    /// does not hold back the other. One that cannot say is left out.
    async fn read_plans(&self) {
        let claude = self.cli(AgentAccount::Claude, plans::CLAUDE_ARGS);
        let codex = self.cli(AgentAccount::Codex, plans::CODEX_ARGS);
        let keep = |agent: AgentAccount, read: Result<Option<PlanUsage>>| match read {
            Ok(Some(plan)) => self.usage.set_plan(plan),
            Ok(None) => {}
            Err(e) => log::info!("{} plan usage not read: {e:#}", agent.as_ref()),
        };
        tokio::join!(
            async {
                if let Some(command) = claude {
                    keep(AgentAccount::Claude, plans::read_claude(command).await);
                }
            },
            async {
                if let Some(command) = codex {
                    keep(AgentAccount::Codex, plans::read_codex(command).await);
                }
            },
        );
    }

    /// Checks installed harness releases once an hour while the node runs.
    pub fn start_harness_checks(self: &Arc<Self>) -> tokio_util::task::AbortOnDropHandle<()> {
        let agents = self.clone();
        tokio_util::task::AbortOnDropHandle::new(tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(60 * 60));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                interval.tick().await;
                harnesses::refresh_versions(&agents.kinds(), &agents.latest).await;
            }
        }))
    }

    /// What this node finds of each agent harness it knows.
    pub(super) async fn harnesses(&self, refresh: bool) -> HarnessReport {
        harnesses::report(&self.kinds(), &self.settings, &self.latest, refresh).await
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

    /// The protocols to offer: one per agent, and `agents/1` always, so the
    /// sessions found on disk list even when no agent's program was found.
    pub fn offers(&self) -> Vec<String> {
        let mut offers: Vec<String> = self
            .kinds()
            .iter()
            .map(|kind| agent::protocol(&kind.name))
            .collect();
        offers.push(SESSIONS_PROTOCOL.to_owned());
        offers.push(agent::SESSION_FOLLOW_PROTOCOL.to_owned());
        offers.push(beep_proto::node::events::PROTOCOL.to_owned());
        offers
    }

    /// The agent `protocol` names, when this node has it.
    pub fn kind(&self, protocol: &str) -> Option<AgentKind> {
        let name = agent::agent_of(protocol)?;
        self.named(name)
    }

    /// The agent called `name`, when this node has it.
    pub(super) fn named(&self, name: &str) -> Option<AgentKind> {
        self.kinds().iter().find(|kind| kind.name == name).cloned()
    }

    /// The agents this node can start now.
    pub(super) fn kinds(&self) -> Arc<Vec<AgentKind>> {
        self.kinds.borrow().clone()
    }

    /// Takes the agents a new detection found, when they differ.
    pub(super) fn set_kinds(&self, kinds: Vec<AgentKind>) {
        let changed = self.kinds.send_if_modified(|current| {
            if **current == kinds {
                return false;
            }
            *current = Arc::new(kinds);
            true
        });
        if changed {
            let names: Vec<String> = self.kinds().iter().map(|kind| kind.name.clone()).collect();
            log::info!("coding agents now: {}", names.join(", "));
        }
    }

    /// Changes each time the agents this node can start change.
    pub(crate) fn kinds_changed(&self) -> watch::Receiver<Arc<Vec<AgentKind>>> {
        self.kinds.subscribe()
    }

    /// The session `id` names: by the id it started under or by the
    /// agent's own id.
    pub(super) fn get(&self, id: &str) -> Option<Arc<Session>> {
        find_session(&self.lock(), id)
    }

    /// Every named session but the reviewers, oldest first.
    pub(super) fn list(&self) -> Vec<SessionInfo> {
        self.listed(false)
    }

    /// `list`, with the reviewers that run as a child when `reviewers`.
    pub(super) fn listed(&self, reviewers: bool) -> Vec<SessionInfo> {
        let mut sessions: Vec<SessionInfo> = self
            .lock()
            .values()
            .filter_map(|s| {
                let live = s.live.borrow();
                live.agent_session.as_ref()?;
                if s.reviewer && !(reviewers && live.info.parent.is_some()) {
                    return None;
                }
                Some(SessionInfo {
                    reviewer: s.reviewer,
                    ..live.info.clone()
                })
            })
            .collect();
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
        }
        let prompt = begin.prompt.as_deref().unwrap_or_default();
        let place = checkout::place(cwd.map(str::to_owned), checkout, prompt).await?;
        begin.note = place.note;
        begin.brief = place.brief.map(Cow::Owned);
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
        let recovery_cwd = crate::files::paths::canonicalize(&cwd).unwrap_or_else(|_| cwd.clone());
        let recovering = crate::lock(&self.recovering);
        anyhow::ensure!(
            !recovering.iter().any(|root| recovery_cwd.starts_with(root)),
            "Git sync is preserving local edits in this checkout; try starting the session again shortly."
        );
        // A reviewer changes nothing, so it needs no git rule.
        let rule = checkout::rule(&cwd).filter(|_| !begin.review);
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
        // A new process needs a private handle before ACP names its session.
        // Only ACP's id is published in SessionInfo.
        let id = match &begin.resume {
            Some(resume) => {
                // The ended run may be keyed by its original private handle.
                // Remove it too, so listings cannot shadow the resumed run.
                sessions.retain(|handle, session| {
                    handle != resume && session.live.borrow().agent_session.as_ref() != Some(resume)
                });
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
        // A reviewer tells the user nothing itself: the review does.
        let alerts = if begin.review {
            Arc::default()
        } else {
            self.alerts.clone()
        };
        let mut session = Session::start(Start {
            id: id.clone(),
            kind,
            cwd: &cwd,
            prompt: begin.prompt.map(|text| Prompt {
                text,
                images: begin.images,
            }),
            resume: begin.resume,
            earlier: begin.earlier,
            timestamps: begin.timestamps,
            model: begin.model,
            mode: begin.mode,
            effort: begin.effort,
            parent: begin.parent,
            follows: begin.follows,
            note: begin.note,
            brief: begin.brief,
            rule,
            mcp: self.mcp.get().map(|launch| launch.server(&id)),
            images: self.images.clone(),
            usage: self.usage.clone(),
            alerts,
            node: NodeMode {
                stopping: self.stopping.clone(),
            },
        })?;
        session.reviewer = begin.review;
        let session = Arc::new(session);
        let how = if resumed { "took over" } else { "started" };
        log::info!("{how} {} session {id} in {}", kind.name, cwd.display());
        let events = self.events.clone();
        let mut updates = session.live.clone();
        session.keep(tokio::spawn(async move {
            let mut previous = None;
            loop {
                let info = updates.borrow_and_update().info.clone();
                events.changed(beep_proto::node::events::Topic::keyed("agent", &info.id));
                let summary = (
                    info.id.clone(),
                    info.state.status,
                    info.title.clone(),
                    info.turns,
                    info.state.tasks.clone(),
                );
                if previous.as_ref() != Some(&summary) {
                    events.changed(beep_proto::node::events::Topic::new("agents"));
                    events.changed(beep_proto::node::events::Topic::new("processes"));
                    previous = Some(summary);
                }
                if updates.changed().await.is_err() {
                    return;
                }
            }
        }));
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

/// How a session begins: on a prompt, and as a new session (on a model
/// and thinking level when given) or an earlier one resumed by the agent's
/// id.
#[derive(Default)]
pub(super) struct Begin {
    pub(super) prompt: Option<String>,
    /// Images that go with `prompt`.
    pub(super) images: Vec<Upload>,
    pub(super) resume: Option<String>,
    /// The resumed session's transcript from before the node restarted.
    pub(super) earlier: Vec<agent::Entry>,
    pub(super) timestamps: Option<(u64, u64)>,
    pub(super) model: Option<ModelId>,
    pub(super) effort: Option<agent::Effort>,
    /// The session mode it was in before the node restarted.
    pub(super) mode: Option<String>,
    /// A first line for the transcript, such as the branch it started on.
    pub(super) note: Option<String>,
    /// Beep's word to the agent ahead of the first prompt.
    pub(super) brief: Option<Cow<'static, str>>,
    /// The session that started this one with `start_agent`.
    pub(super) parent: Option<ParentLink>,
    /// The parent's state when it runs here.
    pub(super) follows: Option<watch::Receiver<Live>>,
    /// A temporary reviewer (`agents::reviews`).
    pub(super) review: bool,
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

/// Takes back the questions nobody waits for any more, once the wait that held it ends before an
/// answer. The order is short and ends with the session, so it runs on its own.
struct Withdraw(Option<Arc<Session>>);

impl Drop for Withdraw {
    fn drop(&mut self) {
        if let Some(session) = self.0.take() {
            tokio::spawn(async move {
                // Refused when the session already ended, which took its questions with it.
                let _ = session.order(Command::Withdraw).await;
            });
        }
    }
}
