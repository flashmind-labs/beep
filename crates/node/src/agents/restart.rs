//! Keeping agent sessions across a node restart: the next start resumes each noted
//! `OpenSession` by the agent's own id, with the prompts that still waited.

use super::{
    AgentKind, Agents, Begin, Command, Prompt, checkout,
    found::Agent,
    moving,
    family::RemoteKid,
    session::{NODE_STOPPED, Session},
};
use anyhow::{Context, Result};
use beep_proto::watched;
use beep_proto::{
    ModelId,
    node::agent::{Effort, Entry, ParentLink, SessionDevice, SessionInfo, Status},
};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::{task::spawn_blocking, time::timeout};

/// How long resumed sessions wait for the user's agent settings before
/// starting without them.
pub(super) const SETTINGS_WAIT: Duration = Duration::from_secs(60);

/// The first line of a resumed session's transcript.
const RESUMED: &str = "Resumed after beep-node restarted";

pub(super) const CONTINUE: &str = "The node restarted while you were working. Continue the interrupted task, checking what already completed before repeating any actions. If you were waiting for an answer, ask again.";

/// The most transcript entries kept across a restart for an agent that
/// does not replay its sessions.
const KEPT_ENTRIES: usize = 1000;

/// The most sessions the note keeps, for sessions taken over later.
const KEPT_SAVED: usize = 500;

/// What the node keeps of a session, also after it ended, so it goes on as
/// it was when it is resumed, taken over or moved: its `SessionInfo` as
/// listed, and the thinking level, which the info does not carry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct SavedSession {
    /// By the agent's own id.
    pub(crate) info: SessionInfo,
    #[serde(default)]
    pub(crate) effort: Option<Effort>,
}

/// Why a session is read to start again.
#[derive(Clone, Copy, PartialEq)]
enum Keep {
    /// For the note: one that ended for good is left out, and its held
    /// prompts go along.
    Open,
    /// For a move to another machine, as it runs now.
    Moving,
}

/// A session the node stopped, to resume when it starts again.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct OpenSession {
    /// The agent, such as `claude`.
    agent: String,
    /// The agent's own id for the session.
    pub(crate) session: String,
    cwd: PathBuf,
    #[serde(default)]
    started_at: u64,
    #[serde(default)]
    last_activity: u64,
    /// Prompts that had not gone to the agent yet.
    held: Vec<Prompt>,
    /// The session mode it was in.
    #[serde(default)]
    mode: Option<String>,
    /// The session that started it with `start_agent`.
    #[serde(default)]
    parent: Option<ParentLink>,
    /// The children it started on other machines.
    #[serde(default)]
    pub(super) kids: Vec<RemoteKid>,
    /// Its last transcript entries, kept when the node stopped, for an
    /// agent that resumes with `session/resume`, which replays nothing.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    transcript: Vec<Entry>,
    /// The model it ran on. A resumed agent starts on its own default otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    model: Option<ModelId>,
    #[serde(default)]
    title: Option<String>,
    /// The device it used, kept as `SessionInfo::device`.
    #[serde(default)]
    device: Option<SessionDevice>,
    /// The linked worktrees it created, kept as `SessionInfo::worktrees`.
    #[serde(default)]
    worktrees: Vec<String>,
    /// Whether it is the home assistant.
    #[serde(default)]
    assistant: bool,
    /// The thinking level it ran at.
    #[serde(default)]
    effort: Option<Effort>,
}

impl From<&OpenSession> for SavedSession {
    /// What a note from before saved sessions knows of open session `open`.
    /// It said nothing of the assistant: the home folder does.
    fn from(open: &OpenSession) -> Self {
        let cwd = open.cwd.to_string_lossy().into_owned();
        let assistant = open.assistant
            || super::running::home_assistant(&open.cwd, open.parent.is_some());

        Self {
            info: SessionInfo {
                id: open.session.clone(),
                agent: open.agent.clone(),
                cwd,
                title: open.title.clone(),
                mode: open.mode.clone(),
                parent: open.parent.clone(),
                model: open.model.clone(),
                device: open.device.clone(),
                worktrees: open.worktrees.clone(),
                assistant,
                ..SessionInfo::default()
            },
            effort: open.effort,
        }
    }
}

