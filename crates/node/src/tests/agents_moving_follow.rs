use super::*;
use std::future::pending;

async fn ended() -> Running {
    let task = AbortOnDropHandle::new(tokio::spawn(async {}));

    while !task.is_finished() {
        tokio::task::yield_now().await;
    }
    Running {
        at: 1,
        side: Side::Target(Arc::default()),
        task,
    }
}

fn going() -> Running {
    Running {
        at: 1,
        side: Side::Source {
            to: iroh::SecretKey::generate().public(),
        },
        task: AbortOnDropHandle::new(tokio::spawn(pending::<()>())),
    }
}

#[tokio::test]
async fn moves_that_ended_leave_room_for_the_next() {
    let mut running = HashMap::new();

    for n in 0..MAX_MOVES + 4 {
        running.insert(SessionId::from(format!("done{n}")), ended().await);
    }

    assert!(
        has_room(&running),
        "records of finished moves stay in the mesh"
    );
    let mut busy = HashMap::new();

    for n in 0..MAX_MOVES {
        busy.insert(SessionId::from(format!("busy{n}")), going());
    }

    assert!(!has_room(&busy));
    busy.extend(running);
    assert!(!has_room(&busy), "only those that go count");
}

#[test]
fn old_background_copies_go_and_versions_that_lost_stay() {
    let dir = tempfile::TempDir::new().expect("a temp folder");
    let copied = dir.path().join("sessions/codex/s1");
    let lost = dir.path().join("sessions/claude/s2");

    for folder in [&copied, &lost] {
        fs::create_dir_all(folder).expect("a folder");
        fs::write(folder.join("copy.jsonl"), "{}\n").expect("a copy");
        fs::write(folder.join("meta.json"), "{}").expect("a note");
    }

    fs::write(lost.join("lost-9.jsonl"), "{}\n").expect("a lost version");
    fs::write(lost.join("lost-9.json"), "{}").expect("its note");
    drop_copies(dir.path());
    assert!(!dir.path().join("sessions/codex").exists());
    let mut left: Vec<_> = fs::read_dir(&lost)
        .expect("the lost version's folder stays")
        .flatten()
        .map(|file| file.file_name().to_string_lossy().into_owned())
        .collect();
    left.sort();
    assert_eq!(left, ["lost-9.json", "lost-9.jsonl"]);
}

#[tokio::test]
async fn one_sessions_worktree_placing_does_not_hold_up_another() {
    use crate::agents::moving::progress::Moves;

    let moves = Moves::default();
    let (a, b) = (SessionId::from("a"), SessionId::from("b"));
    let placing = moves.placing(&a);
    let _held = placing.lock().await;

    assert!(moves.placing(&b).try_lock().is_ok());
    assert!(moves.placing(&a).try_lock().is_err());
}

#[tokio::test]
async fn a_take_back_runs_beside_the_caller_and_the_next_one_waits_for_it() {
    use crate::agents::moving::progress::Moves;
    use tokio::sync::oneshot;

    let moves = Moves::default();
    let id = SessionId::from("a");
    let (release, held) = oneshot::channel::<()>();
    let (first_done, first) = oneshot::channel::<()>();
    moves.clean(&id, async move {
        let _ = held.await;
        let _ = first_done.send(());
    });
    let (second_done, mut second) = oneshot::channel::<()>();
    moves.clean(&id, async move {
        let _ = second_done.send(());
    });
    // The caller went on, and the second take-back waits for the first.
    tokio::task::yield_now().await;
    assert!(second.try_recv().is_err());

    release.send(()).expect("the first take-back waits");
    first.await.expect("the first take-back ended");
    moves.cleaned(&id).expect("the take-back is kept").await;
    assert!(second.try_recv().is_ok());
}

#[tokio::test]
async fn a_take_back_goes_on_when_the_next_move_stops_waiting() {
    use crate::agents::moving::progress::Moves;
    use tokio::sync::mpsc;

    let moves = Moves::default();
    let (open, gate) = tokio::sync::oneshot::channel::<()>();
    let (ended, mut ends) = mpsc::unbounded_channel();
    moves.clean(&SessionId::from("s1"), async move {
        let _ = gate.await;
        let _ = ended.send(());
    });

    // The next move takes the signal, then is cancelled.
    let waiting = moves
        .cleaned(&SessionId::from("s1"))
        .expect("a take-back is going");
    let next = tokio::spawn(waiting);
    next.abort();
    let _ = next.await;
    open.send(()).unwrap();

    assert_eq!(ends.recv().await, Some(()));
}

#[tokio::test]
async fn take_backs_of_one_session_run_one_at_a_time() {
    use crate::agents::moving::progress::Moves;
    use tokio::sync::mpsc;

    let moves = Moves::default();
    let (open, gate) = tokio::sync::oneshot::channel::<()>();
    let (said, mut order) = mpsc::unbounded_channel();
    let first = said.clone();
    moves.clean(&SessionId::from("s1"), async move {
        let _ = gate.await;
        let _ = first.send(1);
    });

    // A move took the signal, so the first is no longer waiting for anyone.
    let _ = moves.cleaned(&SessionId::from("s1"));
    moves.clean(&SessionId::from("s1"), async move {
        let _ = said.send(2);
    });
    open.send(()).unwrap();

    assert_eq!(order.recv().await, Some(1));
    assert_eq!(order.recv().await, Some(2));
}
