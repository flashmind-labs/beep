//! Sessions and folders found in the agents' own logs and on disk.

use super::{
    AgentKind, Agents, Begin, Command, DEFAULT_FOLDERS, DEFAULT_IN_FOLDERS, MAX_ASKED_FOLDERS,
    MAX_FOLDERS, MAX_IN_FOLDERS, Picture, SIGHTED_FOR, Session, Sighted,
    holders::{self, Holding},
};
use crate::{
    agents::claude::{self, External},
    agents::found::{self, Agent},
};
use anyhow::{Context, Result};
use beep_proto::node::{
    agent::{
        ExternalAsk, ExternalSession, MAX_EXTERNAL_NAMED, ParentLink, Presence, SessionFolder,
        Status,
    },
    events::Topic,
};
use beep_proto::{ModelId, watched};
use std::{
    cmp::Reverse,
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{Arc, atomic::Ordering},
    time::{Duration, SystemTime},
};
use tokio::{task::spawn_blocking, time::Instant};

/// How long `close_external` waits for the process to end.
const CLOSE_WAIT: Duration = Duration::from_secs(8);
/// How long a session it closed reads as stopped despite a recent write:
/// as long as such a write counts.
const CLOSED_FOR: Duration = Duration::from_secs(3 * 60);

/// The last folder listing, for the asks that waited while it ran.
pub(super) struct Listed {
    /// When it ended.
    at: Instant,
    limit: usize,
    folders: Arc<Vec<SessionFolder>>,
}

/// What a session of this node that ended was held to.
#[derive(Default)]
pub(super) struct Held {
    pub(super) parent: Option<ParentLink>,
    pub(super) mode: Option<String>,
    pub(super) model: Option<ModelId>,
    pub(super) worktrees: Vec<String>,
    pub(super) assistant: bool,
}

/// How many of a store's sessions one listing keeps.
fn store_limit(limit: u32) -> usize {
    usize::try_from(limit)
        .unwrap_or(MAX_IN_FOLDERS)
        .clamp(1, MAX_IN_FOLDERS)
}

impl Agents {
    /// Reads the checkouts saved in the node's folder `dir` and the agents'
    /// logs ahead of the first requests, then walks `root` so checkouts made
    /// or removed since count. The saved list answers during the walk; with
    /// none saved, `disk_checkouts` refuses until it ends.
    pub async fn warm(&self, root: Option<PathBuf>, dir: PathBuf) {
        let mut on_disk = self.on_disk.lock().await;
        let Some(root) = root else {
            *on_disk = Some(Arc::default());
            drop(on_disk);
            self.read_logs_ahead().await;
            return;
        };
        let reading = dir.clone();
        let saved = spawn_blocking(move || found::saved_checkouts(&reading))
            .await
            .unwrap_or_else(|e| {
                log::warn!("the saved checkouts were not read: {e}");
                None
            });
        let first_walk = match saved {
            Some(saved) => {
                *on_disk = Some(Arc::new(saved));
                drop(on_disk);
                None
            }
            None => Some(on_disk),
        };
        self.read_logs_ahead().await;
        let walk = match spawn_blocking(move || found::walk(&root, &dir)).await {
            Ok(walk) => walk,
            Err(e) => {
                log::warn!("the checkouts on disk were not walked: {e}");
                return;
            }
        };

        match first_walk {
            Some(mut on_disk) => *on_disk = Some(Arc::new(walk.found)),
            None if walk.whole => *self.on_disk.lock().await = Some(Arc::new(walk.found)),
            None => log::info!("the walk for checkouts was cut short; kept the saved list"),
        }
    }

    /// Walks `root` again, quickly and not deep, so a checkout made since the last walk
    /// counts, and saves the list in `dir`. Nothing while the first walk runs or another
    /// rescan does: what that one finds serves this one too.
    pub async fn rescan(&self, root: PathBuf, dir: PathBuf) -> Result<()> {
        if self.rescanning.swap(true, Ordering::Relaxed) {
            return Ok(());
        }

        let walked = async {
            let known = self.disk_checkouts()?;
            let all = spawn_blocking(move || found::rescan(&root, &dir, &known))
                .await
                .context("walking for checkouts again")?;
            *self.on_disk.lock().await = Some(Arc::new(all));
            Ok(())
        }
        .await;
        self.rescanning.store(false, Ordering::Relaxed);
        walked
    }

