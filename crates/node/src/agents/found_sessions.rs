//! Sessions and folders found in the agents' own logs and on disk.

use super::{
    AgentKind, Agents, Begin, Command, DEFAULT_FOLDERS, DEFAULT_IN_FOLDERS, MAX_ASKED_FOLDERS,
    MAX_FOLDERS, MAX_IN_FOLDERS, Picture, SIGHTED_FOR, Session, Sighted, holders,
};
use crate::{
    agents::claude::{self, External},
    agents::found::{self, Agent},
};
use anyhow::{Context, Result};
use beep_proto::node::agent::{ExternalSession, ParentLink, Presence, SessionFolder, Status};
use std::{
    cmp::Reverse,
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::Arc,
    time::SystemTime,
};
use tokio::{task::spawn_blocking, time::Instant};

/// What a session of this node that ended was held to.
#[derive(Default)]
pub(super) struct Held {
    pub(super) parent: Option<ParentLink>,
    pub(super) mode: Option<String>,
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
        self.external.list(limit, None, self.owned()).await
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
        Ok(seen)
    }

    /// The checkouts coding agents worked in, then the other checkouts
    /// found on disk, newest session or commit first.
    pub async fn found_folders(&self, limit: u32) -> Result<Vec<SessionFolder>> {
        let limit = match usize::try_from(limit).unwrap_or(MAX_FOLDERS) {
            0 => DEFAULT_FOLDERS,
            limit => limit.min(MAX_FOLDERS),
        };
        let on_disk = self.disk_checkouts()?;
        let seen = self.sightings().await?;
        let home = dirs::home_dir();
        spawn_blocking(move || {
            let mut out = found::folders(&seen, home.as_deref(), limit, found::checkout);
            found::add_checkouts(&mut out, &on_disk, home.as_deref(), limit);
            found::with_commits(&mut out, found::last_commit);
            out
        })
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

    /// Sessions of `agent`, one other than Claude Code, started outside
    /// Beep, newest first: at most `limit`, or only `session` when given.
    pub(super) async fn stores_external(
        &self,
        agent: &str,
        limit: u32,
        session: Option<String>,
    ) -> Result<Vec<ExternalSession>> {
        let agent = Agent::from_name(agent)
            .filter(|agent| *agent != Agent::Claude)
            .with_context(|| format!("this node lists no {agent} sessions started outside Beep"))?;
        let owned = self.owned();
        let stores = self.stores.clone();
        let mut sessions = spawn_blocking(move || {
            let now = SystemTime::now();
            let mut seen = stores.sessions(Some(agent));
            seen.sort_by_key(|seen| Reverse(seen.last_activity));
            match session {
                Some(id) => seen.retain(|seen| seen.id == id),
                None => seen.truncate(
                    usize::try_from(limit)
                        .unwrap_or(MAX_IN_FOLDERS)
                        .clamp(1, MAX_IN_FOLDERS),
                ),
            }
            seen.into_iter()
                .map(|seen| stores.describe(seen, now))
                .collect::<Vec<_>>()
        })
        .await?;
        for found in &mut sessions {
            found.presence = self.presence(agent, found).await;
            found.taken_over = owned.get(&found.id).cloned();
        }
        Ok(sessions)
    }

    /// The sessions this node runs, by the agent's own id, to the node's id.
    pub(super) fn owned(&self) -> HashMap<String, String> {
        self.lock()
            .values()
            .filter_map(|session| {
                let live = session.live.borrow();
                Some((live.agent_session.clone()?, live.info.id.clone()))
            })
            .collect()
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
            }
        });
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
        let mut found = found;
        found.presence = self.presence(agent, &found).await;
        if let Some(reason) = claude::refuse_takeover(&found) {
            anyhow::bail!("{reason}");
        }
        self.prepare(kind).await?;
        let begin = self.resumed_begin(prompt, id, before.unwrap_or_default());
        let session = self.start(kind, Some(&found.cwd), begin)?;
        self.watch_parent(&session);
        Ok(session)
    }

    /// How a session of this node that ended begins again: with the link to
    /// its parent and the mode it was in, and following the parent when that
    /// runs here.
    pub(super) fn resumed_begin(&self, prompt: Option<String>, id: &str, before: Held) -> Begin {
        let Held { parent, mode } = before;
        let follows = parent.as_ref().and_then(|link| self.parent_live_of(link));
        Begin {
            prompt,
            resume: Some(id.to_owned()),
            mode,
            parent,
            follows,
            ..Begin::default()
        }
    }

    /// What `found`, a session in `agent`'s logs, is doing, once a
    /// recent write alone is checked: Codex keeps a session's rollout open
    /// while it runs, and a session Beep ran here ended with Beep.
    pub(super) async fn presence(&self, agent: Agent, found: &ExternalSession) -> Presence {
        if found.pid.is_some() {
            return found.presence;
        }
        let held = match agent {
            // Claude Code does not keep its transcript open; its processes
            // were looked at already.
            Agent::Claude => None,
            other => {
                let stores = self.stores.clone();
                let id = found.id.clone();
                spawn_blocking(move || holders::held_open(&stores.file(Some(other), &id)?))
                    .await
                    .unwrap_or_else(|e| {
                        log::warn!("could not tell whether session {} runs: {e}", found.id);
                        None
                    })
            }
        };
        holders::settle(found.presence, found.pid, held)
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
}
