//! The source's side of a move, with a shell script for the agent, and the
//! pieces the target uses.

use super::{
    Env, Stored,
    follow::reconcile,
    progress::{Target, show},
    saved::{Folder, Phase, Saved, valid_id},
    source,
    store::{StoreRoots, write},
};
use crate::agents::{AgentKind, Agents, Begin, session::Command};
use beep_mesh::{Key, Mesh, transport_config};
use beep_proto::{
    AgentSlug,
    node::{
        agent::{MovePhase, Status},
        mesh_state::{SessionMove, StateValue},
    },
};
use iroh::{Endpoint, SecretKey, endpoint::presets};
use std::{
    collections::{BTreeMap, HashMap},
    fs,
    path::PathBuf,
    sync::Arc,
    time::Duration,
};
use tempfile::TempDir;
use tokio::sync::watch;
use tokio::time::timeout;

/// An ACP adapter in shell. It asks permission on the prompt "first" and
/// ends that turn once answered or cancelled; other prompts end at once.
const ADAPTER: &str = r#"
id() { printf '%s\n' "$line" | sed -nE 's/.*"id":("[^"]*"|[0-9]+).*/\1/p'; }
reply() { printf '{"jsonrpc":"2.0","id":%s,"result":%s}\n' "$(id)" "$1"; }
read -r line
reply '{"protocolVersion":1,"agentCapabilities":{"loadSession":true}}'
while read -r line; do
  case "$line" in
    *'"session/new"'*) reply '{"sessionId":"held-1"}' ;;
    *'"session/load"'*) reply '{}' ;;
    *'"text":"first'*) prompt=$(id)
      echo '{"jsonrpc":"2.0","id":"p1","method":"session/request_permission","params":{"sessionId":"held-1","toolCall":{"toolCallId":"t1","title":"Run cargo test"},"options":[{"optionId":"allow","name":"Allow","kind":"allow_once"}]}}' ;;
    *'"p1"'*) printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$prompt" ;;
    *'"session/cancel"'*) printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"cancelled"}}\n' "$prompt" ;;
    *'"session/prompt"'*) reply '{"stopReason":"end_turn"}' ;;
    *'"method"'*'"id"'*|*'"id"'*'"method"'*) reply '{}' ;;
  esac
done
"#;

fn kind() -> AgentKind {
    AgentKind {
        name: "fake".into(),
        program: "sh".into(),
        args: vec!["-c".into(), ADAPTER.into(), "fake-adapter".into()],
        env: BTreeMap::new(),
        compact: None,
    }
}

struct Source {
    _dir: TempDir,
    env: Env,
    work: PathBuf,
}

async fn source_env() -> Source {
    let dir = TempDir::new().expect("a temp folder");
    let endpoint = Endpoint::builder(presets::Minimal)
        .secret_key(SecretKey::generate())
        .transport_config(transport_config())
        .bind()
        .await
        .expect("an endpoint binds");
    let mesh = Mesh::create(&dir.path().join("mesh"), endpoint.clone(), "laptop")
        .await
        .expect("the mesh is made");
    let work = dir.path().join("work");
    fs::create_dir_all(&work).expect("a work folder");
    let agents = Arc::new(Agents::new(vec![kind()]));
    agents.reopen(Vec::new()).await;
    let env = Env {
        agents,
        mesh,
        endpoint,
        syncs: None,
        state: Folder::new(dir.path()),
        roots: StoreRoots {
            claude: dir.path().join("claude"),
            codex: dir.path().join("codex"),
            pi: dir.path().join("pi"),
        },
    };
    Source {
        _dir: dir,
        env,
        work,
    }
}

fn record(env: &Env, stop: bool) -> SessionMove {
    SessionMove {
        agent: AgentSlug::from("fake"),
        id: "held-1".into(),
        from: env.mesh.me(),
        to: SecretKey::generate().public(),
        project: "org/app".into(),
        path: String::new(),
        mode: None,
        stop,
        at: 1_793_696_400_000,
        worktree: None,
    }
}

async fn asking(source: &Source) -> Arc<crate::agents::Session> {
    let session = source
        .env
        .agents
        .start(
            &kind(),
            Some(&source.work.to_string_lossy()),
            Begin {
                prompt: Some("first".into()),
                ..Begin::default()
            },
        )
        .expect("the session starts");
    let mut live = session.live.clone();
    timeout(
        Duration::from_secs(20),
        live.wait_for(|live| live.status() == Status::Asking),
    )
    .await
    .expect("the turn asks in time")
    .expect("the session is kept");
    session
}

/// A source that hears of its target through `said`, which a test fills.
fn told(first: Target) -> (watch::Sender<Target>, watch::Receiver<Target>) {
    watch::channel(first)
}

async fn idle(session: &crate::agents::Session) {
    let mut live = session.live.clone();
    timeout(
        Duration::from_secs(30),
        live.wait_for(|live| live.status() == Status::Idle),
    )
    .await
    .expect("the turn ends in time")
    .expect("the session is kept");
}

