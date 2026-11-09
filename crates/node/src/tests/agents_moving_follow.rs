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
        task,
    }
}

fn going() -> Running {
    Running {
        at: 1,
        target: false,
        task: AbortOnDropHandle::new(tokio::spawn(pending::<()>())),
    }
}

#[tokio::test]
async fn moves_that_ended_leave_room_for_the_next() {
    let mut running = HashMap::new();

    for n in 0..MAX_MOVES + 4 {
        running.insert(format!("done{n}"), ended().await);
    }

    assert!(
        has_room(&running),
        "records of finished moves stay in the mesh"
    );
    let mut busy = HashMap::new();

    for n in 0..MAX_MOVES {
        busy.insert(format!("busy{n}"), going());
    }

    assert!(!has_room(&busy));
    busy.extend(running);
    assert!(!has_room(&busy), "only those that go count");
}