    async fn read_logs_ahead(&self) {
        if let Err(e) = self.sighted_folders().await {
            log::debug!("the agents' logs were not read ahead: {e:#}");
        }
    }

    /// The checkouts under the search folder `warm` read. An error until it
    /// read some, which the first walk can make minutes: requests do not wait,
    /// and an answer without them would say a checkout is not there.
    fn disk_checkouts(&self) -> Result<Arc<Vec<found::Checkout>>> {
        self.on_disk
            .try_lock()
            .ok()
            .and_then(|on_disk| on_disk.clone())
            .context("still looking for checkouts on this machine")
    }

    /// The folders sessions started outside Beep worked in, read from the
    /// agents' logs at most `SIGHTED_FOR` ago. Requests that come during a
    /// read wait for it rather than read again.
    pub(super) async fn sighted_folders(&self) -> Result<Arc<Vec<PathBuf>>> {
        let mut sighted = self.sighted.lock().await;

        if let Some(fresh) = sighted.as_ref().filter(|s| s.at.elapsed() < SIGHTED_FOR) {
            return Ok(fresh.folders.clone());
        }

        let mut folders: Vec<PathBuf> = self
            .sightings()
            .await?
            .into_iter()
            .map(|seen| seen.cwd)
            .collect();
        folders.sort();
        folders.dedup();
        let folders = Arc::new(folders);
        *sighted = Some(Sighted {
            at: Instant::now(),
            folders: folders.clone(),
        });
        Ok(folders)
    }

    /// The Claude Code sessions started outside Beep.
    pub fn external(&self) -> &External {
        &self.external
    }

    /// The Claude Code sessions started outside Beep, newest first.
    pub async fn external_sessions(&self, limit: u32) -> Result<Vec<ExternalSession>> {
        let sessions = self.external.list(limit, None, self.owned()).await?;
        self.observe_external(&sessions);
        Ok(sessions)
    }

    /// Every session the agents' stores hold, with the folder it ran in.
    pub(super) async fn sightings(&self) -> Result<Vec<found::Seen>> {
        let stores = self.stores.clone();
        let others = spawn_blocking(move || stores.sessions(None));
        let (claude, others) = tokio::join!(self.external.sightings(), others);
        let mut seen = claude.unwrap_or_else(|e| {
            log::warn!("Claude Code sessions not read: {e:#}");
            Vec::new()
        });
        seen.extend(others.context("reading the agents' sessions")?);

        for native in self.presence.snapshot() {
            if seen
                .iter()
                .any(|seen| seen.agent.as_ref() == native.agent && seen.id == native.id)
            {
                continue;
            }

            seen.push(found::Seen {
                agent: Agent::Flsh,
                id: native.id,
                cwd: PathBuf::from(native.cwd),
                started_at: native.started_at,
                last_activity: native.last_activity,
                title: native.title,
                file: None,
            });
        }

        Ok(seen)
    }

    /// The checkouts coding agents worked in, then the other checkouts
    /// found on disk, newest session or commit first. One listing runs at a
    /// time: an ask that came while one ran takes what it found.
    pub async fn found_folders(&self, limit: u32) -> Result<Vec<SessionFolder>> {
        let limit = match usize::try_from(limit).unwrap_or(MAX_FOLDERS) {
            0 => DEFAULT_FOLDERS,
            limit => limit.min(MAX_FOLDERS),
        };
        let asked = Instant::now();
        let mut listed = self.listed.lock().await;

        if let Some(done) = listed
            .as_ref()
            .filter(|done| done.at >= asked && done.limit == limit)
        {
            return Ok(done.folders.as_ref().clone());
        }

        let on_disk = self.disk_checkouts()?;
        let seen = self.sightings().await?;
        let home = dirs::home_dir();
        let folders = spawn_blocking(move || {
            let mut out = found::folders(&seen, home.as_deref(), limit, found::checkout);
            found::add_checkouts(&mut out, &on_disk, home.as_deref(), limit);
            found::with_commits(&mut out, found::last_commit);
            out
        })
        .await
        .context("looking up the folders' checkouts")?;
        *listed = Some(Listed {
            at: Instant::now(),
            limit,
            folders: Arc::new(folders.clone()),
        });
        Ok(folders)
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
        self.presence.merge(&mut sessions);
        sessions.retain(|session| {
            folders
                .iter()
                .any(|folder| found::within(Path::new(&session.cwd), folder))
        });

        for session in &mut sessions {
            if session.taken_over.is_none() {
                session.taken_over = owned.get(&session.id).cloned();
            }
        }

        sessions.sort_by_key(|session| Reverse(session.last_activity));
        sessions.truncate(limit);
        Ok(sessions)
    }

