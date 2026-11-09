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
    assert_eq!(
        stopped[0].held,
        [
            Prompt::from(CONTINUE.to_owned()),
            Prompt::from("after".to_owned())
        ]
    );
    let read = std::fs::read_to_string(&log).expect("the adapter's log");
    assert!(!read.contains(r#""text":"after""#), "{read}");

    let after = Agents::new(vec![kind]);
    after.settings_arrived();
    after.reopen(stopped).await;
    let session = after.get("held-1").expect("resumed under the agent's id");
    let mut live = session.live.clone();
    until(&mut live, |live| live.info.turns == 2).await;
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
