//! Sessions and folders found in the agents' own logs and on disk.

use super::{AgentKind, Agents, Begin, SavedSession, Session, claude, limits::{DEFAULT_FOLDERS, DEFAULT_IN_FOLDERS, MAX_ASKED_FOLDERS, MAX_FOLDERS, MAX_IN_FOLDERS}, session::Command};
use crate::logs::{claude::Store, found::Stores};
use crate::occupancy::Holding;
use crate::logs::picture::Picture;
use crate::logs::window::clamp_limit;
use crate::{
    logs::found::{self, Agent},
};
use anyhow::{Context, Result};
use beep_proto::node::{
    agent::{
        Entry, ExternalAsk, ExternalSession, HistoryCursor, MAX_EXTERNAL_NAMED, Presence,
        SessionFolder, SessionPage, Status,
    },
    events::Topic,
};
use beep_proto::watched;
use beep_proto::{ModelId, SessionId};
use std::{
    cmp::Reverse,
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, SystemTime},
};
use tokio::{task::spawn_blocking, time::Instant};

/// How long `close_external` waits for the process to end.
const CLOSE_WAIT: Duration = Duration::from_secs(8);

/// The last folder listing, for the asks that waited while it ran.
pub(super) struct Listed {
    /// When it ended.
    at: Instant,
    folders: Arc<Vec<SessionFolder>>,
}

/// The absolute folders asked, each also by its real path.
async fn asked_folders(folders: Vec<String>) -> Result<Vec<PathBuf>> {
    let folders: Vec<PathBuf> = folders
        .into_iter()
        .take(MAX_ASKED_FOLDERS)
        .map(PathBuf::from)
        .filter(|folder| folder.is_absolute())
        .collect();

    Ok(spawn_blocking(move || found::with_real(folders)).await?)
}