impl OpenSession {
    /// How deep in a chain of starts the session is; 0 for a root.
    pub(super) fn depth(&self) -> u8 {
        self.parent.as_ref().map_or(0, |link| link.depth)
    }
}

impl Agents {
    /// Finishes once no session runs a turn or waits for an answer.
    pub(crate) async fn until_idle(&self) {
        loop {
            let mut busy: Vec<_> = self
                .lock()
                .values()
                .map(|session| session.live.clone())
                .collect();
            busy.retain_mut(|live| watched::read_and_update(live, |live| live.info.state.runs()));

            if busy.is_empty() {
                return;
            }
            // Any change to a busy session looks again.
            let changes = busy.iter_mut().map(|live| Box::pin(live.changed()));
            let _ = futures::future::select_all(changes).await;
        }
    }

    /// Waits up to `limit` for sessions to become idle, then restarts.
    #[cfg(feature = "cli")]
    pub(crate) async fn restart_when_idle<T>(
        &self,
        limit: Duration,
        restart: impl Future<Output = T>,
    ) -> T {
        if timeout(limit, self.until_idle()).await.is_err() {
            log::info!("a turn still runs after the wait; restarting anyway");
        }

        restart.await
    }

    /// The sessions to resume when the node starts again: those that run
    /// or ended with the node, once the agent gave them its own id. One
    /// the user closed or that failed stays ended. A reviewer is temporary:
    /// its review ends with the node.
    pub(crate) fn open_sessions(&self) -> Vec<OpenSession> {
        // Copied out, so no request that lists the sessions waits on the
        // reads below.
        let sessions: Vec<Arc<Session>> = self
            .lock()
            .values()
            .filter(|session| !session.reviewer)
            .cloned()
            .collect();
        sessions
            .iter()
            .filter_map(|session| {
                let mut open = Self::as_it_runs(session, Keep::Open)?;
                open.kids = self.kids_to_keep(&open.session);
                Some(open)
            })
            .collect()
    }

    /// What sessions here were, the ones running now last, added to those
    /// kept from before. Reviewers are kept too, so one taken over after a
    /// restart is still known as one.
    pub(crate) fn saved_sessions(&self) -> Vec<SavedSession> {
        // Copied out, as in `open_sessions`.
        let sessions: Vec<Arc<Session>> = self.lock().values().cloned().collect();
        let now: Vec<SavedSession> = sessions
            .iter()
            .filter_map(|session| Self::saved_of(session))
            .collect();
        kept_with(&mut crate::lock(&self.saved), now, |kept| &kept.info.id)
    }

    /// Takes the sessions kept in the note from before the node started.
    pub(crate) fn keep_saved(&self, saved: Vec<SavedSession>) {
        *crate::lock(&self.saved) = saved;
    }

    /// What session `id`, by the agent's own id, is: from its state when it
    /// is here, else as kept from before.
    pub(crate) fn saved(&self, id: &str) -> Option<SavedSession> {
        if let Some(saved) = self.get(id).and_then(|session| Self::saved_of(&session)) {
            return Some(saved);
        }

        crate::lock(&self.saved)
            .iter()
            .rfind(|kept| kept.info.id == id)
            .cloned()
    }

    /// The linked worktrees session `id` created, as `saved` gives them.
    pub(crate) fn created_worktrees(&self, id: &str) -> Vec<String> {
        self.saved(id)
            .map(|saved| saved.info.worktrees)
            .unwrap_or_default()
    }

