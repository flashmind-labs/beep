//! Keeping agent sessions across a node restart: the next start resumes each noted
//! `OpenSession` by the agent's own id, with the prompts that still waited.

use super::{
    Agents, Begin, Command, Prompt,
    family::RemoteKid,
    session::{NODE_STOPPED, Session},
};
use anyhow::{Context, Result};
use beep_proto::node::agent::{Entry, ParentLink, Status};
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, sync::Arc, time::Duration};
use tokio::time::timeout;

/// How long resumed sessions wait for the user's agent settings before
/// starting without them.
pub(super) const SETTINGS_WAIT: Duration = Duration::from_secs(60);

/// The first line of a resumed session's transcript.
const RESUMED: &str = "Resumed after beep-node restarted";

pub(super) const CONTINUE: &str = "The node restarted while you were working. Continue the interrupted task, checking what already completed before repeating any actions. If you were waiting for an answer, ask again.";

/// The most transcript entries kept across a restart for an agent that
/// does not replay its sessions.
const KEPT_ENTRIES: usize = 1000;

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
    session: String,
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
            busy.retain_mut(|live| {
                !matches!(
                    live.borrow_and_update().status(),
                    Status::Idle | Status::Ended
                )
            });
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

    /// What it takes to start `session` again, read from its state in one
    /// go. The state is never read a second time while held: the second
    /// read waits behind a write that came in between, which waits on the
    /// first, and the node stops (#329). Its children come after, from the
    /// agent's id, which is what `name_of` gives once there is one.
    fn as_it_runs(session: &Session, keep: Keep) -> Option<OpenSession> {
        let live = session.live.borrow();
        let stopped = live.status() == Status::Ended
            && live.info.state.error.as_deref() == Some(NODE_STOPPED);
        if keep == Keep::Open && live.status() == Status::Ended && !stopped {
            return None;
        }
        // Kept once the node stopped, so the note is not rewritten with it
        // on every change while the session runs.
        let transcript = if keep == Keep::Open && stopped && !live.replays {
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
                        held.insert(0, Prompt::from(CONTINUE.to_owned()));
                    }
                    held
                }
                Keep::Moving => Vec::new(),
            },
            mode: live.info.mode.clone(),
            parent: live.info.parent.clone(),
            kids: Vec::new(),
            transcript,
        })
    }

    /// Resumes `sessions`, once the user's settings for the agents came or
    /// after `SETTINGS_WAIT`.
    pub(crate) async fn reopen(&self, sessions: Vec<OpenSession>) {
        if sessions.is_empty() {
            self.reopened.send_replace(true);
            return;
        }
        let mut settled = self.settled.subscribe();
        if timeout(SETTINGS_WAIT, settled.wait_for(|settled| *settled))
            .await
            .is_err()
        {
            log::info!("resuming sessions without the agent settings: the mesh gave none");
        }
        let mut sessions = sessions;
        Self::parents_first(&mut sessions);
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
        let _ = self.reopened.subscribe().wait_for(|done| *done).await;
    }

    /// Whether the sessions open before the last stop run again, or there
    /// were none. Until then a session may be about to come back, so one
    /// that does not run is not yet one that ended.
    pub fn reopened(&self) -> bool {
        *self.reopened.borrow()
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
        let cwd = open.cwd.to_string_lossy();
        let follows = open
            .parent
            .as_ref()
            .and_then(|link| self.parent_live_of(link));
        let session = self.start(
            &kind,
            Some(&cwd),
            Begin {
                resume: Some(open.session),
                timestamps: Some((open.started_at, open.last_activity)),
                earlier: open.transcript,
                mode: open.mode,
                parent: open.parent,
                follows,
                note: Some(RESUMED.to_owned()),
                ..Begin::default()
            },
        )?;
        self.watch_parent(&session);
        for kid in open.kids {
            self.adopt_remote(&session, kid);
        }
        for prompt in open.held {
            session
                .order(Command::Prompt(prompt))
                .await
                .map_err(anyhow::Error::msg)?;
        }
        Ok(())
    }
}

#[cfg(all(test, unix))]
#[path = "../tests/agents_restart.rs"]
mod tests;
