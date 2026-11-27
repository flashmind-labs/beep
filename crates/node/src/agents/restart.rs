//! Keeping agent sessions across a node restart: the next start resumes each noted
//! `OpenSession` by the agent's own id, with the prompts that still waited.

use super::{
    AgentKind, Begin,
    acp::Prompt,
    checkout, moving,
    reopen::{OpenSession, Reopen, SavedSession},
    running::Starter,
    session::{CONTINUE, Command, NODE_STOPPED, Session},
    sessions::find_session,
};
use crate::logs::found::Agent;
use anyhow::{Context, Result};
use beep_proto::SessionId;
use beep_proto::node::agent::{SessionInfo, Status};
use beep_proto::watched;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::task::spawn_blocking;

/// The first line of a resumed session's transcript.
const RESUMED: &str = "Resumed after beep-node restarted";

/// The most transcript entries kept across a restart for an agent that
/// does not replay its sessions.
const KEPT_ENTRIES: usize = 1000;

/// The most sessions the note keeps, for sessions taken over later.
const KEPT_SAVED: usize = 500;

/// Why a session is read to start again.
#[derive(Clone, Copy, PartialEq)]
enum Keep {
    /// For the note: one that ended for good is left out, and its held
    /// prompts go along.
    Open,
    /// For a move to another machine, as it runs now.
    Moving,
}

impl OpenSession {
    /// How deep in a chain of starts the session is; 0 for a root.
    pub(super) fn depth(&self) -> u8 {
        self.parent.as_ref().map_or(0, |link| link.depth)
    }
}

impl Reopen {
    /// Finishes once no session runs a turn or waits for an answer.
    pub(crate) async fn until_idle(&self) {
        loop {
            let mut busy: Vec<_> = self
                .sessions
                .read()
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
        limit: std::time::Duration,
        restart: impl Future<Output = T>,
    ) -> T {
        if tokio::time::timeout(limit, self.until_idle())
            .await
            .is_err()
        {
            log::info!("a turn still runs after the wait; restarting anyway");
        }

        restart.await
    }

    /// The sessions to resume when the node starts again: those that run
    /// or ended with the node, once the agent gave them its own id. One
    /// the user closed or that failed stays ended. A reviewer is temporary:
    /// its review ends with the node.
    pub(crate) fn open_sessions(&self) -> Vec<OpenSession> {
        let sessions: Vec<Arc<Session>> = self
            .sessions
            .read()
            .values()
            .filter(|session| !session.reviewer)
            .cloned()
            .collect();
        sessions
            .iter()
            .filter_map(|session| {
                let mut open = Self::as_it_runs(session, Keep::Open)?;
                open.kids = self.links.kids_to_keep(&open.session);
                Some(open)
            })
            .collect()
    }

    /// What sessions here were, the ones running now last, added to those
    /// kept from before. Reviewers are kept too, so one taken over after a
    /// restart is still known as one.
    pub(crate) fn saved_sessions(&self) -> Vec<SavedSession> {
        let sessions: Vec<Arc<Session>> = self.sessions.read().values().cloned().collect();
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

    /// Keeps `saved`, a session that came from another machine ended, as
    /// one that ended here.
    pub(crate) fn keep_arrived(&self, saved: SavedSession) {
        kept_with(&mut crate::lock(&self.saved), vec![saved], |kept| {
            &kept.info.id
        });
        crate::events::open_changed();
    }

    /// What session `id`, by the agent's own id, is: from its state when it
    /// is here, else as kept from before.
    pub(crate) fn saved(&self, id: &SessionId) -> Option<SavedSession> {
        if let Some(saved) =
            find_session(&self.sessions.read(), id).and_then(|session| Self::saved_of(&session))
        {
            return Some(saved);
        }

        crate::lock(&self.saved)
            .iter()
            .rfind(|kept| kept.info.id == *id)
            .cloned()
    }

    /// Drops what was kept of session `id`, by the agent's own id, once it
    /// is deleted.
    pub(super) fn forget_saved(&self, id: &SessionId) {
        crate::lock(&self.saved).retain(|kept| kept.info.id != *id);
    }

    /// The linked worktrees session `id` created, as `saved` gives them.
    pub(crate) fn created_worktrees(&self, id: &SessionId) -> Vec<String> {
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
                    account: info.account.clone(),
                    persona: info.persona.clone(),
                    continues: info.continues.clone(),
                    issue: info.issue,
                    ..SessionInfo::default()
                },
                effort: live.effort,
                fast: live.fast,
                account: info.account.clone(),
                named: live.named,
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
                            held.insert(0, Prompt::from(crate::tags::context(CONTINUE)));
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
                fast: live.fast,
                account: session.account.clone(),
                named: live.named,
                persona: live.info.persona.clone(),
                continues: live.info.continues.clone(),
                issue: live.info.issue,
            })
        })
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
    pub(crate) fn open_session(&self, id: &SessionId) -> Option<OpenSession> {
        let session = find_session(&self.sessions.read(), id)?;
        let mut open = Self::as_it_runs(&session, Keep::Moving)?;
        open.kids = self.links.kids_to_keep(&open.session);
        Some(open)
    }
}