    /// What `session` is now, once the agent gave it its own id: what it
    /// goes on with, and whether it is a reviewer or the assistant as
    /// listings say. Its state, spend and last words are left out.
    fn saved_of(session: &Session) -> Option<SavedSession> {
        watched::read(&session.live, |live| {
            live.agent_session.as_ref()?;
            let info = &live.info;
            Some(SavedSession {
                info: SessionInfo {
                    id: info.id.clone(),
                    agent: info.agent.clone(),
                    cwd: info.cwd.clone(),
                    title: info.title.clone(),
                    mode: info.mode.clone(),
                    parent: info.parent.clone(),
                    reviewer: session.reviewer,
                    model: info.model.clone(),
                    device: info.device.clone(),
                    worktrees: info.worktrees.clone(),
                    assistant: session.assistant,
                    ..SessionInfo::default()
                },
                effort: live.effort,
            })
        })
    }

    /// What it takes to start `session` again, read from its state in one
    /// go. The state is never read a second time while held: the second
    /// read waits behind a write that came in between, which waits on the
    /// first, and the node stops (#329). Its children come after, from the
    /// agent's id, which is what `name_of` gives once there is one.
    fn as_it_runs(session: &Session, keep: Keep) -> Option<OpenSession> {
        watched::read(&session.live, |live| {
            let stopped = live.status() == Status::Ended
                && live.info.state.error.as_deref() == Some(NODE_STOPPED);
            if keep == Keep::Open && live.status() == Status::Ended && !stopped {
                return None;
            }
            // Keep dates even for adapters that replay their history.
            let transcript = if keep == Keep::Open {
                live.transcript_before(KEPT_ENTRIES, None).0
            } else {
                Vec::new()
            };
            Some(OpenSession {
                agent: session.agent.clone(),
                session: live.agent_session.clone()?,
                cwd: PathBuf::from(&live.info.cwd),
                started_at: live.info.started_at,
                last_activity: live.info.last_activity,
                held: match keep {
                    Keep::Open => {
                        let mut held = live.held.clone();
                        if matches!(live.status(), Status::Working | Status::Asking) {
                            held.insert(0, Prompt::from(crate::agents::tags::context(CONTINUE)));
                        }
                        held
                    }
                    Keep::Moving => Vec::new(),
                },
                mode: live.info.mode.clone(),
                parent: live.info.parent.clone(),
                kids: Vec::new(),
                transcript,
                model: live.info.model.clone(),
                title: live.info.title.clone(),
                device: live.info.device.clone(),
                worktrees: live.info.worktrees.clone(),
                assistant: session.assistant,
                effort: live.effort,
            })
        })
    }

    /// Waits for the user's agent settings for `SETTINGS_WAIT` at most;
    /// whether they came.
    pub(super) async fn settings_came(&self) -> bool {
        let mut settled = self.settled.subscribe();

        timeout(SETTINGS_WAIT, watched::wait_for(&mut settled, |settled| *settled))
            .await
            .is_ok()
    }

    /// Resumes `sessions`, once the user's settings for the agents came or
    /// after `SETTINGS_WAIT`.
    pub(crate) async fn reopen(&self, sessions: Vec<OpenSession>) {
        if sessions.is_empty() {
            self.reopened.send_replace(true);
            return;
        }

        if !self.settings_came().await {
            log::info!("resuming sessions without the agent settings: the mesh gave none");
        }

        let mut sessions = sessions;
        // A parent starts before its children.
        sessions.sort_by_key(|open| open.depth());

        for open in sessions {
            let id = open.session.clone();

            if let Err(e) = self.reopen_one(open).await {
                log::warn!("could not resume session {id} after the restart: {e:#}");
            }
        }

        self.reopened.send_replace(true);
    }

    /// Waits until the sessions that were open before the last stop run
    /// again, so a move does not look for a session that is about to come
    /// back.
    pub(crate) async fn until_reopened(&self) {
        // An error means the node is gone, which also ends the wait.
        let _ = watched::wait_for(&mut self.reopened.subscribe(), |done| *done).await;
    }