    /// Sessions of `agent` started outside Beep, newest first: at most
    /// `limit`, or only `session` when given.
    pub(super) async fn external_of(
        &self,
        agent: &str,
        limit: u32,
        session: Option<String>,
    ) -> Result<Vec<ExternalSession>> {
        if agent == Agent::Claude.as_ref() {
            self.external.list(limit, session, self.owned()).await
        } else {
            self.stores_external(agent, limit, session).await
        }
    }

    /// The session `id` of `agent` started outside Beep, when this node
    /// finds it; `owned` names the sessions this node runs.
    pub(super) async fn find_external(
        &self,
        agent: Agent,
        id: &str,
        owned: HashMap<String, String>,
    ) -> Result<Option<ExternalSession>> {
        Ok(match agent {
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
        })
    }

    /// Sessions of `agent`, one other than Claude Code, started outside
    /// Beep, newest first: at most `limit`, or only `session` when given.
    pub(super) async fn stores_external(
        &self,
        agent: &str,
        limit: u32,
        session: Option<String>,
    ) -> Result<Vec<ExternalSession>> {
        let agent = agent
            .parse::<Agent>()
            .ok()
            .filter(|agent| *agent != Agent::Claude)
            .with_context(|| format!("this node lists no {agent} sessions started outside Beep"))?;
        let owned = self.owned();
        let stores = self.stores.clone();
        let only = session.clone();
        let mut sessions = spawn_blocking(move || {
            let now = SystemTime::now();
            let mut seen = match session.as_deref() {
                Some(id) => stores.seen(Some(agent), id).into_iter().collect(),
                None => stores.sessions(Some(agent)),
            };
            seen.sort_by_key(|seen| Reverse(seen.last_activity));

            match session {
                Some(id) => seen.retain(|seen| seen.id == id),
                None => seen.truncate(store_limit(limit)),
            }

            seen.into_iter()
                .map(|seen| stores.describe(seen, now))
                .collect::<Vec<_>>()
        })
        .await?;
        let held = self.held(agent, &sessions).await;

        for found in &mut sessions {
            // A native thread shares its parent's process. Its own turn events,
            // rather than the process holding its file, say whether it works.
            if found.id.starts_with("subagent:") {
                continue;
            }

            let held = held.get(&found.id).copied().unwrap_or(Holding::Unknown);
            found.presence = holders::settle(found.presence, found.pid, held);

            if found.pid.is_none() {
                found.pid = held.pid();
            }
        }

        self.presence.merge(&mut sessions);
        sessions.retain(|found| {
            found.agent == agent.as_ref() && only.as_ref().is_none_or(|id| found.id == *id)
        });
        sessions.sort_by_key(|session| Reverse(session.last_activity));

        if only.is_none() {
            sessions.truncate(store_limit(limit));
        }

        for found in &mut sessions {
            found.taken_over = owned.get(&found.id).cloned();
        }

        Ok(sessions)
    }

    /// The sessions `asks` started outside Beep, each looked up by its file
    /// or record rather than by listing every session; one that is not found
    /// or cannot be read is left out.
    pub(super) async fn external_named(&self, asks: Vec<ExternalAsk>) -> Vec<ExternalSession> {
        let mut found = Vec::new();

        for ask in asks.into_iter().take(MAX_EXTERNAL_NAMED) {
            let one = self
                .external_of(&ask.agent, 1, Some(ask.session.clone()))
                .await;

            match one {
                Ok(sessions) => found.extend(sessions),
                Err(e) => log::debug!(
                    "outside session not read; agent={} session={}: {e:#}",
                    ask.agent,
                    ask.session
                ),
            }
        }

        found
    }

