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
mod tests {
    use super::*;
    use crate::{
        agents::{AgentKind, Session, live::Live},
        link::restart::{self, Reopen},
    };
    use beep_proto::node::agent::EntryKind;
    use std::{collections::BTreeMap, path::Path, sync::Arc};
    use tokio::sync::watch;

    /// An ACP adapter in shell that logs each line it reads to `$1`. It
    /// asks permission on the prompt "first" and ends that turn once
    /// answered; other prompts end at once. It can load sessions.
    const ADAPTER: &str = r#"
id() { printf '%s\n' "$line" | sed -nE 's/.*"id":("[^"]*"|[0-9]+).*/\1/p'; }
reply() { printf '{"jsonrpc":"2.0","id":%s,"result":%s}\n' "$(id)" "$1"; }
read -r line; printf '%s\n' "$line" >> "$1"
reply '{"protocolVersion":1,"agentCapabilities":{"loadSession":true}}'
while read -r line; do
  printf '%s\n' "$line" >> "$1"
  case "$line" in
    *'"session/new"'*) reply '{"sessionId":"held-1"}' ;;
    *'"session/load"'*) reply '{}' ;;
    *'"text":"first'*) prompt=$(id)
      echo '{"jsonrpc":"2.0","id":"p1","method":"session/request_permission","params":{"sessionId":"held-1","toolCall":{"toolCallId":"t1","title":"Run cargo test"},"options":[{"optionId":"allow","name":"Allow","kind":"allow_once"}]}}' ;;
    *'"p1"'*) printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$prompt" ;;
    *'"session/prompt"'*) reply '{"stopReason":"end_turn"}' ;;
    *'"method"'*'"id"'*|*'"id"'*'"method"'*) reply '{}' ;;
  esac
