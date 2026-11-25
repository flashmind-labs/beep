//! The source's side of a move, with a shell script for the agent, and the
//! pieces the target uses.

use super::{
    Env, Stored,
    follow::{in_order, reconcile},
    progress::{Target, show},
    saved::{Folder, Phase, Saved, valid_id},
    source,
    store::{StoreRoots, write},
};
use crate::agents::{AgentKind, Agents, Begin, session::Command};
use beep_mesh::{Key, Mesh, transport_config};
use beep_proto::SessionId;
use beep_proto::watched;
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
        compact_models: Vec::new(),
        default_model: None,
        bridge: None,
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
        state: Folder::new(dir.path()),
        home: dir.path().join("home"),
        roots: StoreRoots {
            claude: dir.path().join("claude"),
            codex: dir.path().join("codex"),
            pi: dir.path().join("pi"),
            flsh: dir.path().join("flsh"),
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
        model: None,
        worktrees: Vec::new(),
        conversation: None,
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
        .await.expect("the session starts");
    let mut live = session.live.clone();
    timeout(
        Duration::from_secs(20),
        watched::wait_for(&mut live, |live| live.status() == Status::Asking),
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
        watched::wait_for(&mut live, |live| live.status() == Status::Idle),
    )
    .await
    .expect("the turn ends in time")
    .expect("the session is kept");
}