impl Starter {
    /// Resumes `sessions`, once the user's settings for the agents came or
    /// after `SETTINGS_WAIT`.
    pub(crate) async fn reopen(&self, sessions: Vec<OpenSession>) {
        if sessions.is_empty() {
            self.reopen.reopened.send_replace(true);
            return;
        }

        if !self.catalog.settings_came().await {
            log::info!("resuming sessions without the agent settings: the mesh gave none");
        }

        // A parent starts before its children: one depth after another, the
        // sessions of a depth together.
        let mut depths: BTreeMap<u8, Vec<OpenSession>> = BTreeMap::new();

        for open in sessions {
            depths.entry(open.depth()).or_default().push(open);
        }

        for group in depths.into_values() {
            futures::future::join_all(group.into_iter().map(|open| async move {
                let id = open.session.clone();

                if let Err(e) = self.reopen_one(open).await {
                    log::warn!("could not resume session {id} after the restart: {e:#}");
                }
            }))
            .await;
        }

        self.reopen.reopened.send_replace(true);
    }

    pub(crate) async fn reopen_one(&self, open: OpenSession) -> Result<()> {
        let kind = self
            .catalog
            .named(&open.agent)
            .with_context(|| format!("this node no longer runs {}", open.agent))?;
        self.catalog.prepare(&kind).await?;
        let cwd = ready_to_resume(&kind, &open.session, &open.cwd).await;
        let cwd = cwd.to_string_lossy();
        let follows = open
            .parent
            .as_ref()
            .and_then(|link| self.family.parent_live_of(link));
        let session = self
            .start(
                &kind,
                Some(&cwd),
                Begin {
                    resume: Some(open.session),
                    timestamps: Some((open.started_at, open.last_activity)),
                    earlier: open.transcript,
                    model: open.model,
                    title: open.title,
                    named: open.named,
                    mode: open.mode,
                    parent: open.parent,
                    follows,
                    note: Some(RESUMED.to_owned()),
                    worktrees: open.worktrees,
                    assistant: open.assistant,
                    effort: open.effort,
                    fast: open.fast,
                    account: open.account,
                    persona: open.persona,
                    continues: open.continues,
                    issue: open.issue,
                    ..Begin::default()
                },
            )
            .await?;
        self.family.watch_parent(&session);

        if open.device.is_some() {
            session
                .order(Command::SetDevice(open.device))
                .await
                .map_err(anyhow::Error::msg)?;
        }

        for kid in open.kids {
            self.links.adopt_remote(&session, kid);
        }

        for mut prompt in open.held {
            if prompt.text == CONTINUE {
                prompt.text = crate::tags::context(CONTINUE);
            }

            session
                .order(Command::Prompt(prompt))
                .await
                .map_err(anyhow::Error::msg)?;
        }

        Ok(())
    }
}

/// The folder resumed session `id` of `kind`, kept for `cwd`, starts in, so
/// that a session can always go on: `cwd`, which `Starter::start` turns into
/// the project folder when it is a worktree, even a removed one, or the home
/// folder when `cwd` is gone and was no worktree. Claude Code's transcript
/// goes along; a failure there only costs the history.
pub(super) async fn ready_to_resume(kind: &AgentKind, id: &SessionId, cwd: &Path) -> PathBuf {
    let claude = kind.name.parse::<Agent>() == Ok(Agent::Claude);
    let (id, kept) = (id.to_owned(), cwd.to_owned());
    let ready = spawn_blocking(move || {
        let project = checkout::project_of(&kept);
        let home = (project.is_none() && !kept.is_dir())
            .then(dirs::home_dir)
            .flatten();

        if claude
            && let Some(to) = project.as_ref().or(home.as_ref())
            && let Err(e) = moving::rehome_claude(&id, &kept, to)
        {
            log::warn!(
                "could not carry a Claude transcript to {}: {e:#}",
                to.display()
            );
        }

        home.unwrap_or(kept)
    })
    .await;

    ready.unwrap_or_else(|_| cwd.to_owned())
}

#[cfg(all(test, unix))]
#[path = "../tests/agents_restart.rs"]
mod tests;

/// `kept` with `now` in place of the entries of the same sessions, the
/// newest `KEPT_SAVED`; a copy of the result.
fn kept_with<T: Clone>(
    kept: &mut Vec<T>,
    now: Vec<T>,
    session: impl Fn(&T) -> &SessionId,
) -> Vec<T> {
    kept.retain(|old| !now.iter().any(|new| session(new) == session(old)));
    kept.extend(now);
    let over = kept.len().saturating_sub(KEPT_SAVED);
    kept.drain(..over);
    kept.clone()
}
