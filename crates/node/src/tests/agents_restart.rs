use crate::agents::sessions;
use super::*;
use crate::agents::Parts;
use crate::{
    agents::{AgentKind, Session, family::RemoteKid, live::Live},
    link::restart::{self, Reopen},
};
use beep_proto::ModelId;
use beep_proto::node::agent::{EntryKind, ExternalSession, ParentLink};
use beep_proto::watched;
use std::{collections::BTreeMap, path::Path, sync::Arc};
use tokio::{sync::watch, time::timeout};
use std::time::Duration;

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
        bridge: None,
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
    timeout(Duration::from_secs(20), watched::wait_for(live, done))
        .await
        .expect("the session got there in time")
        .expect("the session is kept");
}

/// Queues a prompt while the first turn waits for an answer.
async fn queued_during_a_turn(agents: &Agents, kind: &AgentKind, cwd: &Path) -> Arc<Session> {
    let session = agents
        .start(kind, Some(&cwd.to_string_lossy()), begin("first"))
        .await.expect("the session starts");
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
    let agents = Agents::new(Parts { kinds: vec![kind.clone()], ..Parts::default() });
    let session = queued_during_a_turn(&agents, &kind, dir.path()).await;
    let mut live = session.live.clone();
    let idle = agents.reopen.until_idle();
    tokio::pin!(idle);
    assert!(futures::poll!(&mut idle).is_pending());
    let question = watched::read(&live, |live| live.info.question.clone())
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
    let agents = Agents::new(Parts { kinds: vec![kind.clone()], ..Parts::default() });
    let session = agents
        .start(&kind, Some(&dir.path().to_string_lossy()), begin("first"))
        .await.expect("the session starts");
    let mut live = session.live.clone();
    until(&mut live, |live| live.status() == Status::Asking).await;
    let restarted = agents.reopen.restart_when_idle(Duration::from_secs(600), async {});
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
    let before = Agents::new(Parts { kinds: vec![kind.clone()], ..Parts::default() });
    let session = before
        .start(&kind, Some(&dir.path().to_string_lossy()), begin("first"))
        .await.expect("the session starts");
    let mut live = session.live.clone();
    until(&mut live, |live| live.status() == Status::Asking).await;
    session
        .order(Command::Prompt("after".to_owned().into()))
        .await
        .expect("queued");
    assert_eq!(
        before.reopen.open_sessions()[0].held[0].text,
        crate::tags::context(CONTINUE)
    );
    before.shutdown().await;
    let stopped = before.reopen.open_sessions();
    assert_eq!(
        stopped[0]
            .held
            .iter()
            .map(|p| p.text.clone())
            .collect::<Vec<_>>(),
        vec![crate::tags::context(CONTINUE), "after".to_owned()]
    );
    let after = Agents::new(Parts { kinds: vec![kind], ..Parts::default() });
    after.catalog.settings_arrived();
    after.reopen(stopped).await;
    let resumed = after.sessions.get(&SessionId::from("held-1")).expect("resumed");
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
    let before = Agents::new(Parts { kinds: vec![kind.clone()], ..Parts::default() });
    let _session = queued_during_a_turn(&before, &kind, dir.path()).await;
    before.shutdown().await;
    let stopped = before.reopen.open_sessions();
    assert_eq!(stopped.len(), 1);
    assert_eq!(stopped[0].session, "held-1");
    assert_eq!(stopped[0].title.as_deref(), Some("first"));
    assert_eq!(
        stopped[0].held,
        [
            Prompt::from(crate::tags::context(CONTINUE)),
            Prompt::from("after".to_owned())
        ]
    );
    let read = std::fs::read_to_string(&log).expect("the adapter's log");
    assert!(!read.contains(r#""text":"after""#), "{read}");

    let after = Agents::new(Parts { kinds: vec![kind], ..Parts::default() });
    after.catalog.settings_arrived();
    after.reopen(stopped).await;
    let session = after.sessions.get(&SessionId::from("held-1")).expect("resumed under the agent's id");
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
    let before = Agents::new(Parts { kinds: vec![kind.clone()], ..Parts::default() });
    let session = before
        .start(&kind, Some(&dir.path().to_string_lossy()), begin("hello"))
        .await.expect("the session starts");
    let mut live = session.live.clone();
    until(&mut live, |live| live.info.turns == 1).await;
    before.shutdown().await;
    let mut stopped = before.reopen.open_sessions();
    assert_eq!(stopped.len(), 1);
    stopped[0].started_at = 100;
    stopped[0].last_activity = 200;

    let after = Agents::new(Parts { kinds: vec![kind], ..Parts::default() });
    after.catalog.settings_arrived();
    after.reopen(stopped).await;
    let resumed = after.sessions.get(&SessionId::from("held-1")).expect("resumed under the agent's id");
    let mut live = resumed.live.clone();
    until(&mut live, |live| live.status() == Status::Idle).await;
    assert_eq!(watched::read(&live, |live| live.info.started_at), 100);
    assert_eq!(watched::read(&live, |live| live.info.last_activity), 200);
    let read = std::fs::read_to_string(&log).expect("the adapter's log");
    assert!(read.contains("session/resume"), "{read}");
    let shown: Vec<(EntryKind, String)> = watched::read(&live, |live| live.transcript(100))
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
async fn a_session_the_agent_replays_keeps_a_timestamp_snapshot() {
    let dir = tempfile::tempdir().expect("a temporary folder");
    let log = dir.path().join("log");
    let kind = adapter(&log);
    let before = Agents::new(Parts { kinds: vec![kind.clone()], ..Parts::default() });
    let session = before
        .start(&kind, Some(&dir.path().to_string_lossy()), begin("hello"))
        .await.expect("the session starts");
    let mut live = session.live.clone();
    until(&mut live, |live| live.info.turns == 1).await;
    before.shutdown().await;
    let stopped = before.reopen.open_sessions();
    assert_eq!(stopped.len(), 1);
    assert!(!stopped[0].transcript.is_empty(), "{stopped:?}");
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
    let before = Arc::new(Agents::new(Parts { kinds: vec![kind.clone()], ..Parts::default() }));
    let keeping = {
        let (dir, agents) = (dir.path().to_path_buf(), before.clone());
        tokio_util::task::AbortOnDropHandle::new(tokio::spawn(async move {
            restart::keep(&dir, &agents).await;
        }))
    };
    let session = before
        .start(&kind, Some(&dir.path().to_string_lossy()), begin("hello"))
        .await.expect("the session starts");
    let mut live = session.live.clone();
    until(&mut live, |live| live.info.turns == 1).await;
    let name = sessions::name_of(&session);
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

    let after = Agents::new(Parts { kinds: vec![kind], ..Parts::default() });
    after.catalog.settings_arrived();
    after.reopen(vec![noted]).await;
    let resumed = after.sessions.get(&name).expect("resumed under the agent's id");
    assert_eq!(after.remote_kids(&sessions::name_of(&resumed)), vec![kid]);
    before.shutdown().await;
    after.shutdown().await;
}

#[tokio::test]
async fn a_session_comes_back_after_a_crash() {
    let dir = tempfile::tempdir().expect("a temporary folder");
    let log = dir.path().join("log");
    let kind = adapter(&log);
    let before = Arc::new(Agents::new(Parts { kinds: vec![kind.clone()], ..Parts::default() }));
    let keeping = {
        let (dir, agents) = (dir.path().to_path_buf(), before.clone());
        tokio_util::task::AbortOnDropHandle::new(tokio::spawn(async move {
            restart::keep(&dir, &agents).await;
        }))
    };
    let session = before
        .start(&kind, Some(&dir.path().to_string_lossy()), begin("hello"))
        .await.expect("the session starts");
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

    let after = Agents::new(Parts { kinds: vec![kind], ..Parts::default() });
    after.catalog.settings_arrived();
    after.reopen(noted.sessions).await;
    let resumed = after.sessions.get(&SessionId::from("held-1")).expect("resumed under the agent's id");
    let mut live = resumed.live.clone();
    until(&mut live, |live| live.status() == Status::Idle).await;
    let read = std::fs::read_to_string(&log).expect("the adapter's log");
    assert!(read.contains("session/load"), "{read}");
    before.shutdown().await;
    after.shutdown().await;
}

#[tokio::test]
async fn a_resumed_session_goes_back_to_the_model_it_ran_on() {
    const MODELS: &str = r#""models":{"availableModels":[{"modelId":"fast"},{"modelId":"deep"}],"currentModelId":"fast"}"#;
    let dir = tempfile::tempdir().expect("a temporary folder");
    let log = dir.path().join("log");
    let mut kind = adapter(&log);
    kind.args[1] = ADAPTER
        .replace(
            r#"reply '{"sessionId":"held-1"}'"#,
            &format!(r#"reply '{{"sessionId":"held-1",{MODELS}}}'"#),
        )
        .replace(
            r#"*'"session/load"'*) reply '{}'"#,
            &format!(r#"*'"session/load"'*) reply '{{{MODELS}}}'"#),
        );
    let before = Agents::new(Parts { kinds: vec![kind.clone()], ..Parts::default() });
    let session = before
        .start(
            &kind,
            Some(&dir.path().to_string_lossy()),
            Begin {
                model: Some(ModelId::new("deep")),
                worktrees: vec!["/srv/app/.worktrees/fix".into()],
                ..begin("hello")
            },
        )
        .await.expect("the session starts");
    let mut live = session.live.clone();
    until(&mut live, |live| live.info.turns == 1).await;
    before.shutdown().await;
    let stopped = before.reopen.open_sessions();
    assert_eq!(stopped[0].model, Some(ModelId::new("deep")));
    // Kept for a take-over after it ended, across the restart note.
    let noted = Agents::new(Parts::default());
    noted.reopen.keep_saved(before.reopen.saved_sessions());
    let saved = noted.reopen.saved(&SessionId::from("held-1")).expect("saved across the note");
    assert_eq!(saved.info.model, Some(ModelId::new("deep")));
    assert_eq!(
        noted.reopen.created_worktrees(&SessionId::from("held-1")),
        vec!["/srv/app/.worktrees/fix".to_owned()]
    );

    let after = Agents::new(Parts { kinds: vec![kind], ..Parts::default() });
    after.catalog.settings_arrived();
    after.reopen(stopped).await;
    let session = after.sessions.get(&SessionId::from("held-1")).expect("resumed under the agent's id");
    let mut live = session.live.clone();
    until(&mut live, |live| {
        live.info.model == Some(ModelId::new("deep"))
    })
    .await;
    assert_eq!(
        session.info().worktrees,
        vec!["/srv/app/.worktrees/fix".to_owned()]
    );
    let read = std::fs::read_to_string(&log).expect("the adapter's log");
    let loaded = read.find("session/load").expect("the session loads");
    assert!(read[loaded..].contains("session/set_model"), "{read}");
    after.shutdown().await;
}

#[tokio::test]
async fn recovery_context_keeps_the_title_even_when_the_adapter_renames_it() {
    let dir = tempfile::tempdir().expect("temporary folder");
    let mut kind = adapter(&dir.path().join("log"));
    kind.args[1] = ADAPTER.replace(
        r#"*'"session/prompt"'*) reply '{"stopReason":"end_turn"}'"#,
        r#"*'"session/prompt"'*)
          echo '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"held-1","update":{"sessionUpdate":"session_info_update","title":"Continue after node restart"}}}'
          reply '{"stopReason":"end_turn"}'"#,
    );
    let before = Agents::new(Parts { kinds: vec![kind.clone()], ..Parts::default() });
    let session = before
        .start(&kind, Some(&dir.path().to_string_lossy()), begin("first"))
        .await.expect("starts");
    until(&mut session.live.clone(), |live| {
        live.status() == Status::Asking
    })
    .await;
    before.shutdown().await;
    let stopped = before.reopen.open_sessions();
    let after = Agents::new(Parts { kinds: vec![kind], ..Parts::default() });
    after.catalog.settings_arrived();
    after.reopen(stopped).await;
    let resumed = after.sessions.get(&SessionId::from("held-1")).expect("resumed");
    let mut live = resumed.live.clone();
    until(&mut live, |live| live.info.turns == 1).await;
    watched::read(&live, |live| {
        assert_eq!(live.info.title.as_deref(), Some("first"));
        let entries = live.transcript(100).0;
        let context = entries
            .iter()
            .find(|entry| entry.text.contains(CONTINUE))
            .expect("recovery context");
        assert!(crate::tags::own_words(&context.text).is_empty());
    });
    after.shutdown().await;
}

#[tokio::test]
async fn begin_assistant_lists_as_assistant() {
    let dir = tempfile::tempdir().expect("a temporary folder");
    let kind = adapter(&dir.path().join("log"));
    let agents = Agents::new(Parts { kinds: vec![kind.clone()], ..Parts::default() });
    let session = agents
        .start(
            &kind,
            Some(&dir.path().to_string_lossy()),
            Begin {
                assistant: true,
                ..begin("hello")
            },
        )
        .await.expect("the session starts");
    let mut live = session.live.clone();
    until(&mut live, |live| live.info.turns == 1).await;

    let listed = agents.sessions.list();
    assert_eq!(listed.len(), 1);
    assert!(listed[0].assistant);
    assert!(agents.reviews.is_assistant(&listed[0].id));
    agents.shutdown().await;
}

#[tokio::test]
async fn assistant_survives_restart() {
    let dir = tempfile::tempdir().expect("a temporary folder");
    let log = dir.path().join("log");
    let kind = adapter(&log);
    let before = Agents::new(Parts { kinds: vec![kind.clone()], ..Parts::default() });
    let session = before
        .start(
            &kind,
            Some(&dir.path().to_string_lossy()),
            Begin {
                assistant: true,
                ..begin("hello")
            },
        )
        .await.expect("the session starts");
    let mut live = session.live.clone();
    until(&mut live, |live| live.info.turns == 1).await;
    before.shutdown().await;
    let stopped = before.reopen.open_sessions();
    assert!(stopped[0].assistant);

    let after = Agents::new(Parts { kinds: vec![kind], ..Parts::default() });
    after.catalog.settings_arrived();
    after.reopen(stopped).await;
    assert!(after.reviews.is_assistant(&SessionId::from("held-1")));
    after.shutdown().await;
}

#[tokio::test]
async fn a_fresh_start_in_the_home_folder_is_no_assistant() {
    let dir = tempfile::tempdir().expect("a temporary folder");
    let kind = adapter(&dir.path().join("log"));
    let agents = Agents::new(Parts { kinds: vec![kind.clone()], ..Parts::default() });
    // No folder is the home folder; only the flag makes the assistant.
    let home = agents
        .start(&kind, None, begin("hello"))
        .await.expect("the session starts");

    assert!(!agents.reviews.is_assistant(&home.id));
    agents.shutdown().await;
}

#[tokio::test]
async fn an_ended_assistant_stays_the_assistant_after_a_restart() {
    let dir = tempfile::tempdir().expect("a temporary folder");
    let kind = adapter(&dir.path().join("log"));
    // Not the home folder: only the flag makes it the assistant, as on Windows.
    let before = Agents::new(Parts { kinds: vec![kind.clone()], ..Parts::default() });
    let session = before
        .start(
            &kind,
            Some(&dir.path().to_string_lossy()),
            Begin {
                assistant: true,
                ..begin("hello")
            },
        )
        .await.expect("the session starts");
    let mut live = session.live.clone();
    until(&mut live, |live| live.info.turns == 1).await;
    session.close().await;
    until(&mut live, |live| live.status() == Status::Ended).await;
    let title = session.info().title;
    let note = Reopen::new(before.reopen.open_sessions(), Vec::new(), before.reopen.saved_sessions());
    assert!(note.sessions.is_empty(), "closed, so not reopened");
    note.save(dir.path()).expect("the note writes");
    before.shutdown().await;

    let after = Agents::new(Parts { kinds: vec![kind.clone()], ..Parts::default() });
    after.reopen.keep_saved(Reopen::read(dir.path()).saved);
    after.catalog.settings_arrived();
    let resumed = after
        .resume(&kind, &SessionId::from("held-1"), None, None)
        .await
        .expect("taken over");
    assert!(after.reviews.is_assistant(&resumed.id));
    assert_eq!(resumed.info().title, title);
    after.shutdown().await;
}

#[cfg(unix)]
#[tokio::test]
async fn a_saved_session_in_the_home_folder_keeps_its_saved_flag() {
    let dir = tempfile::tempdir().expect("a temporary folder");
    let kind = adapter(&dir.path().join("log"));
    let home = dirs::home_dir().expect("a home folder");
    let agents = Agents::new(Parts { kinds: vec![kind.clone()], ..Parts::default() });
    agents.reopen.keep_saved(vec![SavedSession {
        info: SessionInfo {
            id: "held-1".into(),
            agent: "fake".into(),
            cwd: home.display().to_string(),
            ..SessionInfo::default()
        },
        effort: None,
        fast: None,
        account: None,
        named: false,
    }]);
    agents.catalog.settings_arrived();
    let resumed = agents
        .resume(&kind, &SessionId::from("held-1"), None, None)
        .await
        .expect("taken over");
    assert!(!agents.reviews.is_assistant(&resumed.id));
    agents.shutdown().await;
}

#[test]
fn an_ended_assistant_kept_from_before_is_still_the_assistant() {
    let agents = Agents::new(Parts::default());
    let kept = |id: &str, assistant: bool, reviewer: bool| SavedSession {
        info: SessionInfo {
            id: id.into(),
            agent: "flsh".into(),
            assistant,
            reviewer,
            ..SessionInfo::default()
        },
        effort: None,
        fast: None,
        account: None,
        named: false,
    };
    let mut thread = kept("thread", false, false);
    thread.info.title = Some("Prices".into());
    thread.named = true;
    let mut task = kept("task", false, false);
    task.info.title = Some("An older name".into());
    agents.reopen.keep_saved(vec![
        kept("chat", true, false),
        task,
        kept("review", true, true),
        thread,
    ]);

    // What the agents' files say of them, and of one never run here.
    let mut found: Vec<ExternalSession> = ["chat", "task", "review", "thread", "outside"]
        .into_iter()
        .map(|id| ExternalSession {
            id: id.into(),
            title: Some("Its agent's name".into()),
            assistant: true,
            ..ExternalSession::default()
        })
        .collect();
    agents.listings.mark_kept(&mut found);
    let assistants: Vec<&str> = found
        .iter()
        .filter(|session| session.assistant)
        .map(|session| session.id.as_ref())
        .collect();
    assert_eq!(assistants, ["chat"]);
    // Only a name its parent gave stands in for its agent's.
    let titles: Vec<_> = found.iter().map(|session| session.title.as_deref()).collect();
    let agents_name = Some("Its agent's name");
    assert_eq!(titles, [agents_name, agents_name, agents_name, Some("Prices"), agents_name]);
}

#[test]
fn a_note_from_before_saved_sessions_reads() {
    let dir = tempfile::tempdir().expect("a temporary folder");
    // `s1` was open; `s2` ended, so nothing but its model is known.
    let old = r#"{"sessions":[{"agent":"claude","session":"s1","cwd":"/srv/project","held":[]}],
        "terminals":[],"models":[{"session":"s1","model":"deep"},{"session":"s2","model":"fast"}],
        "worktrees":[{"session":"s1","worktrees":["/srv/project/.worktrees/fix"]},
                     {"session":"s2","worktrees":["/srv/other/.worktrees/x"]}]}"#;
    std::fs::write(dir.path().join("reopen.json"), old).expect("an old note");

    let note = Reopen::read(dir.path());
    assert_eq!(note.sessions.len(), 1);
    assert!(!note.sessions[0].assistant);
    // The open session's model and worktrees are kept with what it is.
    assert_eq!(note.saved.len(), 1);
    let saved = &note.saved[0].info;
    assert_eq!(
        (saved.id.as_ref(), saved.agent.as_str(), saved.cwd.as_str()),
        ("s1", "claude", "/srv/project")
    );
    assert_eq!(saved.model, Some(ModelId::new("deep")));
    assert_eq!(
        saved.worktrees,
        vec!["/srv/project/.worktrees/fix".to_owned()]
    );
    assert!(!saved.assistant);
}

#[test]
fn a_new_note_reads_as_an_old_node_keeps_it() {
    /// `Reopen` as beep-node 0.7.12 reads it.
    #[derive(serde::Deserialize)]
    struct OldReopen {
        sessions: Vec<serde_json::Value>,
        terminals: Vec<serde_json::Value>,
        #[serde(default)]
        models: Vec<OldModel>,
        #[serde(default)]
        worktrees: Vec<OldWorktrees>,
    }
    #[derive(serde::Deserialize)]
    struct OldModel {
        session: String,
        model: ModelId,
    }
    #[derive(serde::Deserialize)]
    struct OldWorktrees {
        session: String,
        worktrees: Vec<String>,
    }

    let dir = tempfile::tempdir().expect("a temporary folder");
    let saved = SavedSession {
        info: SessionInfo {
            id: "s1".into(),
            model: Some(ModelId::new("deep")),
            worktrees: vec!["/srv/project/.worktrees/fix".into()],
            ..SessionInfo::default()
        },
        effort: None,
        fast: None,
        account: None,
        named: false,
    };
    Reopen::new(Vec::new(), Vec::new(), vec![saved])
        .save(dir.path())
        .expect("the note writes");

    let text = std::fs::read_to_string(dir.path().join("reopen.json")).expect("the note");
    let old: OldReopen = serde_json::from_str(&text).expect("an old node reads it");
    assert!(old.sessions.is_empty() && old.terminals.is_empty());
    assert_eq!(old.models.len(), 1);
    assert_eq!(
        (old.models[0].session.as_str(), &old.models[0].model),
        ("s1", &ModelId::new("deep"))
    );
    assert_eq!(old.worktrees.len(), 1);
    assert_eq!(old.worktrees[0].session, "s1");
    assert_eq!(
        old.worktrees[0].worktrees,
        vec!["/srv/project/.worktrees/fix".to_owned()]
    );
}

#[tokio::test]
async fn a_saved_session_keeps_what_it_goes_on_with_and_not_its_state() {
    let dir = tempfile::tempdir().expect("a temporary folder");
    let kind = adapter(&dir.path().join("log"));
    let agents = Agents::new(Parts { kinds: vec![kind.clone()], ..Parts::default() });
    let session = agents
        .start(
            &kind,
            Some(&dir.path().to_string_lossy()),
            Begin {
                assistant: true,
                ..begin("hello")
            },
        )
        .await.expect("the session starts");
    let mut live = session.live.clone();
    until(&mut live, |live| live.info.turns == 1).await;

    let saved = agents.reopen.saved(&SessionId::from("held-1")).expect("saved");
    assert_eq!(saved.info.cwd, session.info().cwd);
    assert!(saved.info.assistant);
    assert_eq!(saved.info.turns, 0);
    assert!(saved.info.last_message.is_empty());
    assert_eq!(saved.info.state, Default::default());
    agents.shutdown().await;
}

#[tokio::test]
async fn a_refiled_session_is_saved_and_reopened_in_its_new_folder() {
    let dir = tempfile::tempdir().expect("a temporary folder");
    let kind = adapter(&dir.path().join("log"));
    let agents = Agents::new(Parts { kinds: vec![kind.clone()], ..Parts::default() });
    let session = agents
        .start(&kind, Some(&dir.path().to_string_lossy()), begin("hello"))
        .await
        .expect("the session starts");
    let mut live = session.live.clone();
    until(&mut live, |live| live.info.turns == 1).await;
    session.close().await;

    session.moved_to("/srv/refiled");

    // The one copy: a watcher that holds the state sees it change.
    assert!(live.has_changed().expect("the state is kept"));
    assert_eq!(session.info().cwd, "/srv/refiled");
    assert_eq!(agents.sessions.list()[0].cwd, "/srv/refiled");
    let saved = agents.reopen.saved(&SessionId::from("held-1")).expect("saved");
    assert_eq!(saved.info.cwd, "/srv/refiled");
    agents.shutdown().await;
}

#[test]
fn a_reviewer_taken_over_is_never_the_assistant() {
    let agents = Agents::new(Parts::default());
    let saved = SavedSession {
        info: SessionInfo {
            id: "held-1".into(),
            reviewer: true,
            assistant: true,
            ..SessionInfo::default()
        },
        effort: None,
        fast: None,
        account: None,
        named: false,
    };

    assert!(!agents.resumed_begin(&SessionId::from("held-1"), saved).assistant);
}

/// flsh sends a session's title again right after `session/load`. A session
/// saved before its title arrived keeps the first prompt until then.
#[tokio::test]
async fn a_reopened_session_takes_the_title_the_agent_sends_after_loading() {
    let dir = tempfile::tempdir().expect("temporary folder");
    let mut kind = adapter(&dir.path().join("log"));
    kind.args[1] = ADAPTER.replace(
        r#"*'"session/load"'*) reply '{}' ;;"#,
        r#"*'"session/load"'*) reply '{}'
          echo '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"held-1","update":{"sessionUpdate":"session_info_update","title":"Explain Rust lifetimes"}}}' ;;"#,
    );
    let before = Agents::new(Parts { kinds: vec![kind.clone()], ..Parts::default() });
    let session = before
        .start(&kind, Some(&dir.path().to_string_lossy()), begin("explain lifetimes"))
        .await.expect("starts");
    until(&mut session.live.clone(), |live| live.info.turns == 1).await;
    before.shutdown().await;
    let stopped = before.reopen.open_sessions();
    assert_eq!(stopped[0].title.as_deref(), Some("explain lifetimes"));

    let after = Agents::new(Parts { kinds: vec![kind], ..Parts::default() });
    after.catalog.settings_arrived();
    after.reopen(stopped).await;
    let resumed = after.sessions.get(&SessionId::from("held-1")).expect("resumed");
    until(&mut resumed.live.clone(), |live| {
        live.info.title.as_deref() == Some("Explain Rust lifetimes")
    })
    .await;
    after.shutdown().await;
}

/// flsh writes a title from the first prompt it gets, which carries Beep's
/// brief: a session that came back with a title keeps it.
#[tokio::test]
async fn a_reopened_session_keeps_its_title_when_the_agent_titles_a_prompt() {
    let dir = tempfile::tempdir().expect("temporary folder");
    let mut kind = adapter(&dir.path().join("log"));
    kind.args[1] = ADAPTER.replace(
        r#"    *'"session/prompt"'*) reply '{"stopReason":"end_turn"}' ;;"#,
        r#"    *'rename it'*)
      echo '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"held-1","update":{"sessionUpdate":"session_info_update","title":"Titled from the brief"}}}'
      reply '{"stopReason":"end_turn"}' ;;
    *'"session/prompt"'*) reply '{"stopReason":"end_turn"}' ;;"#,
    );
    let before = Agents::new(Parts { kinds: vec![kind.clone()], ..Parts::default() });
    let session = before
        .start(&kind, Some(&dir.path().to_string_lossy()), begin("explain lifetimes"))
        .await.expect("starts");
    until(&mut session.live.clone(), |live| live.info.turns == 1).await;
    before.shutdown().await;
    let stopped = before.reopen.open_sessions();

    let after = Agents::new(Parts { kinds: vec![kind], ..Parts::default() });
    after.catalog.settings_arrived();
    after.reopen(stopped).await;
    let resumed = after.sessions.get(&SessionId::from("held-1")).expect("resumed");
    resumed
        .order(Command::Prompt("rename it".to_owned().into()))
        .await
        .expect("the prompt is taken");
    let mut live = resumed.live.clone();
    until(&mut live, |live| live.info.turns >= 1 && live.status() == Status::Idle).await;
    assert_eq!(
        watched::read(&live, |live| live.info.title.clone()).as_deref(),
        Some("explain lifetimes")
    );
    after.shutdown().await;
}

#[tokio::test]
async fn a_saved_assistant_chat_still_names_its_folder_to_delete() {
    let dir = tempfile::tempdir().expect("a temporary folder");
    let agents = Agents::new(Parts { kinds: vec![adapter(&dir.path().join("log"))], ..Parts::default() });
    let kept = |id: &str, assistant: bool, parent: Option<ParentLink>| SavedSession {
        info: SessionInfo {
            id: id.into(),
            agent: "fake".into(),
            cwd: format!("/home/user/.beep/assistant/{id}"),
            assistant,
            parent,
            ..SessionInfo::default()
        },
        effort: None,
        fast: None,
        account: None,
        named: false,
    };
    let link = ParentLink {
        machine: None,
        session: "main-1".into(),
        key: "main-1".into(),
        root_machine: None,
        root: "main-1".into(),
        depth: 1,
        others: 0,
        agent: "fake".into(),
        mode: None,
        background: false,
    };
    agents.reopen.keep_saved(vec![
        kept("main-1", true, None),
        kept("thread-1", true, Some(link)),
        kept("plain-1", false, None),
    ]);

    // Only the main chat of an assistant conversation takes its folder with it.
    assert_eq!(
        agents.delete.conversation_of(&SessionId::from("main-1")),
        Some(std::path::PathBuf::from("/home/user/.beep/assistant/main-1"))
    );
    assert_eq!(agents.delete.conversation_of(&SessionId::from("thread-1")), None);
    assert_eq!(agents.delete.conversation_of(&SessionId::from("plain-1")), None);

    // A chat started in another's folder shares it: neither takes it while
    // the other is there, and the last of them does.
    let mut guest = kept("main-2", true, None);
    guest.info.cwd = "/home/user/.beep/assistant/main-1".into();
    agents.reopen.keep_saved(vec![kept("main-1", true, None), guest]);
    assert_eq!(agents.delete.conversation_of(&SessionId::from("main-1")), None);
    assert_eq!(agents.delete.conversation_of(&SessionId::from("main-2")), None);
    agents.reopen.forget_saved(&SessionId::from("main-2"));
    assert_eq!(
        agents.delete.conversation_of(&SessionId::from("main-1")),
        Some(std::path::PathBuf::from("/home/user/.beep/assistant/main-1"))
    );
    assert_eq!(agents.delete.conversation_of(&SessionId::from("gone-1")), None);
    agents.shutdown().await;
}

/// Built from the owners below it: every field is a value of a lower type.
#[test]
fn reopen_is_built_from_sessions_and_links_alone() {
    let crate::agents::reopen::Reopen {
        saved,
        reopened,
        sessions,
        links,
    } = crate::agents::reopen::Reopen::new(Arc::default(), Arc::default());
    let _: &std::sync::Mutex<Vec<SavedSession>> = &saved;
    let _: &tokio::sync::watch::Sender<bool> = &reopened;
    let _: &Arc<crate::agents::sessions::Sessions> = &sessions;
    let _: &Arc<crate::agents::family::links::Links> = &links;
}