    /// The sessions this node runs, by the agent's own id, to the node's id.
    pub(super) fn owned(&self) -> HashMap<String, String> {
        self.lock()
            .values()
            .filter_map(|session| {
                watched::read(&session.live, |live| {
                    Some((live.agent_session.clone()?, live.info.id.clone()))
                })
            })
            .collect()
    }

    /// Ends the process running `agent`'s session `id`, started outside Beep,
    /// so it can go on here. The process is the one this node finds running
    /// that session now, never one a caller names.
    pub(super) async fn close_external(&self, agent: &str, id: &str) -> Result<()> {
        anyhow::ensure!(self.get(id).is_none(), "the session already runs in Beep");
        let found = self.external_of(agent, 1, Some(id.to_owned())).await?;
        let session = found
            .into_iter()
            .find(|found| found.agent == agent && found.id == id)
            .with_context(|| format!("there is no {agent} session {id} on this machine"))?;

        anyhow::ensure!(
            session.taken_over.is_none(),
            "the session already runs in Beep"
        );
        anyhow::ensure!(
            session.presence != Presence::Working,
            "the session is running a turn"
        );

        // Noted first: a follow reads the session again as soon as its
        // process exits, and must find it closed then.
        {
            let mut closed = crate::lock(&self.closed);
            closed.retain(|_, at| at.elapsed() < CLOSED_FOR);
            closed.insert(id.to_owned(), Instant::now());
        }

        if let Some(pid) = session.pid {
            super::processes::close_outside(pid, CLOSE_WAIT)
                .await
                .context("the session's process did not end")?;
        }

        self.events.changed(Topic::keyed("agent", id));
        self.events.changed(Topic::new("agents"));
        Ok(())
    }

    /// A session `close_external` ended lately reads as stopped, not as
    /// active a moment ago from its last write.
    pub(super) fn settle_closed(&self, sessions: &mut [ExternalSession]) {
        let mut closed = crate::lock(&self.closed);

        if closed.is_empty() {
            return;
        }

        closed.retain(|_, at| at.elapsed() < CLOSED_FOR);

        for found in sessions {
            if found.pid.is_none()
                && found.presence == Presence::Recent
                && closed.contains_key(&found.id)
            {
                found.presence = Presence::Stopped;
            }
        }
    }

    /// Takes over the Claude Code session `id` started outside Beep, once
    /// nothing runs it; one this node already runs is returned as it is.
    /// `id` may also be one of this node's sessions that ended: it goes on
    /// from the agent's own session.
    pub(super) async fn resume(
        &self,
        kind: &AgentKind,
        id: &str,
        prompt: Option<String>,
    ) -> Result<Arc<Session>> {
        let _store = self.session_store.lock().await;
        anyhow::ensure!(
            !id.starts_with("subagent:"),
            "This subagent is managed by its parent. Open the parent to continue it."
        );
        let ours = self.get(id);

        if let Some(session) = &ours
            && session.agent != kind.name
        {
            anyhow::bail!("session {id} runs {}", session.agent);
        }
        // It goes on as it was: under its parent, and in the mode it was held
        // to, so a resume is no way round the limits or the mode.
        let before = ours.as_ref().map(|session| {
            let info = session.info();
            Held {
                parent: info.parent,
                mode: info.mode,
                model: info.model,
                worktrees: info.worktrees,
                assistant: session.assistant,
            }
        });
        let ended = ours
            .as_ref()
            .map(|session| watched::read(&session.live, |live| live.agent_session.clone()));
        let id = match ended {
            Some(Some(agent_session)) => agent_session,
            Some(None) => anyhow::bail!("session {id} ended before the agent gave it an id"),
            None => id.to_owned(),
        };
        let id = id.as_str();
        let owned = self.owned();

        if let Some(session) = owned.get(id).and_then(|node_id| self.get(node_id))
            && session.agent == kind.name
            && watched::read(&session.live, |live| live.status() != Status::Ended)
        {
            if let Some(text) = prompt {
                session
                    .order(Command::Prompt(text.into()))
                    .await
                    .map_err(anyhow::Error::msg)?;
            }

            return Ok(session);
        }
        // ACP can reload a session this node ran without a reader for the
        // harness's private store. Outside sessions still need that reader.
        let cwd = if let Ok(agent) = kind.name.parse::<Agent>() {
            let mut found = self
                .find_external(agent, id, owned)
                .await?
                .with_context(|| format!("there is no {} session {id} on this machine", kind.name))?;
            found.presence = self.presence(agent, &found).await;
            self.settle_closed(std::slice::from_mut(&mut found));

            if let Some(reason) = claude::refuse_takeover(&found) {
                anyhow::bail!("{reason}");
            }

            found.cwd
        } else {
            let previous = ours.with_context(|| {
                format!(
                    "{} sessions started outside Beep cannot be taken over",
                    kind.name
                )
            })?;

            previous.info().cwd
        };

        self.prepare(kind).await?;
        super::restart::to_project(kind, id, Path::new(&cwd)).await;
        // It goes on with the model it last ran on here, also after a restart.
        // One found outside Beep in the home folder is the assistant.
        let mut before = before.unwrap_or_else(|| Held {
            assistant: super::running::home_assistant(Path::new(&cwd), false),
            ..Held::default()
        });
        before.model = before.model.or_else(|| self.last_model(id));
        if before.worktrees.is_empty() {
            before.worktrees = self.created_worktrees(id);
        }
        let begin = self.resumed_begin(prompt, id, before);
        let session = self.start(kind, Some(&cwd), begin)?;
        self.watch_parent(&session);
        Ok(session)
    }

