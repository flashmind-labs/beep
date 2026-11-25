//! Starting, finding and stopping the sessions this node runs.

use super::{
    AgentKind, Agents, Answered, Asking, Command, Session, Start, checkout,
    harnesses,
    live::{Live, Sent},
    models, plans, sessions,
};
use crate::{
    agents::mcp::{self, Launch},
    machine::provision::spawn_env::{self, Scope},
    run::process,
};
use anyhow::{Context, Result};
use beep_proto::SessionId;
use beep_proto::node::{
    agent::{
        self, CheckoutPlan, ModelList, ParentLink, Persona, SESSIONS_PROTOCOL, SessionDevice, SessionInfo,
        Status,
    },
    harness::{AgentSettings, HarnessReport},
    usage::PlanUsage,
};
use beep_proto::watched;
use beep_proto::{AccountName, AgentAccount, AgentHost, AgentSlug, ModelId, Upload};
use std::time::Duration;
use std::{
    borrow::Cow,
    collections::{HashMap, HashSet, hash_map::RandomState},
    env,
    hash::BuildHasher,
    path::{Component, Path, PathBuf},
    str::FromStr,
    sync::{Arc, atomic::Ordering},
};
use futures::{StreamExt, stream::FuturesUnordered};
use tokio::{
    sync::{oneshot, watch},
    task::spawn_blocking,
};

#[derive(Clone)]
pub(super) struct History {
    path: PathBuf,
    native: u64,
    dropped: u64,
}

impl Agents {
    /// Gives the sessions started from now on the Beep MCP server, as
    /// `launch` starts it. Only the first call counts.
    pub fn serve_mcp(&self, launch: Arc<Launch>) {
        // A second launch would give the same server.
        let _ = self.mcp.set(launch);
    }

    /// Lists the skills in Beep's own skills folder for the sessions
    /// started from now on, as a node that serves does. Only the first call
    /// counts.
    pub fn list_beep_skills(&self) {
        if let Some(dir) = crate::machine::provision::skills_dir() {
            let _ = self.skills_dir.set(dir);
        }
    }

    /// Whether `session` is one of this node's and still runs.
    pub(crate) fn runs(&self, session: &SessionId) -> bool {
        self.get(session).is_some_and(|session| {
            watched::read(&session.live, |live| live.status() != Status::Ended)
        })
    }

    /// The folder `session` works in, if it is one of this node's.
    pub(crate) fn folder_of(&self, session: &SessionId) -> Option<PathBuf> {
        self.get(session)
            .map(|session| PathBuf::from(session.info().cwd))
    }

    /// The folders this node's sessions work in, the ended ones it keeps too.
    pub(crate) fn folders(&self) -> std::collections::BTreeSet<PathBuf> {
        let sessions: Vec<Arc<Session>> = self.session_map().values().cloned().collect();

        sessions
            .iter()
            .map(|session| PathBuf::from(session.info().cwd))
            .collect()
    }

    /// Keeps `device` as the device `session` uses, or forgets it.
    pub(crate) async fn set_device(
        &self,
        session: &SessionId,
        device: Option<SessionDevice>,
    ) -> Result<()> {
        let session = self
            .get(session)
            .with_context(|| format!("there is no session {session} here"))?;
        session
            .order(Command::SetDevice(device))
            .await
            .map_err(anyhow::Error::msg)
    }

    /// Shows in `session` how far its running `send_files` call has sent.
    pub(crate) async fn sending(&self, session: &SessionId, sent: Sent) -> Result<()> {
        let session = self
            .get(session)
            .with_context(|| format!("there is no session {session} here"))?;
        session
            .order(Command::Sending(sent))
            .await
            .map_err(anyhow::Error::msg)
    }

