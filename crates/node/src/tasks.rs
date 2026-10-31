//! `process/1`: background commands. They belong to the node, not to the
//! stream that started them, so they outlive the turn; the node keeps at
//! most `MAX_TASKS` and the last `MAX_LINES` lines of each.

use crate::process::{GroupLeader, STOP_GRACE};
use anyhow::{Context, Result};
use beep_proto::node::{
    LinkRx, LinkTx,
    process::{MAX_LINES, MAX_TASKS, Reply, Request, Task, TaskId, TaskState},
};
use std::{
    collections::VecDeque,
    path::PathBuf,
    process::Stdio,
    sync::{Arc, LazyLock, Mutex, MutexGuard, PoisonError},
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::{ChildStdout, Command},
    sync::{oneshot, watch},
    time::timeout,
};
use tokio_util::task::AbortOnDropHandle;

pub use beep_proto::node::process::PROTOCOL;

/// The longest output line kept, in bytes.
const MAX_LINE_BYTES: usize = 1000;
/// How long a stop waits for the task to end, past the group's grace.
const STOP_WAIT: Duration = Duration::from_secs(5);

/// The node's tasks. One per process: a host embedding the node, such as
/// the Mac app, gets them too without wiring anything.
static TASKS: LazyLock<Tasks> = LazyLock::new(Tasks::default);

/// Serves one session stream until the server closes it.
pub async fn serve(mut tx: LinkTx, mut rx: LinkRx) -> Result<()> {
    while let Some(request) = rx.recv::<Request>().await? {
        let reply = TASKS.answer(request).await;
        tx.send(&reply).await?;
    }
    Ok(())
}

#[derive(Default)]
struct Tasks {
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    last: u32,
    jobs: Vec<Job>,
}

struct Job {
    id: TaskId,
    command: String,
    shared: Arc<Mutex<Shared>>,
    /// Taken by the first stop.
    stop: Option<oneshot::Sender<()>>,
    /// Turns true once the task ended.
    done: watch::Receiver<bool>,
    /// Dropping the job ends its task, and the group with it.
    _task: AbortOnDropHandle<()>,
}

struct Shared {
    state: TaskState,
    lines: VecDeque<String>,
    earlier: bool,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Tasks {
    async fn answer(&self, request: Request) -> Reply {
        match request {
            Request::Start { command, cwd } => self.start(command, cwd),
            Request::List {} => Reply::Tasks {
                tasks: lock(&self.inner).jobs.iter().map(Job::task).collect(),
            },
            Request::Status { id } => self.status(id),
            Request::Stop { id } => self.stop(id).await,
            other => Reply::Error {
                message: format!("this node does not handle {other:?}"),
            },
        }
    }

    fn start(&self, command: String, cwd: Option<String>) -> Reply {
        let mut inner = lock(&self.inner);
        if inner.jobs.len() >= MAX_TASKS {
            // Room comes from the oldest finished task.
            match inner.jobs.iter().position(|job| *job.done.borrow()) {
                Some(oldest) => {
                    inner.jobs.remove(oldest);
                }
                None => {
                    return Reply::Error {
                        message: format!(
                            "Background process limit reached ({MAX_TASKS}). Stop a process first."
                        ),
                    };
                }
            }
        }
        let cwd = cwd.map(PathBuf::from).or_else(dirs::home_dir);
        let (leader, stdout) = match spawn(&command, cwd) {
            Ok(started) => started,
            Err(e) => {
                return Reply::Error {
                    message: format!("{e:#}"),
                };
            }
        };
        inner.last = inner.last.wrapping_add(1);
        let id = TaskId(inner.last);
        let shared = Arc::new(Mutex::new(Shared {
            state: TaskState::Running,
            lines: VecDeque::new(),
            earlier: false,
        }));
        let (stop_tx, stop_rx) = oneshot::channel();
        let (done_tx, done_rx) = watch::channel(false);
        let task = tokio::spawn(follow(leader, stdout, shared.clone(), stop_rx, done_tx));
        inner.jobs.push(Job {
            id,
            command,
            shared,
            stop: Some(stop_tx),
            done: done_rx,
            _task: AbortOnDropHandle::new(task),
        });
        Reply::Started { id }
    }

    fn status(&self, id: TaskId) -> Reply {
        let inner = lock(&self.inner);
        match inner.jobs.iter().find(|job| job.id == id) {
            Some(job) => job.status(),
            None => unknown(id),
        }
    }

    async fn stop(&self, id: TaskId) -> Reply {
        let mut done = {
            let mut inner = lock(&self.inner);
            let Some(job) = inner.jobs.iter_mut().find(|job| job.id == id) else {
                return unknown(id);
            };
            if let Some(stop) = job.stop.take() {
                // A task that already ended dropped its receiver.
                let _ = stop.send(());
            }
            job.done.clone()
        };
        if timeout(STOP_GRACE + STOP_WAIT, done.wait_for(|done| *done))
            .await
            .is_err()
        {
            log::warn!("background task {id} did not stop in time");
        }
        self.status(id)
    }
}

impl Job {
    fn task(&self) -> Task {
        Task {
            id: self.id,
            command: self.command.clone(),
            state: lock(&self.shared).state,
        }
    }

