//! Keeping agent sessions across a node restart: an update holds prompts
//! until no turn runs (`Hold`), and the next start resumes each noted
//! `OpenSession` by the agent's own id, with the prompts that still waited.

use super::{Agents, Begin, Command, Prompt, family::RemoteKid, session::NODE_STOPPED};
use anyhow::{Context, Result};
use beep_proto::node::agent::{ParentLink, Status};
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, time::Duration};
use tokio::{sync::watch, time::timeout};

/// How long resumed sessions wait for the user's agent settings before
/// starting without them.
const SETTINGS_WAIT: Duration = Duration::from_secs(60);

/// The first line of a resumed session's transcript.
const RESUMED: &str = "Resumed after beep-node restarted";

/// A session the node stopped, to resume when it starts again.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct OpenSession {
    /// The agent, such as `claude`.
    agent: String,
    /// The agent's own id for the session.
    session: String,
    cwd: PathBuf,
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
}

impl OpenSession {
    /// How deep in a chain of starts the session is; 0 for a root.
    pub(super) fn depth(&self) -> u8 {
        self.parent.as_ref().map_or(0, |link| link.depth)
    }
}

/// Prompts wait instead of starting turns while this lives.
pub(crate) struct Hold<'a>(&'a watch::Sender<bool>);

impl Drop for Hold<'_> {
    fn drop(&mut self) {
        self.0.send_replace(false);
    }
}

impl Agents {
    /// Holds new prompts in every session until the guard drops: running
    /// turns finish, the next ones wait.
    pub(crate) fn hold(&self) -> Hold<'_> {
        self.holding.send_replace(true);
        Hold(&self.holding)
    }

    /// Whether prompts are held now, and when that changes.
    #[cfg(test)]
    pub(crate) fn holding_now(&self) -> watch::Receiver<bool> {
        self.holding.subscribe()
    }

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

    /// Holds new prompts, waits up to `limit` for no turn to run, then
    /// runs `restart` still holding them. A session that closes or ends
    /// meanwhile is done.
    #[cfg(feature = "cli")]
    pub(crate) async fn restart_when_idle<T>(
        &self,
        limit: Duration,
        restart: impl Future<Output = T>,
    ) -> T {
        let _hold = self.hold();
        if timeout(limit, self.until_idle()).await.is_err() {
            log::info!("a turn still runs after the wait; restarting anyway");
        }
        restart.await
    }

    /// The sessions to resume when the node starts again: those that run
    /// or ended with the node, once the agent gave them its own id. One
    /// the user closed or that failed stays ended.
    pub(crate) fn open_sessions(&self) -> Vec<OpenSession> {
        self.lock()
            .values()
            .filter_map(|session| {
                let live = session.live.borrow();
                let open = live.status() != Status::Ended
                    || live.info.state.error.as_deref() == Some(NODE_STOPPED);
                if !open {
                    return None;
                }
                Some(OpenSession {
                    agent: session.agent.clone(),
                    session: live.agent_session.clone()?,
                    cwd: PathBuf::from(&live.info.cwd),
                    held: live.held.clone(),
                    mode: live.info.mode.clone(),
                    parent: live.info.parent.clone(),
                    kids: self.kids_to_keep(&Self::name_of(session)),
                })
            })
            .collect()
    }

    /// Resumes `sessions`, once the user's settings for the agents came or
    /// after `SETTINGS_WAIT`.
    pub(crate) async fn reopen(&self, sessions: Vec<OpenSession>) {
        if sessions.is_empty() {
            return;
        }
        let mut settled = self.settled.subscribe();
        if timeout(SETTINGS_WAIT, settled.wait_for(|settled| *settled))
            .await
            .is_err()
        {
            log::info!("resuming sessions without the agent settings: the server sent none");
        }
        let mut sessions = sessions;
        Self::parents_first(&mut sessions);
        for open in sessions {
            let id = open.session.clone();
            if let Err(e) = self.reopen_one(open).await {
                log::warn!("could not resume session {id} after the restart: {e:#}");
            }
        }
    }

    async fn reopen_one(&self, open: OpenSession) -> Result<()> {
        let kind = self
            .kinds
            .iter()
            .find(|kind| kind.name == open.agent)
            .with_context(|| format!("this node no longer runs {}", open.agent))?
            .clone();
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
                mode: open.mode,
                parent: open.parent,
                follows,
                note: Some(RESUMED.to_owned()),
                ..Begin::default()
            },
        )?;
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
    use std::{collections::BTreeMap, path::Path, sync::Arc};

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
        }
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

    /// Starts a session whose first turn asks, holds prompts, queues one
    /// and answers: the wait ends with the turn, the queued prompt unsent.
    async fn held_after_a_turn<'a>(
        agents: &'a Agents,
        kind: &AgentKind,
        cwd: &Path,
    ) -> (Arc<Session>, Hold<'a>) {
        let session = agents
            .start(kind, Some(&cwd.to_string_lossy()), begin("first"))
            .expect("the session starts");
        let mut live = session.live.clone();
        until(&mut live, |live| live.status() == Status::Asking).await;
        let question = live
            .borrow()
            .info
            .question
            .as_ref()
            .expect("the turn asks")
            .id;
        let hold = agents.hold();
        let idle = agents.until_idle();
        tokio::pin!(idle);
        assert!(
            futures::poll!(&mut idle).is_pending(),
            "a turn waiting for an answer is not idle"
        );
        session
            .order(Command::Prompt("after".to_owned().into()))
            .await
            .expect("the prompt is taken");
        session
            .order(Command::Answer {
                question,
                reply: "allow".into(),
            })
            .await
            .expect("the answer is taken");
        timeout(Duration::from_secs(20), idle)
            .await
            .expect("the turn ends and the session is idle");
        let info = session.info();
        assert_eq!(info.turns, 1);
        assert_eq!(info.state.queued, 1, "the next prompt waits");
        (session, hold)
    }

    #[tokio::test]
    async fn an_update_waits_for_the_turn_and_the_next_prompt_goes_after() {
        let dir = tempfile::tempdir().expect("a temporary folder");
        let log = dir.path().join("log");
        let kind = adapter(&log);
        let agents = Agents::new(vec![kind.clone()]);
        let (session, hold) = held_after_a_turn(&agents, &kind, dir.path()).await;
        // The update gave up: the held prompt goes.
        drop(hold);
        let mut live = session.live.clone();
        until(&mut live, |live| live.info.turns == 2).await;
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
        let holding = agents.holding_now();
        let restarted = agents.restart_when_idle(Duration::from_secs(600), async {
            assert!(*holding.borrow(), "prompts stay held through the restart");
        });
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
    async fn a_held_session_resumes_after_the_restart_with_its_prompt() {
        let dir = tempfile::tempdir().expect("a temporary folder");
        let log = dir.path().join("log");
        let kind = adapter(&log);
        let before = Agents::new(vec![kind.clone()]);
        let (_session, _hold) = held_after_a_turn(&before, &kind, dir.path()).await;
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