    /// Puts `question` to the user in `session` and waits for the answer.
    /// Fails when the session ends or its turn is cancelled first.
    pub(crate) async fn ask(&self, session: &SessionId, question: Asking) -> Result<Answered> {
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
        let history = self.claude_history(session, last, before).await;
        let (native, dropped) = history.as_ref().map_or((0, 0), |h| (h.native, h.dropped));
        let live_before = before.map(|end| end.saturating_sub(native).saturating_add(dropped));
        let (mut entries, earlier) = self.codex_transcript(session, last, live_before).await;

        if session.agent == "claude" {
            let (id, calls) = {
                watched::read(&session.live, |live| {
                    (
                        live.agent_session.clone(),
                        live.claude_children(earlier, entries.len()),
                    )
                })
            };

            if let Some(id) = id
                && !calls.is_empty()
            {
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
        let start = native + earlier.saturating_sub(dropped);
        if let Some(history) = history
            && earlier == dropped
            && entries.len() < last
        {
            let end = before.unwrap_or(native).min(native);
            if end > 0 {
                match self
                    .external
                    .page(history.path, last - entries.len(), Some(end))
                    .await
                {
                    Ok(mut page) => {
                        page.entries.extend(entries);
                        return (page.entries, page.earlier);
                    }
                    Err(error) => log::warn!("native history page not recovered: {error:#}"),
                }
            } else if entries.is_empty() {
                return (entries, 0);
            }
        }

        (entries, start)
    }

    async fn claude_history(
        &self,
        session: &Session,
        last: usize,
        before: Option<u64>,
    ) -> Option<History> {
        let mut cached = session.history.as_ref()?.lock().await;
        let (id, (dropped, retained)) = watched::read(&session.live, |live| {
            (
                live.agent_session
                    .clone()
                    .filter(|_| live.status() != Status::Starting),
                live.retained_range(),
            )
        });
        let id = id?;
        // Fresh sessions have a private handle until the agent assigns an id.
        // Their entire conversation is already live until the first eviction.
        if session.id != id && dropped == 0 {
            return None;
        }
        if let Some(history) = cached
            .as_ref()
            && (history.dropped == dropped || (before.is_none() && last <= retained))
        {
            return Some(history.clone());
        }

        // Realign after live entries were evicted only when a page needs disk
        // history. Newest-page polls can keep their existing position.
        let result: Result<History> = async {
            let path = self
                .external
                .file(&id)
                .await?
                .context("native transcript not found")?;
            let (replay, dropped) = watched::read(&session.live, |live| {
                live.transcript_before(usize::MAX, None)
            });
            let native = self.external.prefix_len(path.clone(), replay).await?;
            Ok(History {
                path,
                native,
                dropped,
            })
        }
        .await;
        match result {
            Ok(history) => {
                *cached = Some(history.clone());
                Some(history)
            }
            Err(error) => {
                log::warn!("native history not recovered: {error:#}");
                None
            }
        }
    }

    async fn codex_transcript(
        &self,
        session: &Session,
        last: usize,
        before: Option<u64>,
    ) -> (Vec<agent::Entry>, u64) {
        if session.agent == "codex" {
            let parent = watched::read(&session.live, |live| live.agent_session.clone());
            if let Some(parent) = parent {
                let stores = self.stores.clone();
                match spawn_blocking(move || stores.codex_children(&parent)).await {
                    Ok(runs) if !runs.is_empty() => {
                        let _ = session.order(Command::RecoverCodexChildren(runs)).await;
                    }
                    Err(error) => log::debug!("Codex child discovery failed: {error}"),
                    _ => {}
                }
            }
        }

        let (mut entries, earlier) =
            watched::read(&session.live, |live| live.transcript_before(last, before));
        let children = watched::read(&session.live, |live| live.codex_children(earlier));

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

        watched::read(&session.live, |live| {
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
        });
        (entries, earlier)
    }

    /// Applies the saved Beep instructions to sessions started or resumed from now on.
    pub(crate) fn set_instructions(&self, text: Option<String>) {
        *crate::lock(&self.instructions) = text;
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

    /// The CLI of agent `name`, with `args`, as a plan read starts it: signed
    /// in as the agent's sessions on `account` are, with the user's settings
    /// for it. `None` when this node does not run it or the user turned it off.
    fn cli(
        &self,
        name: &str,
        account: Option<&AccountName>,
        args: &[&str],
    ) -> Option<tokio::process::Command> {
        let kind = self.named(name)?;
        let kind = self.settings.configured(&kind).ok()?;
        let program = harnesses::find(name)?;
        let mut command = process::command(program);
        spawn_env::apply(&mut command, Scope::for_agent(name, account));
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
        if !self.settings_came().await {
            log::debug!("reading the plans without the agent settings");
        }

        self.read_plans_soon();
    }

    /// Reads how much of each plan is left from the CLIs of the agents this
    /// node runs, once per account, into `usage` as each answers: a slow CLI
    /// does not hold back the others. One that cannot say is left out.
    async fn read_plans(&self) {
        let reads = [AgentAccount::Claude, AgentAccount::Codex]
            .into_iter()
            .flat_map(|agent| {
                let named = spawn_env::accounts_of(agent).into_iter().map(Some);
                std::iter::once(None)
                    .chain(named)
                    .map(move |account| (agent, account))
            })
            .map(|(agent, account)| self.read_plan(agent, account))
            .collect::<FuturesUnordered<_>>();
        tokio::join!(reads.collect::<Vec<()>>(), self.read_agy_plan());
    }

    /// Reads Antigravity's plan into `usage`; it has no named accounts.
    async fn read_agy_plan(&self) {
        let name = AgentSlug::AGY;
        let Some(command) = self.cli(name.as_ref(), None, plans::AGY_ARGS) else {
            return;
        };

        self.keep_plan(name.as_ref(), None, plans::read_agy(command).await);
    }

    /// Reads `agent`'s plan on `account` into `usage`.
    async fn read_plan(&self, agent: AgentAccount, account: Option<AccountName>) {
        let args = match agent {
            AgentAccount::Claude => plans::CLAUDE_ARGS,
            AgentAccount::Codex => plans::CODEX_ARGS,
        };
        let Some(command) = self.cli(agent.as_ref(), account.as_ref(), args) else {
            return;
        };
        let read = match agent {
            AgentAccount::Claude => plans::read_claude(command).await,
            AgentAccount::Codex => plans::read_codex(command).await,
        };

        self.keep_plan(agent.as_ref(), account, read);
    }

    /// Keeps `name`'s plan read on `account` in `usage`.
    fn keep_plan(&self, name: &str, account: Option<AccountName>, read: Result<Option<PlanUsage>>) {
        match read {
            Ok(Some(plan)) => self.usage.set_plan(PlanUsage { account, ..plan }),
            Ok(None) => {}
            Err(e) => log::info!("{name} plan usage not read: {e:#}"),
        }
    }

    /// While the node runs: checks installed harness releases once an hour
    /// and deletes background children as they close.
    pub fn start_upkeep(self: &Arc<Self>) -> tokio_util::task::AbortOnDropHandle<()> {
        let agents = self.clone();
        tokio_util::task::AbortOnDropHandle::new(tokio::spawn(async move {
            let harness_checks = async {
                let mut interval = tokio::time::interval(Duration::from_secs(60 * 60));
                interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

                loop {
                    interval.tick().await;
                    harnesses::refresh_versions(&agents.kinds(), &agents.latest).await;
                }
            };
            let background_children = async {
                if let Some(closed) = agents.family.take_closed() {
                    agents.delete_closed_background(closed).await;
                }
            };
            tokio::join!(harness_checks, background_children);
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
            .session_map()
            .values()
            .filter(|session| watched::read(&session.live, |live| live.status() != Status::Ended))
            .cloned()
            .collect();
        futures::future::join_all(running.iter().map(|session| session.close())).await;
        self.lsp.shutdown().await;
    }

    /// Marks the node as stopping: sessions that end from now on ended
    /// with it, even when the service manager stopped their agents first.
    pub fn begin_stopping(&self) {
        self.stopping.store(true, Ordering::Relaxed);
    }

    /// The sessions now, as last published.
    pub(super) fn session_map(&self) -> Arc<sessions::Map> {
        self.sessions.read()
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

    /// The agent called `name`, or why this node cannot start it.
    pub(super) fn kind_or_refusal(&self, name: &str) -> Result<AgentKind, String> {
        let kinds = self.kinds();

        kinds
            .iter()
            .find(|kind| kind.name == name)
            .cloned()
            .ok_or_else(|| {
                let have: Vec<&str> = kinds.iter().map(|kind| kind.name.as_str()).collect();
                format!(
                    "This machine has no {name} agent. It has: {}.",
                    have.join(", ")
                )
            })
    }

    /// The agents this node can start now.
    pub(super) fn kinds(&self) -> Arc<Vec<AgentKind>> {
        watched::read_sent(&self.kinds, Clone::clone)
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
    pub(super) fn get(&self, id: &SessionId) -> Option<Arc<Session>> {
        find_session(&self.session_map(), id)
    }

    /// Every named session but the reviewers, oldest first.
    pub(super) fn list(&self) -> Vec<SessionInfo> {
        self.listed(false)
    }

    /// `list`, with the reviewers that run as a child when `reviewers`.
    pub(super) fn listed(&self, reviewers: bool) -> Vec<SessionInfo> {
        self.listed_where(reviewers, None)
    }

    /// Listings also read Claude Code's current folder from its native
    /// transcript. Shell tools may change it without naming a structured cwd.
    pub(super) async fn listed_with_places(&self, reviewers: bool) -> Vec<SessionInfo> {
        let mut sessions = if reviewers {
            self.listed(true)
        } else {
            self.list()
        };
        let owned: HashMap<SessionId, SessionId> = sessions
            .iter()
            .filter(|info| info.agent == "claude")
            .filter_map(|info| {
                let session = self.get(&info.id)?;
                let id = watched::read(&session.live, |live| live.agent_session.clone())?;
                Some((id, info.id.clone()))
            })
            .collect();

        if owned.is_empty() {
            return sessions;
        }

        match self.external.places(owned.keys().cloned().collect()).await {
            Ok(found) => {
                for (id, place) in found {
                    if let Some(info) = sessions
                        .iter_mut()
                        .find(|info| owned.get(&id) == Some(&info.id))
                    {
                        info.place = Some(place);
                    }
                }
            }
            Err(e) => log::debug!("native session folders not read: {e:#}"),
        }

        sessions
    }

    /// The entries of the full list whose id is `id`, without copying the others.
    pub(crate) fn listed_session(&self, id: &SessionId) -> Vec<SessionInfo> {
        self.listed_where(true, Some(id))
    }

    fn listed_where(&self, reviewers: bool, id: Option<&SessionId>) -> Vec<SessionInfo> {
        let mut sessions: Vec<SessionInfo> = self
            .session_map()
            .values()
            .filter_map(|s| {
                // One read: a second while this one is held waits behind any
                // write queued in between, which waits on this one, and the
                // node stops.
                let info = watched::read(&s.live, |live| {
                    (live.agent_session.is_some()
                        && id.is_none_or(|id| live.info.id == *id)
                        && (!s.reviewer || reviewers && live.info.parent.is_some()))
                    .then(|| live.info.clone())
                })?;

                Some(s.listed(info))
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
        self.admits(kind, cwd, &begin)?;

        let prompt = begin.prompt.as_deref().unwrap_or_default();
        let place = checkout::place(cwd.map(str::to_owned), checkout, prompt).await?;
        begin.note = place.note;
        begin.brief = place.brief.map(Cow::Owned);
        begin.worktrees.extend(place.worktree);
        self.start(kind, place.folder.as_deref(), begin).await
    }

    /// Refuses what `start` refuses of a session asked to run in `cwd`,
    /// beyond what `prepare` refuses, so that `start_in` makes nothing for
    /// it: an account `kind` lacks here, or a checkout git sync is
    /// preserving local edits in.
    fn admits(&self, kind: &AgentKind, cwd: Option<&str>, begin: &Begin) -> Result<()> {
        signs_in(&kind.name, begin.account.as_ref())?;
        let (cwd, _) = in_project(cwd, begin.resume.is_some(), None)?;

        not_recovering(&crate::lock(&self.recovering), &cwd)
    }

    /// The harness Auto starts here, as a desktop picks one when the start
    /// names none.
    pub(crate) fn auto_harness(&self) -> Option<AgentKind> {
        let kinds = self.kinds();
        let host = AgentHost {
            machine_id: String::new(),
            machine_name: String::new(),
            agents: kinds.iter().map(|kind| kind.name.clone()).collect(),
            workspace: None,
            home: None,
            capabilities: Vec::new(),
        };
        let name = beep_mesh::machine::pick_agent(&host, None).ok()?;

        kinds.iter().find(|kind| kind.name == name).cloned()
    }

    /// Starts an issue's automatic session: the `auto_harness`, on the
    /// worktree `plan` makes in `cwd`, titled `title`. Its id, once the
    /// agent names it as listings and cards do, and the worktree it works
    /// in (the session itself runs in the project folder).
    pub(crate) async fn start_on_issue(
        &self,
        cwd: &str,
        plan: CheckoutPlan,
        title: String,
        prompt: String,
    ) -> Result<(SessionId, String)> {
        let kind = self
            .auto_harness()
            .context("this machine has no coding agent")?;
        let begin = Begin {
            prompt: Some(prompt),
            title: Some(title),
            ..Begin::default()
        };
        let session = self.start_in(&kind, Some(cwd), Some(plan), begin).await?;
        let named = session.named_info().await.map_err(anyhow::Error::msg)?;

        let worktree = named.worktrees.into_iter().next();

        Ok((named.id, worktree.unwrap_or(named.cwd)))
    }

    /// Readies what `kind` reads before it starts: for flsh, the user's
    /// flsh settings in its config.
    pub(super) async fn prepare(&self, kind: &AgentKind) -> Result<()> {
        let configured = self.settings.configured(kind)?;
        self.settings.prepare(&configured).await
    }

    pub(super) async fn start(
        &self,
        kind: &AgentKind,
        cwd: Option<&str>,
        mut begin: Begin,
    ) -> Result<Arc<Session>> {
        let kind = &self.settings.configured(kind)?;
        signs_in(&kind.name, begin.account.as_ref())?;
        let own = own_folder(cwd, &begin)?;
        let (cwd, brief) = in_project(own.as_deref().or(cwd), begin.resume.is_some(), begin.brief.take())?;
        let brief = match begin.instructions.take() {
            Some(instructions) => super::session::standing(brief.map(Cow::into_owned), Some(&instructions)).map(Cow::Owned),
            None => brief,
        };
        let brief = match self.mesh().is_some_and(|mesh| crate::team::home::in_team(&mesh, &cwd)) {
            true => Some(Cow::Owned(match brief {
                Some(brief) => format!("{brief}\n\n{}", super::brief::TEAM),
                None => super::brief::TEAM.to_owned(),
            })),
            false => brief,
        };

        if let Some(mesh) = self.mesh() {
            mcp::projects::may_run(&mesh, &cwd, &kind.name).map_err(anyhow::Error::msg)?;
        }

        let resume = begin.resume.clone();
        // The id is taken first, so no other start takes it while this one
        // spawns. Recovery reads the starts under way.
        let reservation = self.sessions.reserve(resume.clone(), cwd.clone())?;
        let id = reservation.id.clone();
        not_recovering(&crate::lock(&self.recovering), &cwd)?;
        // A reviewer changes nothing, so it needs no git rule.
        let rule = checkout::rule(&cwd).filter(|_| !begin.review).map(str::to_owned);
        let instructions = crate::lock(&self.instructions)
            .clone()
            .filter(|text| !text.trim().is_empty());
        let resumed = begin.resume.is_some();
        // A reviewer tells the user nothing itself: the review does.
        let alerts = if begin.review {
            Arc::default()
        } else {
            self.alerts.clone()
        };
        let mesh = self.mesh();
        let context = super::brief::Context {
            machine: mesh
                .as_ref()
                .map(|mesh| mcp::children::machine_name(Some(mesh)))
                .filter(|name| !name.is_empty()),
            project: mesh.as_ref().and_then(|mesh| {
                mcp::projects::of_folder(mesh, &cwd.to_string_lossy(), mesh.me())
            }),
            multi_machine: mesh.as_ref().is_some_and(|mesh| !mcp::alone(mesh)),
            rust_or_c: super::brief::has_native_build(&cwd),
        };
        begin.brief = brief;
        let review = begin.review;
        // A WSL agent cannot reach a socket on Windows.
        let docker = match !super::kinds::in_wsl(kind) && docker_containers(mesh.as_ref()) {
            true => super::processes::Docker::open(kind.env.get("DOCKER_HOST").map(String::as_str)).await,
            false => None,
        };
        let mut session = Session::start(Start {
            id: id.clone(),
            kind,
            cwd: &cwd,
            begin,
            instructions,
            rule,
            skills: self
                .skills_dir
                .get()
                .and_then(|dir| crate::machine::provision::skills_brief(dir)),
            mcp: self.mcp.get().map(|launch| launch.server(&id)),
            context,
            images: self.images.clone(),
            usage: self.usage.clone(),
            alerts,
            checkouts: self.checkouts.clone(),
            stopping: self.stopping.clone(),
            docker,
        })?;
        session.reviewer = review;
        let session = Arc::new(session);
        let how = if resumed { "took over" } else { "started" };
        log::info!("{how} {} session {id} in {}", kind.name, cwd.display());
        let events = self.events.clone();
        let mut updates = session.live.clone();
        session.keep(tokio::spawn(async move {
            let mut previous = None;
            let mut saved = None;

            loop {
                let (info, effort, fast) = watched::read_and_update(&mut updates, |live| {
                    (live.info.clone(), live.effort, live.fast)
                });
                // What the saved session keeps, noted as it changes.
                let keeps = (
                    info.id.clone(),
                    info.title.clone(),
                    info.model.clone(),
                    effort,
                    fast,
                    info.mode.clone(),
                    info.device.clone(),
                    info.worktrees.clone(),
                );

                if saved.as_ref() != Some(&keeps) {
                    crate::link::restart::changed();
                    saved = Some(keeps);
                }
                events.changed(beep_proto::node::events::Topic::keyed("agent", &info.id));
                let summary = (
                    info.id.clone(),
                    info.state.status,
                    info.title.clone(),
                    info.turns,
                    info.state.tasks.clone(),
                    info.place.clone(),
                );

                if previous.as_ref() != Some(&summary) {
                    events.changed(beep_proto::node::events::Topic::new("agents"));
                    // An ended session lists no processes; its watch reports the rest.
                    let ended = |status| status == Status::Ended;

                    if previous
                        .as_ref()
                        .is_none_or(|(_, status, ..)| ended(*status) != ended(info.state.status))
                    {
                        events.changed(beep_proto::node::events::Topic::new("processes"));
                    }

                    previous = Some(summary);
                }

                if updates.changed().await.is_err() {
                    return;
                }
            }
        }));
        self.sessions.insert(reservation, resume, session.clone());
        crate::link::restart::changed();
        Ok(session)
    }
}

/// The session `id` names in `sessions`, by its key or the agent's id.
fn find_session(
    sessions: &HashMap<SessionId, Arc<Session>>,
    id: &SessionId,
) -> Option<Arc<Session>> {
    if let Some(found) = sessions.get(id) {
        return Some(found.clone());
    }

    sessions
        .values()
        .filter(|session| {
            watched::read(&session.live, |live| {
                live.agent_session.as_ref() == Some(id)
            })
        })
        .max_by_key(|session| {
            watched::read(&session.live, |live| {
                (live.status() != Status::Ended, live.info.started_at)
            })
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
    pub(super) resume: Option<SessionId>,
    /// The resumed session's transcript from before the node restarted.
    pub(super) earlier: Vec<agent::Entry>,
    pub(super) timestamps: Option<(u64, u64)>,
    pub(super) title: Option<String>,
    /// Its parent gave `title`, which its agent's own names leave alone.
    pub(super) named: bool,
    pub(super) model: Option<ModelId>,
    pub(super) effort: Option<agent::Effort>,
    pub(super) fast: Option<bool>,
    /// The session mode it was in before the node restarted.
    pub(super) mode: Option<String>,
    /// A first line for the transcript, such as the branch it started on.
    pub(super) note: Option<String>,
    /// Beep's word to the agent ahead of the first prompt.
    pub(super) brief: Option<Cow<'static, str>>,
    /// The linked worktrees it created, kept across restarts and moves.
    pub(super) worktrees: Vec<String>,
    /// The session that started this one with `start_agent`.
    pub(super) parent: Option<ParentLink>,
    /// The parent's state when it runs here.
    pub(super) follows: Option<watch::Receiver<Live>>,
    /// A temporary reviewer (`agents::reviews`).
    pub(super) review: bool,
    /// The home assistant, as asked for (Cmd+N on Home) or as saved.
    pub(super) assistant: bool,
    /// The named account to sign in as; `None` for the default one.
    pub(super) account: Option<AccountName>,
    /// The custom agent it runs as.
    pub(super) persona: Option<Persona>,
    /// The custom agent's instructions, said once after Beep's brief.
    pub(super) instructions: Option<String>,
}

/// Refuses a start as `account` that this node cannot sign `agent` in as,
/// rather than run it on another account.
fn signs_in(agent: &str, account: Option<&AccountName>) -> Result<()> {
    let Some(account) = account else {
        return Ok(());
    };
    let Ok(keeps) = AgentAccount::from_str(agent) else {
        anyhow::bail!("{agent} has no named accounts; start it on its default account");
    };
    anyhow::ensure!(
        spawn_env::has_account(keeps, account),
        "this machine does not have {agent}'s account {account} yet; try again once it has synced, or add it in Agent accounts"
    );
    Ok(())
}

/// Refuses a session in `cwd` while git sync preserves the local edits of
/// its checkout, one of `recovering`.
fn not_recovering(recovering: &HashSet<PathBuf>, cwd: &Path) -> Result<()> {
    let real = crate::files::paths::canonicalize(cwd).unwrap_or_else(|_| cwd.to_owned());
    anyhow::ensure!(
        !recovering.iter().any(|root| real.starts_with(root)),
        "Git sync is preserving local edits in this checkout; try starting the session again shortly."
    );

    Ok(())
}

/// The folder a session asked to run in `cwd` runs in, with Beep's brief
/// to it: the project folder when `cwd` is in a linked worktree, even a
/// removed one, so the session outlives its worktrees. The brief then names
/// the worktree, unless one came already.
fn in_project(
    cwd: Option<&str>,
    resumed: bool,
    brief: Option<Cow<'static, str>>,
) -> Result<(PathBuf, Option<Cow<'static, str>>)> {
    let asked = folder(cwd).or_else(|e| {
        cwd.map(PathBuf::from)
            .filter(|gone| checkout::project_of(gone).is_some())
            .ok_or(e)
    })?;
    let Some(project) = checkout::project_of(&asked) else {
        return Ok((asked, brief));
    };

    let (project_shown, tree) = (project.display(), asked.display());
    let said = match (resumed, asked.exists()) {
        (false, _) => format!(
            "This session runs in the project folder {project_shown}. Work and commit in the worktree {tree}."
        ),
        (true, true) => format!(
            "This session now runs in the project folder {project_shown}. Its work so far is in the worktree {tree}; continue there."
        ),
        (true, false) => format!(
            "This session now runs in the project folder {project_shown}. The worktree it worked in, {tree}, was removed."
        ),
    };
    Ok((project, brief.or(Some(Cow::Owned(said)))))
}

/// The folder of its own a new assistant conversation starts in, which its
/// threads share. `None` for every other start.
/// Whether new sessions list the Docker containers they start: the shared
/// setting, on until turned off, and on without a mesh.
fn docker_containers(mesh: Option<&beep_mesh::Mesh>) -> bool {
    use beep_proto::node::mesh_state::{DockerSettings, StateValue};

    mesh.and_then(|mesh| mesh.get(&beep_mesh::Key::Docker))
        .and_then(|bytes| DockerSettings::from_bytes(&bytes).ok())
        .and_then(|settings| settings.containers)
        .unwrap_or(true)
}

fn own_folder(cwd: Option<&str>, begin: &Begin) -> Result<Option<String>> {
    if !begin.assistant || begin.resume.is_some() {
        return Ok(None);
    }

    let home = dirs::home_dir().context("this machine has no home folder")?;
    let made = super::conversation::own(&home, cwd, new_id)
        .context("making the conversation's folder")?;
    Ok(made.map(|folder| folder.to_string_lossy().into_owned()))
}

impl Agents {
    /// The subfolder of its own, made now, that a child called `title`
    /// starts in when its parent names none and `parent` is in an assistant
    /// conversation's folder. `None` anywhere else: it starts in the parent's.
    pub(super) fn child_folder(&self, parent: &Session, title: Option<&str>) -> Result<Option<PathBuf>> {
        let Some(home) = dirs::home_dir() else {
            return Ok(None);
        };

        super::conversation::thread_folder(&home, Path::new(&parent.info().cwd), title, new_id)
            .context("making the thread's folder")
    }
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
pub(super) fn new_id() -> SessionId {
    let random = RandomState::new().hash_one(std::time::SystemTime::now());
    SessionId::new(format!("{:06x}", random & 0xff_ffff))
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