    /// Whether the sessions open before the last stop run again, or there
    /// were none. Until then a session may be about to come back, so one
    /// that does not run is not yet one that ended.
    pub fn reopened(&self) -> bool {
        watched::read_sent(&self.reopened, |done| *done)
    }

    /// What it takes to start session `id` again, as it runs now.
    pub(crate) fn open_session(&self, id: &str) -> Option<OpenSession> {
        let session = self.get(id)?;
        let mut open = Self::as_it_runs(&session, Keep::Moving)?;
        open.kids = self.kids_to_keep(&open.session);
        Some(open)
    }

    pub(crate) async fn reopen_one(&self, open: OpenSession) -> Result<()> {
        let kind = self
            .named(&open.agent)
            .with_context(|| format!("this node no longer runs {}", open.agent))?;
        self.prepare(&kind).await?;
        to_project(&kind, &open.session, &open.cwd).await;
        let cwd = open.cwd.to_string_lossy();
        let follows = open
            .parent
            .as_ref()
            .and_then(|link| self.parent_live_of(link));
        // A note from before the saved sessions says nothing of the
        // assistant: the home folder does.
        let assistant = open.assistant
            || self.saved(&open.session).map_or_else(
                || super::running::home_assistant(&open.cwd, open.parent.is_some()),
                |saved| saved.info.assistant,
            );
        let session = self.start(
            &kind,
            Some(&cwd),
            Begin {
                resume: Some(open.session),
                timestamps: Some((open.started_at, open.last_activity)),
                earlier: open.transcript,
                model: open.model,
                title: open.title,
                mode: open.mode,
                parent: open.parent,
                follows,
                note: Some(RESUMED.to_owned()),
                worktrees: open.worktrees,
                assistant,
                effort: open.effort,
                ..Begin::default()
            },
        )?;
        self.watch_parent(&session);

        if open.device.is_some() {
            session
                .order(Command::SetDevice(open.device))
                .await
                .map_err(anyhow::Error::msg)?;
        }

        for kid in open.kids {
            self.adopt_remote(&session, kid);
        }

        for mut prompt in open.held {
            if prompt.text == CONTINUE {
                prompt.text = crate::agents::tags::context(CONTINUE);
            }

            session
                .order(Command::Prompt(prompt))
                .await
                .map_err(anyhow::Error::msg)?;
        }

        Ok(())
    }
}

/// Readies resumed session `id` of `kind`, kept for `cwd`, to run in its
/// project folder when `cwd` is in a worktree (`Agents::start`): Claude
/// Code's transcript goes along. A failure only costs the history.
pub(super) async fn to_project(kind: &AgentKind, id: &str, cwd: &Path) {
    if kind.name.parse::<Agent>() != Ok(Agent::Claude) {
        return;
    }

    let (id, cwd) = (id.to_owned(), cwd.to_owned());
    let moved = spawn_blocking(move || {
        let Some(project) = checkout::project_of(&cwd) else {
            return Ok(());
        };
        moving::rehome_claude(&id, &cwd, &project)
    })
    .await;

    if let Ok(Err(e)) | Err(e) = moved.map_err(anyhow::Error::from) {
        log::warn!("could not carry a Claude transcript to its project folder: {e:#}");
    }
}

#[cfg(all(test, unix))]
#[path = "../tests/agents_restart.rs"]
mod tests;

/// `kept` with `now` in place of the entries of the same sessions, the
/// newest `KEPT_SAVED`; a copy of the result.
fn kept_with<T: Clone>(kept: &mut Vec<T>, now: Vec<T>, session: impl Fn(&T) -> &String) -> Vec<T> {
    kept.retain(|old| !now.iter().any(|new| session(new) == session(old)));
    kept.extend(now);
    let over = kept.len().saturating_sub(KEPT_SAVED);
    kept.drain(..over);
    kept.clone()
}