done
"#;

    fn adapter(log: &Path) -> AgentKind {
        AgentKind {
            name: "fake".into(),
            program: "sh".into(),
            args: vec![
                "-c".into(),
                ADAPTER.into(),
                "fake-adapter".into(),
                log.display().to_string(),
            ],
            env: BTreeMap::new(),
            compact: None,
            compact_models: Vec::new(),
            default_model: None,
        }
    }

    /// `ADAPTER` as an agent that resumes sessions with `session/resume`,
    /// which replays nothing, as flsh before 0.2.8 does.
    fn resuming_adapter(log: &Path) -> AgentKind {
        let mut kind = adapter(log);
        kind.args[1] = ADAPTER.replace(
            r#""agentCapabilities":{"loadSession":true}"#,
            r#""agentCapabilities":{"sessionCapabilities":{"resume":{}}}"#,
        );
        kind
    }

    fn begin(prompt: &str) -> Begin {
        Begin {
            prompt: Some(prompt.into()),
            ..Begin::default()
        }
    }

    async fn until(live: &mut watch::Receiver<Live>, done: impl FnMut(&Live) -> bool) {
        timeout(Duration::from_secs(20), live.wait_for(done))
            .await
            .expect("the session got there in time")
            .expect("the session is kept");
    }

    /// Queues a prompt while the first turn waits for an answer.
    async fn queued_during_a_turn(agents: &Agents, kind: &AgentKind, cwd: &Path) -> Arc<Session> {
        let session = agents
            .start(kind, Some(&cwd.to_string_lossy()), begin("first"))
            .expect("the session starts");
        let mut live = session.live.clone();
        until(&mut live, |live| live.status() == Status::Asking).await;
        session
            .order(Command::Prompt("after".to_owned().into()))
            .await
            .expect("the prompt is taken");
        session
    }

    #[tokio::test]
    async fn an_update_waits_for_the_turn_and_the_next_prompt_goes_after() {
        let dir = tempfile::tempdir().expect("a temporary folder");
        let log = dir.path().join("log");
        let kind = adapter(&log);
        let agents = Agents::new(vec![kind.clone()]);
        let session = queued_during_a_turn(&agents, &kind, dir.path()).await;
        let mut live = session.live.clone();
        let idle = agents.until_idle();
        tokio::pin!(idle);
        assert!(futures::poll!(&mut idle).is_pending());
        let question = live
            .borrow()
            .info
            .question
            .as_ref()
            .expect("the turn asks")
            .id;
        session
            .order(Command::Answer {
                question,
                reply: "allow".into(),
            })
            .await
            .expect("the answer is taken");
        until(&mut live, |live| live.info.turns == 2).await;
        timeout(Duration::from_secs(20), idle)
            .await
            .expect("both turns finish");
        let read = std::fs::read_to_string(&log).expect("the adapter's log");
        assert!(read.contains(r#""text":"after""#), "{read}");
        agents.shutdown().await;
    }

    #[cfg(feature = "cli")]
    #[tokio::test]
    async fn an_update_restarts_once_the_busy_session_closes() {
        let dir = tempfile::tempdir().expect("a temporary folder");
        let kind = adapter(&dir.path().join("log"));
        let agents = Agents::new(vec![kind.clone()]);
        let session = agents
            .start(&kind, Some(&dir.path().to_string_lossy()), begin("first"))
            .expect("the session starts");
        let mut live = session.live.clone();
        until(&mut live, |live| live.status() == Status::Asking).await;
        let restarted = agents.restart_when_idle(Duration::from_secs(600), async {});
        tokio::pin!(restarted);
        assert!(
            futures::poll!(&mut restarted).is_pending(),
            "a turn waiting for an answer holds the restart"
        );
        session.close().await;
        timeout(Duration::from_secs(20), restarted)
            .await
            .expect("a closed session no longer holds the restart");
        agents.shutdown().await;
    }

    #[tokio::test]
    async fn an_interrupted_turn_continues_before_queued_prompts() {
        let dir = tempfile::tempdir().expect("a temporary folder");
        let log = dir.path().join("log");
        let kind = adapter(&log);
        let before = Agents::new(vec![kind.clone()]);
        let session = before
            .start(&kind, Some(&dir.path().to_string_lossy()), begin("first"))
            .expect("the session starts");
        let mut live = session.live.clone();
        until(&mut live, |live| live.status() == Status::Asking).await;
        let _hold = before.hold();
        session
            .order(Command::Prompt("after".to_owned().into()))
            .await
            .expect("queued");
        assert_eq!(before.open_sessions()[0].held[0].text, CONTINUE);
        before.shutdown().await;
        let stopped = before.open_sessions();
        assert_eq!(
            stopped[0]
                .held
                .iter()
                .map(|p| p.text.as_str())
                .collect::<Vec<_>>(),
            vec![CONTINUE, "after"]
        );
        let after = Agents::new(vec![kind]);
        after.settings_arrived();
        after.reopen(stopped).await;
        let resumed = after.get("held-1").expect("resumed");
        let mut live = resumed.live.clone();
        until(&mut live, |live| live.info.turns == 2).await;
        let read = std::fs::read_to_string(&log).expect("adapter log");
        assert!(
            read.find(CONTINUE).expect("continued")
                < read.find(r#""text":"after""#).expect("queued prompt")
        );
        after.shutdown().await;
    }

    #[tokio::test]
    async fn a_held_session_resumes_after_the_restart_with_its_prompt() {
        let dir = tempfile::tempdir().expect("a temporary folder");
        let log = dir.path().join("log");
        let kind = adapter(&log);
        let before = Agents::new(vec![kind.clone()]);
        let _session = queued_during_a_turn(&before, &kind, dir.path()).await;
        before.shutdown().await;
        let stopped = before.open_sessions();
        assert_eq!(stopped.len(), 1);
        assert_eq!(stopped[0].session, "held-1");
        assert_eq!(stopped[0].held, [Prompt::from("after".to_owned())]);
        let read = std::fs::read_to_string(&log).expect("the adapter's log");
        assert!(!read.contains(r#""text":"after""#), "{read}");

        let after = Agents::new(vec![kind]);
        after.settings_arrived();
        after.reopen(stopped).await;
        let session = after.get("held-1").expect("resumed under the agent's id");
        let mut live = session.live.clone();
        until(&mut live, |live| live.info.turns == 1).await;
        let read = std::fs::read_to_string(&log).expect("the adapter's log");
        assert!(read.contains("session/load"), "{read}");
        assert!(read.contains(r#""text":"after""#), "{read}");
        after.shutdown().await;
    }

    #[tokio::test]
    async fn a_session_resumed_without_a_replay_keeps_its_transcript() {
        let dir = tempfile::tempdir().expect("a temporary folder");
        let log = dir.path().join("log");
        let kind = resuming_adapter(&log);
        let before = Agents::new(vec![kind.clone()]);
        let session = before
            .start(&kind, Some(&dir.path().to_string_lossy()), begin("hello"))
            .expect("the session starts");
        let mut live = session.live.clone();
        until(&mut live, |live| live.info.turns == 1).await;
        before.shutdown().await;
        let mut stopped = before.open_sessions();
        assert_eq!(stopped.len(), 1);
        stopped[0].started_at = 100;
        stopped[0].last_activity = 200;

        let after = Agents::new(vec![kind]);
        after.settings_arrived();
        after.reopen(stopped).await;
        let resumed = after.get("held-1").expect("resumed under the agent's id");
        let mut live = resumed.live.clone();
        until(&mut live, |live| live.status() == Status::Idle).await;
        assert_eq!(live.borrow().info.started_at, 100);
        assert_eq!(live.borrow().info.last_activity, 200);
        let read = std::fs::read_to_string(&log).expect("the adapter's log");
        assert!(read.contains("session/resume"), "{read}");
        let shown: Vec<(EntryKind, String)> = live
            .borrow()
            .transcript(100)
            .0
            .into_iter()
            .map(|entry| (entry.kind, entry.text))
            .collect();
        assert_eq!(
            shown,
            [
                (EntryKind::Other, RESUMED.to_owned()),
                (EntryKind::User, "hello".to_owned()),
            ]
        );
        after.shutdown().await;
    }

    #[tokio::test]
    async fn a_session_the_agent_replays_keeps_no_transcript() {
        let dir = tempfile::tempdir().expect("a temporary folder");
        let log = dir.path().join("log");
        let kind = adapter(&log);
        let before = Agents::new(vec![kind.clone()]);
        let session = before
            .start(&kind, Some(&dir.path().to_string_lossy()), begin("hello"))
            .expect("the session starts");
        let mut live = session.live.clone();
        until(&mut live, |live| live.info.turns == 1).await;
        before.shutdown().await;
        let stopped = before.open_sessions();
        assert_eq!(stopped.len(), 1);
        assert!(stopped[0].transcript.is_empty(), "{stopped:?}");
    }

    /// The note of the first session, once `ready` holds for what it keeps.
    async fn note_with(dir: &Path, ready: impl Fn(&OpenSession) -> bool) -> OpenSession {
        timeout(Duration::from_secs(20), async {
            loop {
                if let Some(open) = Reopen::read(dir).sessions.into_iter().find(&ready) {
                    break open;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("the note holds it")
    }

    #[tokio::test]
    async fn a_child_on_another_machine_is_noted_and_comes_back_after_a_crash() {
        let dir = tempfile::tempdir().expect("a temporary folder");
        let log = dir.path().join("log");
        let kind = adapter(&log);
        let before = Arc::new(Agents::new(vec![kind.clone()]));
        let keeping = {
            let (dir, agents) = (dir.path().to_path_buf(), before.clone());
            tokio_util::task::AbortOnDropHandle::new(tokio::spawn(async move {
                restart::keep(&dir, &agents).await;
            }))
        };
        let session = before
            .start(&kind, Some(&dir.path().to_string_lossy()), begin("hello"))
            .expect("the session starts");
        let mut live = session.live.clone();
        until(&mut live, |live| live.info.turns == 1).await;
        let name = Agents::name_of(&session);
        let mut kid = RemoteKid::expected(&name, iroh::SecretKey::generate().public(), "box");
        kid.id = "k1".into();
        // Adopting changes the note with no session opening or closing.
        before.adopt_remote(&session, kid.clone());
        let noted = note_with(dir.path(), |open| open.kids == [kid.clone()]).await;
        // Ending it changes the note too.
        before.end_remote(&kid);
        let ended = note_with(dir.path(), |open| {
            !open.kids.is_empty() && open.kids.iter().all(|kid| kid.ended)
        })
        .await;
        assert_eq!(ended.kids.len(), 1);
        // The node dies here: the note is what is left.
        drop(keeping);

        let after = Agents::new(vec![kind]);
        after.settings_arrived();
        after.reopen(vec![noted]).await;
        let resumed = after.get(&name).expect("resumed under the agent's id");
        assert_eq!(after.remote_kids(&Agents::name_of(&resumed)), vec![kid]);
        before.shutdown().await;
        after.shutdown().await;
    }

    #[tokio::test]
    async fn a_session_comes_back_after_a_crash() {
        let dir = tempfile::tempdir().expect("a temporary folder");
        let log = dir.path().join("log");
        let kind = adapter(&log);
        let before = Arc::new(Agents::new(vec![kind.clone()]));
        let keeping = {
            let (dir, agents) = (dir.path().to_path_buf(), before.clone());
            tokio_util::task::AbortOnDropHandle::new(tokio::spawn(async move {
                restart::keep(&dir, &agents).await;
            }))
        };
        let session = before
            .start(&kind, Some(&dir.path().to_string_lossy()), begin("hello"))
            .expect("the session starts");
        let mut live = session.live.clone();
        until(&mut live, |live| live.info.turns == 1).await;
        let noted = timeout(Duration::from_secs(20), async {
            loop {
                let note = Reopen::read(dir.path());
                if !note.sessions.is_empty() {
                    break note;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("the running session is noted");
        // The node dies here: nothing stops, the note is what is left.
        drop(keeping);
        assert_eq!(Reopen::read(dir.path()).sessions, noted.sessions);

        let after = Agents::new(vec![kind]);
        after.settings_arrived();
        after.reopen(noted.sessions).await;
        let resumed = after.get("held-1").expect("resumed under the agent's id");
        let mut live = resumed.live.clone();
        until(&mut live, |live| live.status() == Status::Idle).await;
        let read = std::fs::read_to_string(&log).expect("the adapter's log");
        assert!(read.contains("session/load"), "{read}");
        before.shutdown().await;
        after.shutdown().await;
    }
}