async fn ended(session: &crate::agents::Session) {
    let mut live = session.live.clone();
    timeout(
        Duration::from_secs(30),
        live.wait_for(|live| live.status() == Status::Ended),
    )
    .await
    .expect("the session ends in time")
    .expect("the session is kept");
}

#[tokio::test]
async fn the_source_stops_the_turn_when_told_to_and_ends_the_session() {
    let source = source_env().await;
    let session = asking(&source).await;
    let record = record(&source.env, true);
    let (said, mut target) = told(Target::Taken);
    let moving = source::run(&source.env, &record, &mut target);
    tokio::pin!(moving);
    tokio::select! { done = &mut moving => panic!("the move ended early: {done:?}"), () = ended(&session) => {} }
    said.send(Target::Landed).expect("the source listens");
    timeout(Duration::from_secs(30), moving)
        .await
        .expect("it ends in time")
        .expect("it ends the session");
    let saved = source.env.state.load("held-1").expect("it is kept");
    assert_eq!((saved.at, saved.phase), (record.at, Phase::Done));
    source.env.agents.shutdown().await;
}

#[tokio::test]
async fn the_source_waits_for_the_turn_and_runs_what_was_queued_before_the_move() {
    let source = source_env().await;
    let session = asking(&source).await;
    session
        .order(Command::Prompt("after".to_owned().into()))
        .await
        .expect("the prompt queues");
    let record = record(&source.env, false);
    let env = &source.env;
    let (said, mut target) = told(Target::Taken);
    let waiting = source::run(env, &record, &mut target);
    tokio::pin!(waiting);
    assert!(
        futures::poll!(&mut waiting).is_pending(),
        "a turn waiting for an answer holds the move"
    );
    let question = session
        .live
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
    tokio::select! { done = &mut waiting => panic!("the move ended early: {done:?}"), () = ended(&session) => {} }
    said.send(Target::Landed).expect("the source listens");
    timeout(Duration::from_secs(30), waiting)
        .await
        .expect("it ends in time")
        .expect("it ends the session");
    let info = session.info();
    assert_eq!(
        info.turns, 2,
        "what was queued before the move was not lost"
    );
    source.env.agents.shutdown().await;
}

#[tokio::test]
async fn a_session_that_moves_takes_no_new_prompt() {
    let source = source_env().await;
    let session = asking(&source).await;
    session
        .order(Command::Moving { stop: false })
        .await
        .expect("held");
    for command in [
        Command::Prompt("later".to_owned().into()),
        Command::Steer("later".to_owned().into()),
    ] {
        let refused = session.order(command).await.expect_err("refused");
        assert!(refused.contains("moving to another machine"), "{refused}");
    }
    source.env.agents.shutdown().await;
}

#[tokio::test]
async fn a_cancelled_move_lets_prompts_in_again() {
    let source = source_env().await;
    let session = asking(&source).await;
    session
        .order(Command::Moving { stop: false })
        .await
        .expect("held");
    source::cancel(&source.env, "held-1").await;
    session
        .order(Command::Prompt("after".to_owned().into()))
        .await
        .expect("a prompt is taken again");
    source.env.agents.shutdown().await;
}

#[tokio::test]
async fn the_source_keeps_the_session_until_the_target_can_take_it() {
    let source = source_env().await;
    let session = asking(&source).await;
    let record = record(&source.env, true);
    let (said, mut target) = told(Target::Unknown);
    let moving = source::run(&source.env, &record, &mut target);
    tokio::pin!(moving);
    // The turn is over, and the target has said nothing.
    tokio::select! {
        done = &mut moving => panic!("the move ended without the target: {done:?}"),
        () = idle(&session) => {}
    }
    assert!(
        futures::poll!(&mut moving).is_pending() && session.live.borrow().status() == Status::Idle,
        "a target that has not answered costs the session nothing"
    );
    said.send(Target::Taken).expect("the source listens");
    tokio::select! { done = &mut moving => panic!("the move ended early: {done:?}"), () = ended(&session) => {} }
    said.send(Target::Landed).expect("the source listens");
    timeout(Duration::from_secs(30), moving)
        .await
        .expect("it ends in time")
        .expect("the move is over");
    source.env.agents.shutdown().await;
}

#[tokio::test]
async fn a_target_that_gives_up_leaves_the_session_running_here() {
    let source = source_env().await;
    let session = asking(&source).await;
    let record = record(&source.env, true);
    let (said, mut target) = told(Target::Unknown);
    let moving = source::run(&source.env, &record, &mut target);
    tokio::pin!(moving);
    tokio::select! {
        done = &mut moving => panic!("the move ended without the target: {done:?}"),
        () = idle(&session) => {}
    }
    said.send(Target::Failed("this machine has no fake".into()))
        .expect("the source listens");
    let ended = timeout(Duration::from_secs(30), moving)
        .await
        .expect("it ends in time");
    assert!(ended.is_err(), "the move did not happen");
    assert_ne!(session.live.borrow().status(), Status::Ended);
    session
        .order(Command::Prompt("still here".to_owned().into()))
        .await
        .expect("the session takes prompts again");
    let saved = source.env.state.load("held-1").expect("it is kept");
    assert_eq!(saved.phase, Phase::Failed);
    assert_eq!(saved.reason.as_deref(), Some("this machine has no fake"));
    assert_eq!(
        source.env.agents.moves.refusal("held-1").as_deref(),
        Some("this machine has no fake"),
        "a target that asks hears why"
    );
    source.env.agents.shutdown().await;
}