impl Agents {
    /// Reads the checkouts saved in the node's folder `dir` and the agents'
    /// logs ahead of the first requests, then walks `root` so checkouts made
    /// or removed since count. The saved list answers during the walk; with
    /// none saved, `disk_checkouts` refuses until it ends. No rescan runs
    /// meanwhile, so the walk is the only writer of the list.
    pub async fn warm(&self, root: Option<PathBuf>, dir: PathBuf) {
        let _walking = self.rescanning.lock().await;
        let Some(root) = root else {
            self.publish_checkouts(Some(Arc::default()));
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
        let first_walk = saved.is_none();
        self.publish_checkouts(saved.map(Arc::new));
        self.read_logs_ahead().await;
        let walk = match spawn_blocking(move || found::walk(&root, &dir)).await {
            Ok(walk) => walk,
            Err(e) => {
                log::warn!("the checkouts on disk were not walked: {e}");
                return;
            }
        };

        if first_walk || walk.whole {
            self.publish_checkouts(Some(Arc::new(walk.found)));
        } else {
            log::info!("the walk for checkouts was cut short; kept the saved list");
        }
    }

    /// Walks `root` again, quickly and not deep, so a checkout made since the last walk
    /// counts, and saves the list in `dir`. Nothing while a walk runs or another
    /// rescan does: what that one finds serves this one too.
    pub async fn rescan(&self, root: PathBuf, dir: PathBuf) -> Result<()> {
        let Ok(_walking) = self.rescanning.try_lock() else {
            return Ok(());
        };

        let known = self.disk_checkouts()?;
        let all = spawn_blocking(move || found::rescan(&root, &dir, &known))
            .await
            .context("walking for checkouts again")?;
        self.publish_checkouts(Some(Arc::new(all)));

        Ok(())
    }

    fn publish_checkouts(&self, checkouts: Option<Arc<Vec<found::Checkout>>>) {
        *crate::lock(&self.on_disk) = checkouts;
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
        crate::lock(&self.on_disk)
            .clone()
            .context("still looking for checkouts on this machine")
    }

    /// The folders sessions started outside Beep worked in, with those of
    /// sessions known only from a live connection.
    pub(super) async fn sighted_folders(&self) -> Result<Vec<PathBuf>> {
        let folders = self.occupancy.sighted().await?.folders;
        let mut folders = folders.as_ref().clone();
        let before = folders.len();
        folders.extend(
            self.occupancy
                .presence()
                .snapshot()
                .into_iter()
                .map(|native| PathBuf::from(native.cwd)),
        );

        if folders.len() > before {
            folders.sort();
            folders.dedup();
        }

        Ok(folders)
    }

    /// Where Claude Code keeps the sessions started outside Beep.
    pub(crate) fn external(&self) -> &Store {
        self.occupancy.claude()
    }

    /// Where Codex, pi, flsh and OpenCode keep theirs.
    pub(crate) fn stores(&self) -> &Stores {
        self.occupancy.stores()
    }

    /// The Claude Code sessions started outside Beep, newest first.
    pub async fn external_sessions(&self, limit: u32) -> Result<Vec<ExternalSession>> {
        let sessions = self
            .external_of(Agent::Claude.as_ref(), limit, None)
            .await?;
        self.observe_external(&sessions);
        Ok(sessions)
    }

    /// Every session the agents' stores hold, with the folder it ran in:
    /// what the watcher last read while they are watched, else a read of them.
    pub(super) async fn sightings(&self) -> Result<Vec<found::Seen>> {
        let sighted = self.occupancy.sighted().await?;

        Ok(self.occupancy.with_present(sighted.seen.as_ref().clone()))
    }

    /// The checkouts coding agents worked in, then the other checkouts
    /// found on disk, newest session or commit first. One listing runs at a
    /// time for a `limit`: an ask for it that came while one ran takes what
    /// it found. An ask for another limit does not wait.
    pub async fn found_folders(&self, limit: u32) -> Result<Vec<SessionFolder>> {
        let limit = clamp_limit(limit, DEFAULT_FOLDERS, MAX_FOLDERS);
        let asked = Instant::now();
        let slot = crate::lock(&self.listed).entry(limit).or_default().clone();
        let mut listed = slot.lock().await;

        if let Some(done) = listed.as_ref()
            && done.at >= asked
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
        let limit = clamp_limit(limit, DEFAULT_IN_FOLDERS, MAX_IN_FOLDERS);
        let folders = asked_folders(folders).await?;
        let seen = self.sightings().await?;
        let picked = {
            let folders = folders.clone();
            spawn_blocking(move || found::in_folders(seen, &folders, limit)).await?
        };
        let mut sessions = self.describe_history(picked, folders).await?;
        sessions.truncate(limit);

        Ok(sessions)
    }

    /// Pages metadata without a total history cap. The cursor follows the
    /// discovery order, even when a file disappears while describing it.
    pub async fn sessions_page(
        &self,
        folders: Vec<String>,
        limit: u32,
        before: Option<HistoryCursor>,
    ) -> Result<SessionPage> {
        let limit = clamp_limit(limit, DEFAULT_IN_FOLDERS, MAX_IN_FOLDERS);
        let folders = asked_folders(folders).await?;
        let seen = self.sightings().await?;
        let (picked, next) = {
            let folders = folders.clone();
            spawn_blocking(move || found::history_page(seen, &folders, limit, before.as_ref()))
                .await?
        };
        let selected: HashSet<_> = picked
            .iter()
            .map(|seen| (seen.agent.to_string(), seen.id.clone()))
            .collect();
        let mut sessions = self.describe_history(picked, folders).await?;
        sessions.retain(|session| selected.contains(&(session.agent.clone(), session.id.clone())));
        for session in &mut sessions {
            session.last_prompt.clear();
            session.last_message.clear();
        }
        Ok(SessionPage { sessions, next })
    }

    async fn describe_history(
        &self,
        picked: Vec<found::Seen>,
        folders: Vec<PathBuf>,
    ) -> Result<Vec<ExternalSession>> {
        let (claude, others): (Vec<found::Seen>, Vec<found::Seen>) = picked
            .into_iter()
            .partition(|seen| seen.agent == Agent::Claude);
        let ids: HashSet<SessionId> = claude.into_iter().map(|seen| seen.id).collect();
        let owned = self.sessions.owned();
        let starting = self.sessions.starting_pids();
        let stores = self.stores().clone();
        let described = spawn_blocking(move || {
            let now = SystemTime::now();
            others
                .into_iter()
                .map(|seen| stores.describe(seen, now))
                .collect::<Vec<_>>()
        });
        let (claude, others) = tokio::join!(self.external().named(ids, owned.clone()), described);
        let mut sessions = claude?;
        sessions.extend(others.context("reading the sessions' titles")?);
        self.occupancy.presence().merge(&mut sessions);
        let mut sessions = spawn_blocking(move || {
            let mut under = found::Under::new(&folders);
            sessions.retain(|session| under.has(Path::new(&session.cwd)));
            sessions
        })
        .await?;

        for session in &mut sessions {
            if session.taken_over.is_none() {
                session.taken_over = owned.get(&session.id).cloned();
            }
        }

        // A running reviewer is never listed (`Session::reviewer`), nor is a
        // session whose process this node is still starting before ACP names it.
        sessions.retain(|session| {
            !session
                .taken_over
                .as_ref()
                .is_some_and(|id| self.is_reviewer(id))
                && !(session.taken_over.is_none()
                    && session.pid.is_some_and(|pid| starting.contains(&pid)))
        });

        self.mark_kept(&mut sessions);
        sessions.sort_by_key(|session| Reverse(session.last_activity));
        Ok(sessions)
    }

    /// Sessions of `agent` started outside Beep, newest first: at most
    /// `limit`, or only `session` when given.
    pub(super) async fn external_of(
        &self,
        agent: &str,
        limit: u32,
        session: Option<SessionId>,
    ) -> Result<Vec<ExternalSession>> {
        let mut sessions = self.raw_external_of(agent, limit, session).await?;
        self.mark_kept(&mut sessions);
        Ok(sessions)
    }

    pub(super) async fn raw_external_of(
        &self,
        agent: &str,
        limit: u32,
        session: Option<SessionId>,
    ) -> Result<Vec<ExternalSession>> {
        let starting = self.sessions.starting_pids();
        let mut sessions = if agent == Agent::Claude.as_ref() {
            self.external().list(limit, session, self.sessions.owned()).await?
        } else {
            self.stores_external(agent, limit, session).await?
        };

        for found in &mut sessions {
            if found.taken_over.is_none() && found.pid.is_some_and(|pid| starting.contains(&pid)) {
                found.taken_over = Some(found.id.clone());
            }
        }

        Ok(sessions)
    }

    pub(super) fn follow_roots(&self, agent: Agent) -> Vec<PathBuf> {
        match agent {
            Agent::Claude => self.external().follow_paths(),
            other => self.stores().follow_paths(Some(other)),
        }
    }

    pub(super) async fn transcript_window(
        &self,
        session: &SessionId,
        last: usize,
        before: Option<u64>,
        followed: Option<(&crate::logs::claude::Followed, Option<u64>)>,
        keep_pictures: bool,
    ) -> Result<Option<(Vec<Entry>, u64)>> {
        if let Some(found) = self.get(session) {
            return Ok(Some(self.live_transcript(&found, last, before).await));
        }

        let read = match followed {
            Some((f, from)) => Some(self.external().followed_transcript(f, last, from).await?),
            None => match self.external().transcript(session, last, before).await? {
                Some(read) => Some(read),
                None => {
                    let (stores, id) = (self.stores().clone(), session.to_owned());
                    spawn_blocking(move || stores.transcript(&id, last, before)).await??
                }
            },
        };
        Ok(read.map(|read| {
            if keep_pictures {
                for picture in read.pictures {
                    self.images.keep(session, picture);
                }
            }
            (read.entries, read.earlier)
        }))
    }

    /// Gives those of `sessions` that ran here what this node kept of them:
    /// that one was the home assistant, so it stays with its chats once it
    /// ended, and the name its parent gave it and the custom agent it ran
    /// as, which its agent's files lack.
    pub(super) fn mark_kept(&self, sessions: &mut [ExternalSession]) {
        let kept: HashMap<SessionId, SavedSession> = self
            .saved_sessions()
            .into_iter()
            .map(|saved| (saved.info.id.clone(), saved))
            .collect();

        for session in sessions {
            let kept = kept.get(&session.id);
            session.assistant = kept.is_some_and(|kept| kept.info.assistant && !kept.info.reviewer);
            session.persona = kept.and_then(|kept| kept.info.persona.clone());
            session.continues = kept.and_then(|kept| kept.info.continues.clone());

            if let Some(kept) = kept
                && kept.named
            {
                session.title.clone_from(&kept.info.title);
            }
        }
    }

    /// The session `id` of `agent` started outside Beep, when this node
    /// finds it; `owned` names the sessions this node runs.
    pub(super) async fn find_external(
        &self,
        agent: Agent,
        id: &SessionId,
        owned: HashMap<SessionId, SessionId>,
    ) -> Result<Option<ExternalSession>> {
        Ok(match agent {
            Agent::Claude => self
                .external()
                .list(1, Some(id.clone()), owned)
                .await?
                .into_iter()
                .next(),
            other => {
                let stores = self.stores().clone();
                let id = id.clone();
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
        session: Option<SessionId>,
    ) -> Result<Vec<ExternalSession>> {
        let agent = agent
            .parse::<Agent>()
            .ok()
            .filter(|agent| *agent != Agent::Claude)
            .with_context(|| format!("this node lists no {agent} sessions started outside Beep"))?;
        let limit = clamp_limit(limit, 1, MAX_IN_FOLDERS);
        let owned = self.sessions.owned();
        let starting = self.sessions.starting_pids();
        let stores = self.stores().clone();
        let only = session.clone();
        let mut sessions = spawn_blocking(move || {
            let now = SystemTime::now();
            let seen: Vec<_> = match session.as_ref() {
                Some(id) => stores.seen(Some(agent), id).into_iter().collect(),
                None => {
                    let mut seen = stores.sessions(Some(agent));
                    seen.sort_by_key(|seen| Reverse(seen.last_activity));
                    seen.truncate(limit);
                    seen
                }
            };
            seen.into_iter()
                .map(|seen| stores.describe(seen, now))
                .collect::<Vec<_>>()
        })
        .await?;
        let held = self.occupancy.held(agent, &sessions).await;

        for found in &mut sessions {
            // A native thread shares its parent's process. Its own turn events,
            // rather than the process holding its file, say whether it works.
            if found.id.as_ref().starts_with("subagent:") {
                continue;
            }

            let held = held.get(&found.id).copied().unwrap_or(Holding::Unknown);
            found.presence = crate::occupancy::settle(found.presence, found.pid, held);

            if found.pid.is_none() {
                found.pid = held.pid();
            }
        }

        self.occupancy.presence().merge(&mut sessions);
        sessions.retain(|found| {
            found.agent == agent.as_ref() && only.as_ref().is_none_or(|id| found.id == *id)
        });
        sessions.sort_by_key(|session| Reverse(session.last_activity));

        if only.is_none() {
            sessions.truncate(limit);
        }

        for found in &mut sessions {
            found.taken_over = owned.get(&found.id).cloned().or_else(|| {
                found
                    .pid
                    .filter(|pid| starting.contains(pid))
                    .map(|_| found.id.clone())
            });
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

    /// Ends the process running `agent`'s session `id`, started outside Beep,
    /// so it can go on here. The process is the one this node finds running
    /// that session now, never one a caller names.
    pub(super) async fn close_external(&self, agent: &str, id: &SessionId) -> Result<()> {
        anyhow::ensure!(self.get(id).is_none(), "the session already runs in Beep");
        let found = self.external_of(agent, 1, Some(id.clone())).await?;
        let session = found
            .into_iter()
            .find(|found| found.agent == agent && found.id == *id)
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
        self.occupancy.note_closed(id);

        if let Some(pid) = session.pid {
            super::processes::close_outside(pid, CLOSE_WAIT)
                .await
                .context("the session's process did not end")?;
        }

        self.events.changed(Topic::keyed("agent", id));
        self.events.changed(Topic::new("agents"));
        Ok(())
    }

    /// Takes over the Claude Code session `id` started outside Beep, once
    /// nothing runs it; one this node already runs is returned as it is.
    /// `id` may also be one of this node's sessions that ended: it goes on
    /// from the agent's own session.
    pub(super) async fn resume(
        &self,
        kind: &AgentKind,
        id: &SessionId,
        prompt: Option<String>,
        model: Option<ModelId>,
    ) -> Result<Arc<Session>> {
        let store = self.sessions.store_of(id);
        let _store = store.lock().await;
        anyhow::ensure!(
            !id.as_ref().starts_with("subagent:"),
            "This subagent is managed by its parent. Open the parent to continue it."
        );
        let ours = self.get(id);

        if let Some(session) = &ours
            && session.agent != kind.name
        {
            anyhow::bail!("session {id} runs {}", session.agent);
        }
        // It goes on as it was: under its parent, and in the mode it was held
        // to, so a resume is no way round the limits or the mode. One that
        // ended before the node restarted goes on as saved.
        let before = self.saved(id);
        let (ended, held) = match &ours {
            Some(session) => watched::read(&session.live, |live| {
                (Some(live.agent_session.clone()), live.held.clone())
            }),
            None => (None, Vec::new()),
        };
        let id = match ended {
            Some(Some(agent_session)) => agent_session,
            Some(None) => anyhow::bail!("session {id} ended before the agent gave it an id"),
            None => id.clone(),
        };
        let id = &id;
        let owned = self.sessions.owned();

        if let Some(session) = owned.get(id).and_then(|node_id| self.get(node_id))
            && session.agent == kind.name
            && watched::read(&session.live, |live| live.status() != Status::Ended)
        {
            return self.said_on_resume(session, prompt).await;
        }
        // ACP can reload a session this node ran without a reader for the
        // harness's private store. Outside sessions still need that reader.
        let cwd = if let Ok(agent) = kind.name.parse::<Agent>() {
            let mut found = self
                .find_external(agent, id, owned)
                .await?
                .with_context(|| {
                    format!("there is no {} session {id} on this machine", kind.name)
                })?;
            found.presence = self.occupancy.presence_of(agent, &found).await;
            self.occupancy.settle_closed(std::slice::from_mut(&mut found));

            if let Some(reason) = claude::refuse_takeover(&found) {
                anyhow::bail!("{reason}");
            }

            found.cwd
        } else {
            // A record from before saved sessions may know no folder.
            let previous = before
                .as_ref()
                .filter(|saved| !saved.info.cwd.is_empty())
                .with_context(|| {
                    format!(
                        "{} sessions started outside Beep cannot be taken over",
                        kind.name
                    )
                })?;

            previous.info.cwd.clone()
        };

        self.catalog.prepare(kind).await?;
        let cwd = super::restart::ready_to_resume(kind, id, Path::new(&cwd)).await;
        let cwd = cwd.to_string_lossy();
        // Never seen here, as one found outside Beep: never the assistant.
        let mut begin = self.resumed_begin(id, before.unwrap_or_default());
        begin.model = model.or(begin.model);
        let session = self.start(kind, Some(&cwd), begin).await?;
        self.watch_parent(&session);

        let bare_resume = prompt.as_deref().is_none_or(|text| text.trim().is_empty());
        if bare_resume && !held.is_empty() {
            session.set_user_turn(true).await;
            for waiting in held {
                self.order_by_user(&session, Command::Prompt(waiting))
                    .await
                    .map_err(anyhow::Error::msg)?;
            }
            return Ok(session);
        }

        // Its first turn here is the user's, whose words begin it.
        if prompt.is_some() {
            session.set_user_turn(true).await;
        }

        self.said_on_resume(session, prompt).await
    }

    /// Gives `session`, resumed, the words the user resumed it with: they
    /// are the user's like any other message, so a thread hears what its
    /// relatives came to and its turn wakes no idle parent.
    async fn said_on_resume(
        &self,
        session: Arc<Session>,
        prompt: Option<String>,
    ) -> Result<Arc<Session>> {
        if let Some(text) = prompt {
            self.order_by_user(&session, Command::Prompt(text.into()))
                .await
                .map_err(anyhow::Error::msg)?;
        }

        Ok(session)
    }

    /// How a session of this node that ended begins again, as `before` it
    /// was: with the link to its parent and the mode it was in, and
    /// following the parent when that runs here.
    pub(super) fn resumed_begin(&self, id: &SessionId, before: SavedSession) -> Begin {
        let SavedSession {
            info,
            effort,
            fast,
            account,
            named,
        } = before;
        let follows = info
            .parent
            .as_ref()
            .and_then(|link| self.parent_live_of(link));
        Begin {
            resume: Some(id.clone()),
            title: info.title,
            named,
            mode: info.mode,
            model: info.model,
            effort,
            fast,
            parent: info.parent,
            follows,
            worktrees: info.worktrees,
            assistant: info.assistant && !info.reviewer,
            account,
            persona: info.persona,
            continues: info.continues,
            issue: info.issue,
            ..Begin::default()
        }
    }

    /// The picture `key` of `session`'s transcript: kept, or else found
    /// again in the agent's own file of the session and kept. `session` is
    /// one of this node's sessions or one started outside Beep.
    pub(crate) async fn image(&self, session: &SessionId, key: &str) -> Result<Option<Arc<Picture>>> {
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
    pub(super) async fn session_file(&self, session: &SessionId) -> Result<Option<PathBuf>> {
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
        self.store_file(agent, &id).await
    }

    pub(in crate::agents) async fn store_file(
        &self,
        agent: Option<Agent>,
        id: &SessionId,
    ) -> Result<Option<PathBuf>> {
        if agent.is_none_or(|agent| agent == Agent::Claude)
            && let Some(file) = self.external().file(id).await?
        {
            return Ok(Some(file));
        }

        if agent == Some(Agent::Claude) {
            return Ok(None);
        }

        let (stores, id) = (self.stores().clone(), id.to_owned());
        Ok(spawn_blocking(move || stores.file(agent, &id)).await?)
    }
}

#[cfg(test)]
#[path = "../tests/agents_found_sessions.rs"]
mod tests;