    /// How a session of this node that ended begins again: with the link to
    /// its parent and the mode it was in, and following the parent when that
    /// runs here.
    pub(super) fn resumed_begin(&self, prompt: Option<String>, id: &str, before: Held) -> Begin {
        let Held {
            parent,
            mode,
            model,
            worktrees,
            assistant,
        } = before;
        let follows = parent.as_ref().and_then(|link| self.parent_live_of(link));
        Begin {
            prompt,
            resume: Some(id.to_owned()),
            mode,
            model,
            parent,
            follows,
            worktrees,
            assistant,
            ..Begin::default()
        }
    }

    /// What `found`, a session in `agent`'s logs, is doing, once a
    /// recent write alone is checked: Codex keeps a session's rollout open
    /// while it runs, and a session Beep ran here ended with Beep.
    pub(super) async fn presence(&self, agent: Agent, found: &ExternalSession) -> Presence {
        if let Some(registered) = self
            .presence
            .snapshot()
            .into_iter()
            .find(|native| native.agent == agent.as_ref() && native.id == found.id)
        {
            return registered.presence;
        }

        let held = self.held(agent, std::slice::from_ref(found)).await;
        holders::settle(
            found.presence,
            found.pid,
            held.get(&found.id).copied().unwrap_or(Holding::Unknown),
        )
    }

    /// What holds the file of each of `found` that no process was seen
    /// running, by session id, all looked up at once.
    async fn held(&self, agent: Agent, found: &[ExternalSession]) -> HashMap<String, Holding> {
        let ids: Vec<String> = found
            .iter()
            .filter(|found| found.pid.is_none())
            .map(|found| found.id.clone())
            .collect();
        // Claude Code does not keep its transcript open, but its processes
        // were looked at already and none runs these sessions.
        if agent == Agent::Claude {
            return ids.into_iter().map(|id| (id, Holding::Closed)).collect();
        }

        if ids.is_empty() {
            return HashMap::new();
        }

        let stores = self.stores.clone();
        let files = spawn_blocking(move || {
            ids.into_iter()
                .filter_map(|id| Some((stores.file(Some(agent), &id)?, id)))
                .collect::<HashMap<PathBuf, String>>()
        })
        .await
        .unwrap_or_else(|e| {
            log::warn!("could not find the sessions' files: {e}");
            HashMap::new()
        });
        let open = self
            .holders
            .held_open(files.keys().cloned().collect())
            .await;
        files
            .into_iter()
            .map(|(file, id)| {
                let holding = open.get(&file).copied().unwrap_or(Holding::Unknown);
                (id, holding)
            })
            .collect()
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
        let (images, wanted) = (self.images.clone(), key.to_owned());
        let found = spawn_blocking(move || images.find_in_file(&file, &wanted))
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
    pub(super) async fn session_file(&self, session: &str) -> Result<Option<PathBuf>> {
        let (agent, id) = match self.get(session) {
            Some(running) => {
                let Some(id) = watched::read(&running.live, |live| live.agent_session.clone())
                else {
                    return Ok(None);
                };
                (running.agent.parse::<Agent>().ok(), id)
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
}