#[tokio::test]
async fn a_target_that_gives_up_after_the_session_ended_has_it_start_again_here() {
    let source = source_env().await;
    let session = asking(&source).await;
    let record = record(&source.env, true);
    let (said, mut target) = told(Target::Taken);
    let moving = source::run(&source.env, &record, &mut target);
    tokio::pin!(moving);
    tokio::select! {
        done = &mut moving => panic!("the move ended early: {done:?}"),
        () = ended(&session) => {}
    }
    said.send(Target::Failed("the files did not come".into()))
        .expect("the source listens");
    let done = timeout(Duration::from_secs(30), moving)
        .await
        .expect("it ends in time");
    assert!(done.is_err(), "the move did not happen");
    assert!(
        source.env.agents.runs("held-1"),
        "the session runs again where it was"
    );
    assert_eq!(
        source.env.state.load("held-1").map(|saved| saved.phase),
        Some(Phase::Failed)
    );
    source.env.agents.shutdown().await;
}

#[tokio::test]
async fn a_move_taken_back_after_an_earlier_one_ended_here_still_frees_the_session() {
    // The session came here as a target once (its tracker says done), and
    // now moves away again.
    let source = source_env().await;
    let session = asking(&source).await;
    let env = Arc::new(source.env);
    let earlier = env.agents.moves.begin("held-1", 1);
    show(&earlier, |progress| progress.phase = MovePhase::Done);
    let record = record(&env, false);
    env.mesh
        .set(
            Key::SessionMove("held-1".to_owned()),
            Some(record.to_bytes().expect("a record encodes")),
        )
        .await
        .expect("the record is written");
    let mut running = HashMap::new();
    reconcile(&env, &mut running).await;
    assert_eq!(running.len(), 1, "this machine is the source");
    // Moving, and told so: a prompt waits no longer.
    let mut live = session.live.clone();
    timeout(
        Duration::from_secs(30),
        live.wait_for(|live| live.status() == Status::Asking),
    )
    .await
    .expect("asking")
    .expect("kept");
    timeout(Duration::from_secs(30), async {
        while session
            .order(Command::Prompt("x".to_owned().into()))
            .await
            .is_ok()
        {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("the source ordered the move");
    // The move is taken back.
    env.mesh
        .set(Key::SessionMove("held-1".to_owned()), None)
        .await
        .expect("the record goes");
    reconcile(&env, &mut running).await;
    assert!(running.is_empty());
    session
        .order(Command::Prompt("again".to_owned().into()))
        .await
        .expect("the session takes prompts again: the old tracker did not hide the cancel");
    env.agents.shutdown().await;
}

#[test]
fn a_transcript_is_written_private_and_a_different_one_is_kept_aside() {
    use std::os::unix::fs::PermissionsExt;
    let dir = TempDir::new().expect("a temp folder");
    let path = dir.path().join("projects/-work-app/s1.jsonl");
    let stored = |text: &str| Stored {
        bytes: text.as_bytes().to_vec(),
        path: PathBuf::from("s1.jsonl"),
    };
    write(&path, &stored("a\n")).expect("written");
    assert_eq!(
        fs::metadata(&path).expect("it exists").permissions().mode() & 0o777,
        0o600
    );
    // The same session moving back: the new one only adds.
    write(&path, &stored("a\nb\n")).expect("replaced");
    assert_eq!(fs::read_to_string(&path).expect("read"), "a\nb\n");
    assert!(!path.with_extension("jsonl.replaced").exists());
    // Another history under that name is not thrown away.
    write(&path, &stored("x\n")).expect("replaced");
    assert_eq!(fs::read_to_string(&path).expect("read"), "x\n");
    assert_eq!(
        fs::read_to_string(path.with_extension("jsonl.replaced")).expect("kept"),
        "a\nb\n"
    );
}

#[test]
fn a_move_that_ended_is_remembered_by_the_record_it_answered() {
    let dir = TempDir::new().expect("a temp folder");
    let folder = Folder::new(dir.path());
    assert_eq!(folder.load("s1"), None);
    let saved = Saved::over(7, Phase::Failed, Some("no checkout".into()));
    folder.save("s1", &saved).expect("saved");
    assert_eq!(folder.load("s1"), Some(saved));
    folder.clear("s1", true);
    assert_eq!(folder.load("s1"), None);
    assert!(valid_id("0b1c2d3e-0000-4000-8000-0123456789ab"));
    assert!(!valid_id("../etc"));
    assert!(!valid_id(""));
}
