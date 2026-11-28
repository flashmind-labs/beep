use super::*;
use beep_proto::watched;

fn started(reply: Reply) -> TaskId {
    match reply {
        Reply::Started { id } => id,
        other => panic!("not started: {other:?}"),
    }
}

/// Ends the tasks of a test on every exit path, a failed assert included.
struct KillOnDrop(Tasks);

impl Drop for KillOnDrop {
    fn drop(&mut self) {
        self.0.kill_all();
    }
}

async fn wait_done(tasks: &Tasks, id: TaskId) -> Reply {
    let mut done = tasks.entry(id).expect("the task is kept").done.clone();
    timeout(
        Duration::from_secs(20),
        watched::wait_for(&mut done, |done| *done),
    )
    .await
    .expect("the task ends")
    .expect("the task reports its end");
    tasks.status(id)
}

#[tokio::test]
async fn a_task_keeps_its_last_lines_and_exit_code() {
    let tasks = Tasks::default();
    let command = if cfg!(windows) {
        "1..150; cmd /c \"(echo oops) 1>&2\"; exit 3"
    } else {
        "seq 1 150; echo oops >&2; exit 3"
    };
    let id = started(
        tasks
            .answer(Request::Start {
                command: command.into(),
                cwd: None,
            })
            .await,
    );
    let Reply::Status {
        task,
        lines,
        earlier,
    } = wait_done(&tasks, id).await
    else {
        panic!("no status");
    };
    assert_eq!(task.state, TaskState::Exited { code: Some(3) });
    assert!(earlier);
    assert_eq!(lines.len(), MAX_LINES);
    assert_eq!(lines.first().map(String::as_str), Some("52"));
    assert_eq!(
        lines.last().map(String::as_str),
        Some("oops"),
        "stderr is merged"
    );
}

