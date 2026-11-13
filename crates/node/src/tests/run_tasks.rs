use super::*;
use beep_proto::watched;

fn started(reply: Reply) -> TaskId {
    match reply {
        Reply::Started { id } => id,
        other => panic!("not started: {other:?}"),
    }
}

async fn wait_done(tasks: &Tasks, id: TaskId) -> Reply {
    let mut done = {
        let inner = lock(&tasks.inner);
        inner
            .jobs
            .iter()
            .find(|job| job.id == id)
            .expect("the task is kept")
            .done
            .clone()
    };
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
