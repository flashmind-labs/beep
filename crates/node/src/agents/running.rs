//! Starting, finding and stopping the sessions this node runs.

use crate::machine::env::locate::folder;
use super::{AgentKind, Agents, Answered, Asking, Session, checkout, harnesses, live::Sent, machine_list, models, projects, session::{Begin, Command, History, Start}, sessions::{self, new_id}};
use crate::{
    agents::child_reach,
    process::spawn_env,
};
use anyhow::{Context, Result};
use beep_proto::SessionId;
use beep_proto::node::{
    agent::{
        self, CheckoutPlan, ModelList, SESSIONS_PROTOCOL, SessionDevice, SessionInfo,
        Status,
    },
    harness::AgentSettings,
};
use beep_proto::watched;
use beep_proto::{AccountName, AgentAccount, AgentHost};
use std::time::Duration;
use std::{
    borrow::Cow,
    collections::HashMap,
    path::{Path, PathBuf},
    str::FromStr,
    sync::{Arc, atomic::Ordering},
};
use tokio::{
    sync::{oneshot, watch},
    task::spawn_blocking,
};

impl Agents {
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
                    .external()
                    .attach_children(&id, calls, entries.clone())
                    .await
                {
                    Ok(linked) => entries = linked,
                    Err(e) => log::debug!("native child conversations not linked: {e}"),
                }
            }
        }

        child_reach::show_children(self, session, &mut entries);
        let start = native + earlier.saturating_sub(dropped);
        if let Some(history) = history
            && earlier == dropped
            && entries.len() < last
        {
            let end = before.unwrap_or(native).min(native);
            if end > 0 {
                match self
                    .external()
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
                .external()
                .file(&id)
                .await?
                .context("native transcript not found")?;
            let (replay, dropped) = watched::read(&session.live, |live| {
                live.transcript_before(usize::MAX, None)
            });
            let native = self.external().prefix_len(path.clone(), replay).await?;
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
                let stores = self.stores().clone();
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

        let stores = self.stores().clone();
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

    /// The settings now in force.
    pub fn settings(&self) -> AgentSettings {
        self.catalog.settings.current()
    }

    /// The models the agent `name` can run on here; `None` when this node
    /// has no such agent.
    pub(crate) async fn models(&self, name: &str) -> Option<ModelList> {
        let kind = self.named(name)?;
        let kind = self
            .catalog
            .settings
            .configured(&kind)
            .unwrap_or_else(|_| kind.clone());
        let default_model = kind.default_model.clone();
        let mut list = self.catalog.models.get(name, models::find(kind)).await;
        if let Some(model) = default_model {
            list.current = Some(model.to_string());
        }
        Some(list)
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
                    harnesses::refresh_versions(&agents.kinds(), &agents.catalog.latest).await;
                }
            };
            let background_children = async {
                if let Some(closed) = agents.closed.take() {
                    agents.delete_closed_background(closed).await;
                }
            };
            tokio::join!(harness_checks, background_children);
        }))
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
        self.sessions.stopping.store(true, Ordering::Relaxed);
    }

    /// The sessions now, as last published.
    pub(crate) fn session_map(&self) -> Arc<sessions::Map> {
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
    pub(crate) fn named(&self, name: &str) -> Option<AgentKind> {
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
    pub(crate) fn kinds(&self) -> Arc<Vec<AgentKind>> {
        watched::read_sent(&self.catalog.kinds, Clone::clone)
    }

    /// Changes each time the agents this node can start change.
    pub(crate) fn kinds_changed(&self) -> watch::Receiver<Arc<Vec<AgentKind>>> {
        self.catalog.kinds.subscribe()
    }

    /// The session `id` names: by the id it started under or by the
    /// agent's own id.
    pub(crate) fn get(&self, id: &SessionId) -> Option<Arc<Session>> {
        sessions::find_session(&self.sessions.read(), id)
    }

    /// Listings also read Claude Code's current folder from its native
    /// transcript. Shell tools may change it without naming a structured cwd.
    pub(super) async fn listed_with_places(&self, reviewers: bool) -> Vec<SessionInfo> {
        let mut sessions = if reviewers {
            self.sessions.listed(true)
        } else {
            self.sessions.list()
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

        match self.external().places(owned.keys().cloned().collect()).await {
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
        self.sessions.listed_where(true, Some(id))
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
        self.catalog.prepare(kind).await?;
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

        self.occupancy.holds().check_start(&cwd)
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

    pub(crate) async fn start(
        &self,
        kind: &AgentKind,
        cwd: Option<&str>,
        mut begin: Begin,
    ) -> Result<Arc<Session>> {
        let kind = &self.catalog.settings.configured(kind)?;
        signs_in(&kind.name, begin.account.as_ref())?;
        let own = own_folder(cwd, &begin)?;
        let (cwd, brief) = in_project(own.as_deref().or(cwd), begin.resume.is_some(), begin.brief.take())?;
        let brief = match begin.instructions.take() {
            Some(instructions) => super::session::standing(brief.map(Cow::into_owned), Some(&instructions)).map(Cow::Owned),
            None => brief,
        };
        // Only the first start reads the earlier session; a resume has it.
        let brief = match begin.continues.as_ref().filter(|_| begin.resume.is_none()) {
            Some(earlier) => {
                if begin.title.is_none()
                    && let Some(title) = self
                        .get(earlier)
                        .and_then(|session| session.info().title)
                        .or_else(|| self.reopen.saved(earlier).and_then(|saved| saved.info.title))
                        .filter(|title| !title.trim().is_empty())
                {
                    begin.title = Some(title);
                    begin.named = true;
                }
                let said = super::brief::continues(earlier);
                Some(Cow::Owned(match brief {
                    Some(brief) => format!("{brief}\n\n{said}"),
                    None => said,
                }))
            }
            None => brief,
        };
        let team = self
            .mesh()
            .and_then(|mesh| crate::membership::team_repo_of(&mesh, &cwd));
        let brief = match team {
            Some(_) => Some(Cow::Owned(match brief {
                Some(brief) => format!("{brief}\n\n{}", super::brief::TEAM),
                None => super::brief::TEAM.to_owned(),
            })),
            None => brief,
        };

        // Only a new session's first prompt names its issue.
        if begin.resume.is_none()
            && begin.issue.is_none()
            && let (Some(repo), Some(prompt)) = (&team, &begin.prompt)
        {
            begin.issue = super::start::issue_of_prompt(repo, prompt);
        }

        if let Some(mesh) = self.mesh() {
            projects::may_run(&mesh, &cwd, &kind.name).map_err(anyhow::Error::msg)?;
        }

        let resume = begin.resume.clone();
        // The id is taken first, so no other start takes it while this one
        // spawns. Recovery reads the starts under way.
        let reservation = self.sessions.reserve(resume.clone())?;
        let id = reservation.id.clone();
        let running = self.occupancy.holds().running(cwd.clone());
        self.occupancy.holds().check_start(&cwd)?;
        // A reviewer changes nothing, so it needs no git rule.
        let rule = checkout::rule(&cwd).filter(|_| !begin.review).map(str::to_owned);
        let instructions = crate::lock(&self.catalog.instructions)
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
                .map(|mesh| machine_list::machine_name(Some(mesh)))
                .filter(|name| !name.is_empty()),
            project: mesh.as_ref().and_then(|mesh| {
                projects::of_folder(mesh, &cwd.to_string_lossy(), mesh.me())
            }),
            multi_machine: mesh.as_ref().is_some_and(|mesh| !machine_list::alone(mesh)),
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
                .catalog
                .skills_dir
                .as_deref()
                .and_then(crate::machine::provision::skills_brief),
            mcp: self.catalog.mcp.as_ref().map(|launch| launch.server(&id)),
            context,
            images: self.images.clone(),
            usage: self.usage.hub.clone(),
            alerts,
            checkouts: self.checkouts.clone(),
            stopping: self.sessions.stopping.clone(),
            running: Some(running),
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
                    crate::events::open_changed();
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
        crate::events::open_changed();
        Ok(session)
    }
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
    pub(crate) fn child_folder(&self, parent: &Session, title: Option<&str>) -> Result<Option<PathBuf>> {
        let Some(home) = dirs::home_dir() else {
            return Ok(None);
        };

        super::conversation::thread_folder(&home, Path::new(&parent.info().cwd), title, new_id)
            .context("making the thread's folder")
    }
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
