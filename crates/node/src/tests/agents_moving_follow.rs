use super::*;
use std::future::pending;

async fn ended() -> Running {
    let task = AbortOnDropHandle::new(tokio::spawn(async {}));

    while !task.is_finished() {
        tokio::task::yield_now().await;
    }
    Running {
        at: 1,
        target: true,
        to: iroh::SecretKey::generate().public(),
        task,
    }
}

fn going() -> Running {
    Running {
        at: 1,
        target: false,
        to: iroh::SecretKey::generate().public(),
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