// Reads /proc to see the child gone.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn stopping_a_task_stops_its_whole_group() {
    let tasks = Tasks::default();
    let dir = std::env::temp_dir().join(format!("beep-tasks-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a test folder");
    let id = started(
        tasks
            .answer(Request::Start {
                command: "sleep 300 & echo $! > child.pid; echo started; wait".into(),
                cwd: Some(dir.to_string_lossy().into_owned()),
            })
            .await,
    );
    let Reply::Tasks { tasks: listed } = tasks.answer(Request::List {}).await else {
        panic!("no list");
    };
    assert_eq!(listed.len(), 1);
    assert!(listed[0].state.is_running());
    // Waits for the child's pid to be written before stopping.
    let pid_file = dir.join("child.pid");
    let pid = timeout(Duration::from_secs(20), async {
        loop {
            if let Ok(text) = tokio::fs::read_to_string(&pid_file).await
                && let Ok(pid) = text.trim().parse::<libc::pid_t>()
            {
                return pid;
            }

            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("the pid is written");
    let Reply::Status { task, .. } = tasks.answer(Request::Stop { id }).await else {
        panic!("no status");
    };
    assert_eq!(task.state, TaskState::Stopped);
    // Gone, or a zombie its new parent has yet to reap. The kill is
    // delivered asynchronously, so watch /proc until it lands.
    let stat_file = format!("/proc/{pid}/stat");
    let stopped = timeout(Duration::from_secs(10), async {
        loop {
            let stat = tokio::fs::read_to_string(&stat_file)
                .await
                .unwrap_or_default();
            let running = stat
                .rsplit_once(") ")
                .is_some_and(|(_, rest)| !rest.starts_with('Z'));
            if !running {
                return;
            }

            tokio::task::yield_now().await;
        }
    })
    .await;
    let stat = std::fs::read_to_string(&stat_file).unwrap_or_default();
    assert!(
        stopped.is_ok(),
        "the background sleep was stopped with its group: {stat}"
    );
    std::fs::remove_dir_all(dir).expect("the test folder is removed");
}

#[cfg(unix)]
#[tokio::test]
async fn background_timeout_still_applies_after_stdout_closes() {
    let tasks = Tasks::default();
    let id = started(
        tasks
            .start_limited(
                "exec >/dev/null 2>&1; sleep 300".to_owned(),
                None,
                Some(Duration::from_millis(100)),
            )
            .await,
    );
    let Reply::Status { task, .. } = wait_done(&tasks, id).await else {
        panic!("no status");
    };
    assert_eq!(task.state, TaskState::Stopped);
}

// A spawn in flight is not in the list yet, so `kill_all` cannot reach it:
// the task is killed as it lands.
#[cfg(unix)]
#[tokio::test]
async fn a_task_spawned_after_kill_all_is_killed() {
    let tasks = Tasks::default();
    let (reached, reached_rx) = oneshot::channel();
    let (release_tx, release) = oneshot::channel();
    *lock(&tasks.gate) = Some(Gate { reached, release });
    let start = tasks.start_limited("sleep 300".to_owned(), None, None);
    tokio::pin!(start);
    let id = tokio::select! {
        _ = &mut start => panic!("the spawn was not held"),
        reached = reached_rx => {
            reached.expect("the spawn reports it is held");
            tasks.kill_all();
            release_tx.send(()).expect("the spawn waits");
            started(start.await)
        }
    };
    let Reply::Status { task, .. } = wait_done(&tasks, id).await else {
        panic!("no status");
    };
    assert!(!task.state.is_running());
}

// A start in flight holds a place: with it, the set is full and the next
// start is refused, though only MAX_TASKS - 1 tasks are listed.
#[cfg(unix)]
#[tokio::test]
async fn a_start_in_flight_holds_a_place() {
    let tasks = Tasks::default();
    let _kill = KillOnDrop(tasks.clone());

    for _ in 0..MAX_TASKS - 1 {
        started(
            tasks
                .start_limited("sleep 300".to_owned(), None, None)
                .await,
        );
    }

    let (reached, reached_rx) = oneshot::channel();
    let (release_tx, release) = oneshot::channel();
    *lock(&tasks.gate) = Some(Gate { reached, release });
    let last = tokio::spawn({
        let tasks = tasks.clone();
        async move {
            tasks
                .start_limited("sleep 300".to_owned(), None, None)
                .await
        }
    });
    reached_rx.await.expect("the spawn reports it is held");

    match tasks
        .start_limited("sleep 300".to_owned(), None, None)
        .await
    {
        Reply::Error { message } => assert!(message.contains("limit reached"), "{message}"),
        other => panic!("the full set took another start: {other:?}"),
    }

    release_tx.send(()).expect("the spawn waits");
    started(last.await.expect("the start ends"));
}

#[cfg(unix)]
#[tokio::test]
async fn a_full_set_makes_room_from_the_oldest_finished_task() {
    let tasks = Tasks::default();
    let _kill = KillOnDrop(tasks.clone());
    let first = started(tasks.start_limited("true".to_owned(), None, None).await);
    wait_done(&tasks, first).await;

    for _ in 0..MAX_TASKS - 1 {
        started(
            tasks
                .start_limited("sleep 300".to_owned(), None, None)
                .await,
        );
    }

    let next = started(
        tasks
            .start_limited("sleep 300".to_owned(), None, None)
            .await,
    );
    assert!(tasks.entry(first).is_none(), "the finished task made room");
    assert!(tasks.entry(next).is_some());
}

// Starts on several threads race for the last place: one gets it.
#[test]
fn two_starts_never_take_the_last_place_together() {
    for _ in 0..200 {
        let tasks = Tasks::default();
        tasks.starting.store(MAX_TASKS - 1, Ordering::SeqCst);
        let barrier = std::sync::Barrier::new(8);
        let places: Vec<_> = std::thread::scope(|scope| {
            let racers: Vec<_> = (0..8)
                .map(|_| {
                    scope.spawn(|| {
                        barrier.wait();
                        tasks.reserve()
                    })
                })
                .collect();
            racers
                .into_iter()
                .filter_map(|racer| racer.join().expect("a racer ends").ok())
                .collect()
        });

        assert_eq!(places.len(), 1, "the set took more than its places");
    }
}