    fn status(&self) -> Reply {
        let shared = lock(&self.shared);
        Reply::Status {
            task: Task {
                id: self.id,
                command: self.command.clone(),
                state: shared.state,
            },
            lines: shared.lines.iter().cloned().collect(),
            earlier: shared.earlier,
        }
    }
}

fn unknown(id: TaskId) -> Reply {
    Reply::Error {
        message: format!("No background process {id}."),
    }
}

/// Starts `command` in its own process group, stderr merged into stdout.
fn spawn(command: &str, cwd: Option<PathBuf>) -> Result<(GroupLeader, ChildStdout)> {
    let program = if which::which("bash").is_ok() {
        "bash"
    } else {
        "sh"
    };
    let mut child = Command::new(program);
    child
        .arg("-c")
        .arg(format!("exec 2>&1\n{command}"))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .process_group(0)
        .kill_on_drop(true);
    if let Some(cwd) = cwd {
        child.current_dir(cwd);
    }
    let mut child = child
        .spawn()
        .with_context(|| format!("starting {program}"))?;
    let stdout = child.stdout.take().context("the command has no stdout")?;
    Ok((GroupLeader::new(child), stdout))
}

/// Keeps the task's last lines and its end, until it ends or is stopped.
async fn follow(
    mut leader: GroupLeader,
    stdout: ChildStdout,
    shared: Arc<Mutex<Shared>>,
    mut stop: oneshot::Receiver<()>,
    done: watch::Sender<bool>,
) {
    let mut stdout = BufReader::new(stdout);
    let mut line = Vec::new();
    // A dropped sender counts as a stop too: the job is gone.
    let mut stopped = false;
    loop {
        line.clear();
        tokio::select! {
            _ = &mut stop => {
                stopped = true;
                break;
            }
            read = stdout.read_until(b'\n', &mut line) => match read {
                Ok(0) => break,
                Ok(_) => keep(&shared, &line),
                Err(e) => {
                    log::debug!("reading a background task failed: {e}");
                    break;
                }
            },
        }
    }
    let state = if stopped {
        leader.stop(STOP_GRACE).await;
        TaskState::Stopped
    } else {
        tokio::select! {
            _ = &mut stop => {
                leader.stop(STOP_GRACE).await;
                TaskState::Stopped
            }
            status = leader.child_mut().wait() => match status {
                Ok(status) => TaskState::Exited { code: status.code() },
                Err(e) => {
                    log::warn!("could not wait for a background task: {e}");
                    TaskState::Exited { code: None }
                }
            },
        }
    };
    lock(&shared).state = state;
    done.send_replace(true);
}

fn keep(shared: &Mutex<Shared>, line: &[u8]) {
    let text = String::from_utf8_lossy(line);
    let text = text.trim_end_matches(['\n', '\r']);
    let mut end = text.len().min(MAX_LINE_BYTES);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    let mut shared = lock(shared);
    if shared.lines.len() == MAX_LINES {
        shared.lines.pop_front();
        shared.earlier = true;
    }
    shared.lines.push_back(text[..end].to_owned());
}

#[cfg(test)]
mod tests {
    use super::*;

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
        timeout(Duration::from_secs(20), done.wait_for(|done| *done))
            .await
            .expect("the task ends")
            .expect("the task reports its end");
        tasks.status(id)
    }

    #[tokio::test]
    async fn a_task_keeps_its_last_lines_and_exit_code() {
        let tasks = Tasks::default();
        let id = started(
            tasks
                .answer(Request::Start {
                    command: "seq 1 150; echo oops >&2; exit 3".into(),
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
        // Gone, or a zombie its new parent has yet to reap.
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).unwrap_or_default();
        let running = stat
            .rsplit_once(") ")
            .is_some_and(|(_, rest)| !rest.starts_with('Z'));
        assert!(
            !running,
            "the background sleep was stopped with its group: {stat}"
        );
        std::fs::remove_dir_all(dir).expect("the test folder is removed");
    }

    #[tokio::test]
    async fn an_unknown_task_is_an_error() {
        let tasks = Tasks::default();
        assert!(matches!(
            tasks.answer(Request::Status { id: TaskId(9) }).await,
            Reply::Error { .. }
        ));
    }
}