async fn ended(session: &crate::agents::Session) {
    let mut live = session.live.clone();
    timeout(
        Duration::from_secs(30),
        watched::wait_for(&mut live, |live| live.status() == Status::Ended),
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
    let saved = source
        .env
        .state
        .load(&SessionId::from("held-1"))
        .expect("it is kept");
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
    let question = watched::read(&session.live, |live| live.info.question.clone())
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
async fn a_thread_that_moves_outlives_a_parent_that_left_first() {
    let source = source_env().await;
    let session = asking(&source).await;
    session
        .order(Command::Moving { stop: false })
        .await
        .expect("held");
    session.order(Command::ParentGone).await.expect("heard");
    session
        .order(Command::NotMoving)
        .await
        .expect("it still takes orders");

    // One that does not move ends with its parent.
    session.order(Command::ParentGone).await.expect("heard");
    ended(&session).await;
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
    source::cancel(
        &source.env,
        &SessionId::from("held-1"),
        1_793_696_400_000,
        record(&source.env, true).to,
    )
    .await;
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
        futures::poll!(&mut moving).is_pending()
            && watched::read(&session.live, |live| live.status()) == Status::Idle,
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
    assert_ne!(
        watched::read(&session.live, |live| live.status()),
        Status::Ended
    );
    session
        .order(Command::Prompt("still here".to_owned().into()))
        .await
        .expect("the session takes prompts again");
    let saved = source
        .env
        .state
        .load(&SessionId::from("held-1"))
        .expect("it is kept");
    assert_eq!(saved.phase, Phase::Failed);
    assert_eq!(saved.reason.as_deref(), Some("this machine has no fake"));
    assert_eq!(
        source
            .env
            .agents
            .moves
            .refusal(&SessionId::from("held-1"))
            .as_deref(),
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
        source.env.agents.runs(&SessionId::from("held-1")),
        "the session runs again where it was"
    );
    assert_eq!(
        source
            .env
            .state
            .load(&SessionId::from("held-1"))
            .map(|saved| saved.phase),
        Some(Phase::Failed)
    );
    source.env.agents.shutdown().await;
}

#[tokio::test]
async fn a_move_cancelled_after_the_session_ended_does_not_start_it_while_the_target_is_away() {
    let source = source_env().await;
    let session = asking(&source).await;
    let record = record(&source.env, true);
    let (_said, mut target) = told(Target::Taken);
    {
        let moving = source::run(&source.env, &record, &mut target);
        tokio::pin!(moving);
        tokio::select! {
            done = &mut moving => panic!("the move ended early: {done:?}"),
            () = ended(&session) => {}
        }
    }
    // The target took it and cannot be asked since: it may run it.
    source::cancel(
        &source.env,
        &SessionId::from("held-1"),
        record.at,
        record.to,
    )
    .await;
    assert!(
        !source.env.agents.runs(&SessionId::from("held-1")),
        "the session does not run on two machines"
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
    let earlier = env
        .agents
        .moves
        .begin(&SessionId::from("held-1"), 1, env.mesh.me());
    earlier.finish(MovePhase::Done, None);
    let record = record(&env, false);
    env.mesh
        .set(
            Key::SessionMove(SessionId::from("held-1").to_owned()),
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
        watched::wait_for(&mut live, |live| live.status() == Status::Asking),
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
        .set(Key::SessionMove(SessionId::from("held-1").to_owned()), None)
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

#[tokio::test]
async fn a_replacement_move_releases_the_old_source_before_starting() {
    let source = source_env().await;
    let session = asking(&source).await;
    let env = Arc::new(source.env);
    let old = record(&env, false);
    let key = Key::SessionMove(old.id.clone());
    env.mesh
        .set(key.clone(), Some(old.to_bytes().unwrap()))
        .await
        .unwrap();
    let mut running = HashMap::new();
    reconcile(&env, &mut running).await;
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
    .expect("the old move held the session");

    // The new record makes this node the target. Nothing here can pick up
    // where the old source left off if its moving flag is never cleared.
    let replacement = SessionMove {
        at: old.at + 1,
        from: old.to,
        to: old.from,
        ..old
    };
    env.mesh
        .set(key, Some(replacement.to_bytes().unwrap()))
        .await
        .unwrap();
    reconcile(&env, &mut running).await;

    session
        .order(Command::Prompt("again".to_owned().into()))
        .await
        .expect("the replaced move released the session");
    env.agents.shutdown().await;
}

#[tokio::test]
async fn a_move_that_ends_has_the_records_looked_at_again() {
    let source = source_env().await;
    let env = Arc::new(source.env);
    // A move to this machine of a session of an agent that cannot move.
    let record = SessionMove {
        agent: AgentSlug::from("opencode"),
        from: SecretKey::generate().public(),
        to: env.mesh.me(),
        ..record(&env, false)
    };
    env.mesh
        .set(
            Key::SessionMove(SessionId::from("held-1")),
            Some(record.to_bytes().expect("a record encodes")),
        )
        .await
        .expect("the record is written");
    let ended = env.agents.moves.ended.notified();
    let mut running = HashMap::new();
    reconcile(&env, &mut running).await;

    timeout(Duration::from_secs(30), ended)
        .await
        .expect("a record that found no room is taken up when a move ends");
    env.agents.shutdown().await;
}

#[test]
fn the_oldest_move_is_taken_up_first_and_a_chat_before_its_threads() {
    let machine = SecretKey::generate().public();
    let one = |id: &str, path: &str, at| {
        let record = SessionMove {
            agent: AgentSlug::from("fake"),
            id: id.into(),
            from: machine,
            to: machine,
            project: String::new(),
            path: path.to_owned(),
            mode: None,
            model: None,
            worktrees: Vec::new(),
            conversation: None,
            stop: false,
            at,
            worktree: None,
        };
        (SessionId::from(id), record)
    };
    let records = HashMap::from([
        one("third", "ab12/c", 2),
        one("first", "ab12/a", 2),
        one("chat", "ab12", 2),
        one("second", "ab12/b", 2),
        one("older", "zz", 1),
    ]);

    let order: Vec<String> = in_order(records)
        .into_iter()
        .map(|(id, _)| id.as_ref().to_owned())
        .collect();

    assert_eq!(order, ["older", "chat", "first", "second", "third"]);
}

#[test]
fn a_target_that_resumes_the_session_reads_as_landed() {
    let moves = crate::agents::moving::Moves::default();
    let id = SessionId::from("held-1");
    let source = SecretKey::generate().public();
    let next_source = SecretKey::generate().public();
    let tracker = moves.begin(&id, 7, source);
    tracker.take();
    show(&tracker, |progress| progress.phase = MovePhase::Files);
    assert_eq!(moves.target(&id, 7), Target::Taken);
    assert!(tracker.resume());
    assert_eq!(moves.target(&id, 7), Target::Starting);
    assert!(!tracker.request_cancel());
    tracker.finish(MovePhase::Done, None);
    assert_eq!(moves.target(&id, 7), Target::Landed);

    let next = moves.begin(&id, 8, next_source);
    assert_eq!(moves.target(&id, 7), Target::Landed);
    assert!(moves.owns_target(&id, 7, source));
    assert!(!moves.owns_target(&id, 7, next_source));
    assert!(next.request_cancel());
    assert!(!next.resume());
    next.finish(MovePhase::Cancelled, None);
    assert!(matches!(moves.target(&id, 8), Target::Failed(_)));
}

/// A desktop that connects after a move failed still reads the failure from the push.
#[test]
fn a_failed_move_stays_in_the_push_after_the_session_moves_again() {
    let moves = crate::agents::moving::Moves::default();
    let id = SessionId::from("held-2");
    let source = SecretKey::generate().public();
    moves
        .begin(&id, 7, source)
        .finish(MovePhase::Failed, Some("no room".to_owned()));
    moves.begin(&id, 8, source);

    let failed = moves
        .entries()
        .into_iter()
        .find(|entry| entry.at == 7)
        .expect("the failed move is still pushed");

    assert_eq!(failed.progress.phase, MovePhase::Failed);
}

#[tokio::test]
async fn the_source_waits_for_a_starting_target_to_run_the_session() {
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
    said.send(Target::Starting).expect("the source listens");
    let early = timeout(Duration::from_millis(300), &mut moving).await;
    assert!(
        early.is_err(),
        "the source is done before the session runs there"
    );
    said.send(Target::Landed).expect("the source listens");
    timeout(Duration::from_secs(30), moving)
        .await
        .expect("it ends in time")
        .expect("the move is over");
    source.env.agents.shutdown().await;
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
    let kept = || -> Vec<PathBuf> {
        fs::read_dir(path.parent().unwrap())
            .unwrap()
            .flatten()
            .map(|f| f.path())
            .filter(|f| f.to_string_lossy().contains(".replaced-"))
            .collect()
    };
    assert!(kept().is_empty());
    // Another history under that name is not thrown away.
    write(&path, &stored("x\n")).expect("replaced");
    assert_eq!(fs::read_to_string(&path).expect("read"), "x\n");
    let kept = kept();
    assert_eq!(kept.len(), 1);
    assert_eq!(fs::read_to_string(&kept[0]).expect("kept"), "a\nb\n");
}

#[test]
fn a_move_that_ended_is_remembered_by_the_record_it_answered() {
    let dir = TempDir::new().expect("a temp folder");
    let folder = Folder::new(dir.path());
    assert_eq!(folder.load(&SessionId::from("s1")), None);
    let saved = Saved::over(7, Phase::Failed, Some("no checkout".into()));
    folder.save(&SessionId::from("s1"), &saved).expect("saved");
    assert_eq!(folder.load(&SessionId::from("s1")), Some(saved));
    folder.clear(&SessionId::from("s1"), 7);
    assert_eq!(folder.load(&SessionId::from("s1")), None);
    assert!(valid_id("0b1c2d3e-0000-4000-8000-0123456789ab"));
    assert!(!valid_id("../etc"));
    assert!(!valid_id(""));
}
