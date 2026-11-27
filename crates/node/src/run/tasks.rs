//! `process/1`: background commands. They belong to the node, not to the
//! stream that started them, so they outlive the turn; the node keeps at
//! most `MAX_TASKS` and the last `MAX_LINES` lines of each.

use crate::{actor, lock};
use crate::{
    process::spawn_env::{self, Scope},
    process::{Group, GroupLeader, STOP_GRACE, own_group},
    run::lines::read_piece,
    run::shell,
};
use anyhow::{Context, Result};
use beep_proto::node::{
    LinkRx, LinkTx,
    process::{MAX_LINES, MAX_TASKS, Reply, Request, Task, TaskId, TaskState},
};
use beep_proto::watched;
use iroh::EndpointId;
use std::{
    collections::{HashMap, VecDeque},
    path::PathBuf,
    process::Stdio,
    sync::{
        Arc, LazyLock, Mutex,
        atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};
#[cfg(test)]
use tokio::sync::oneshot;
use tokio::{
    io::BufReader,
    process::ChildStdout,
    sync::watch,
    time::{Instant, sleep_until, timeout},
};
use tokio_util::{sync::CancellationToken, task::AbortOnDropHandle};

pub use beep_proto::node::process::PROTOCOL;

/// The longest output line kept, in bytes.
const MAX_LINE_BYTES: usize = 1000;
/// How long a stop waits for the task to end, past the group's grace.
const STOP_WAIT: Duration = Duration::from_secs(5);

/// The node's tasks. One per process: a host embedding the node, such as
/// the Mac app, gets them too without wiring anything.
static TASKS: LazyLock<Arc<Tasks>> = LazyLock::new(Arc::default);

/// The tasks each node peer started over `tools/1`, apart from `TASKS` and
/// from each other, so a peer can neither fill the desktop's slots nor reach
/// its jobs or another peer's.
static PEER_TASKS: LazyLock<Mutex<HashMap<EndpointId, Arc<Tasks>>>> = LazyLock::new(Mutex::default);

/// Whose tasks a `tools/1` call works on.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Table {
    /// The node's own, shared with `process/1`.
    Shared,
    /// The ones this peer started.
    Peer(EndpointId),
}

impl Table {
    fn tasks(self) -> Arc<Tasks> {
        match self {
            Self::Shared => TASKS.clone(),
            Self::Peer(id) => lock(&PEER_TASKS).entry(id).or_default().clone(),
        }
    }
}

/// Kills every task still running, at once. For a host about to exit, such
/// as the Mac app: a process exit drops nothing, so its tasks would outlive it.
pub fn kill_all() {
    TASKS.kill_all();

    for tasks in lock(&PEER_TASKS).values() {
        tasks.kill_all();
    }
}

/// Serves one session stream until the caller closes it.
pub async fn serve(mut tx: LinkTx, mut rx: LinkRx) -> Result<()> {
    while let Some(request) = rx.recv::<Request>().await? {
        let reply = TASKS.answer(request).await;
        tx.send(&reply).await?;
    }

    Ok(())
}

/// Runs one request on the node's tasks, as `process/1` would.
pub(crate) async fn answer(table: Table, request: Request) -> Reply {
    table.tasks().answer(request).await
}

/// Starts a background task with a deadline owned by its process watcher.
/// No process/1 request fields change: MCP timeouts are local task policy.
pub(crate) async fn start_for(
    table: Table,
    command: String,
    cwd: Option<String>,
    limit: Duration,
) -> Reply {
    table.tasks().start_limited(command, cwd, Some(limit)).await
}

/// Runs `command` as a task and waits for it, up to `limit`; one still
/// running then is stopped. The reply is the task's status.
pub(crate) async fn run_for(
    table: Table,
    command: String,
    cwd: Option<String>,
    limit: Duration,
) -> Reply {
    let tasks = table.tasks();
    let id = match tasks.start(command, cwd).await {
        Reply::Started { id } => id,
        other => return other,
    };
    let done = tasks.entry(id).map(|entry| entry.done.clone());
    if let Some(mut done) = done
        && timeout(limit, watched::wait_for(&mut done, |done| *done))
            .await
            .is_err()
    {
        return tasks.stop(id).await;
    }

    tasks.status(id)
}

/// A set of tasks. The list is the one copy: the lock inside it is taken to
/// reserve a place (evicting the oldest finished task when the set is full)
/// and again to add the task, and everything that only looks reads it.
#[derive(Clone)]
struct Tasks {
    list: Arc<watch::Sender<Vec<Arc<Entry>>>>,
    /// Starts that hold a place and are not in the list yet.
    starting: Arc<AtomicUsize>,
    /// The last id given out.
    last: Arc<AtomicU32>,
    /// Set by `kill_all`: tasks that finish spawning after it are killed.
    killed: Arc<AtomicBool>,
    /// Holds the next spawn, for a test.
    #[cfg(test)]
    gate: Arc<Mutex<Option<Gate>>>,
}

/// A spawn held in flight: it says it is there, then waits to go on.
#[cfg(test)]
struct Gate {
    reached: oneshot::Sender<()>,
    release: oneshot::Receiver<()>,
}

impl Default for Tasks {
    fn default() -> Self {
        Self {
            list: Arc::new(watch::channel(Vec::new()).0),
            starting: Arc::default(),
            last: Arc::new(AtomicU32::new(first_id())),
            killed: Arc::default(),
            #[cfg(test)]
            gate: Arc::default(),
        }
    }
}

/// A place in the set for a task that is starting, given back when dropped.
struct Place(Arc<AtomicUsize>);

impl Drop for Place {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Where a run of the node starts numbering tasks: from the clock, so a
/// restarted node does not hand out an id the model still holds for a task
/// that died with the old one.
pub(super) fn first_id() -> u32 {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    // Runs started a second apart overlap only after the first gave out a
    // hundred ids.
    u32::try_from(secs % 1_000_000 * 100).unwrap_or(0)
}

/// What readers of the list share of a task. Its lines are written by the
/// task itself.
struct Entry {
    id: TaskId,
    command: String,
    shared: Arc<Mutex<Shared>>,
    /// Asks the task to stop.
    stop: CancellationToken,
    /// Turns true once the task ended.
    done: watch::Receiver<bool>,
    /// The task's process group, for `kill_all` at exit.
    group: Option<Group>,
    /// Dropping the entry ends its task, and the group with it.
    _task: AbortOnDropHandle<()>,
}

struct Shared {
    state: TaskState,
    lines: VecDeque<String>,
    earlier: bool,
}

impl Tasks {
    async fn answer(&self, request: Request) -> Reply {
        match request {
            Request::Start { command, cwd } => self.start(command, cwd).await,
            Request::List {} => Reply::Tasks {
                tasks: watched::read_sent(&self.list, |list| {
                    list.iter().map(|entry| entry.task()).collect()
                }),
            },
            Request::Status { id } => self.status(id),
            Request::Stop { id } => self.stop(id).await,
            other => Reply::Error {
                message: format!("this node does not handle {other:?}"),
            },
        }
    }

    async fn start(&self, command: String, cwd: Option<String>) -> Reply {
        self.start_limited(command, cwd, None).await
    }

    /// The start runs as its own task: a caller that goes away leaves the
    /// task running, as a task belongs to the node.
    async fn start_limited(
        &self,
        command: String,
        cwd: Option<String>,
        limit: Option<Duration>,
    ) -> Reply {
        let tasks = self.clone();

        tokio::spawn(async move { tasks.launch(command, cwd, limit).await })
            .await
            .unwrap_or_else(|_| Reply::Error {
                message: "Could not start the background process.".to_owned(),
            })
    }

    async fn launch(&self, command: String, cwd: Option<String>, limit: Option<Duration>) -> Reply {
        let place = match self.reserve() {
            Ok(place) => place,
            Err(reply) => return reply,
        };

        // Spawning a process is slow, so it runs off the async workers.
        #[cfg(test)]
        let gate = self.gate.clone();
        let spawned = tokio::task::spawn_blocking(move || {
            #[cfg(test)]
            if let Some(gate) = lock(&gate).take() {
                let _ = gate.reached.send(());
                let _ = gate.release.blocking_recv();
            }

            let cwd = cwd.as_ref().map(PathBuf::from).or_else(dirs::home_dir);
            let process = actor::job_blocking("tasks", "spawn", || spawn(&command, cwd));
            (command, process)
        })
        .await;
        let (command, process) = match spawned {
            Ok(spawned) => spawned,
            Err(e) => {
                log::warn!("spawning a background task failed: {e}");
                return Reply::Error {
                    message: "Could not start the background process.".to_owned(),
                };
            }
        };
        let (leader, stdout) = match process {
            Ok(process) => process,
            Err(e) => {
                return Reply::Error {
                    message: format!("{e:#}"),
                };
            }
        };

        let id = TaskId(self.last.fetch_add(1, Ordering::SeqCst).wrapping_add(1));
        let shared = Arc::new(Mutex::new(Shared {
            state: TaskState::Running,
            lines: VecDeque::new(),
            earlier: false,
        }));
        let stop = CancellationToken::new();
        let (done_tx, done_rx) = watch::channel(false);
        let group = leader.group();
        let task = tokio::spawn(follow(
            leader,
            stdout,
            shared.clone(),
            stop.clone(),
            done_tx,
            limit.map(|limit| Instant::now() + limit),
        ));
        let entry = Arc::new(Entry {
            id,
            command,
            shared,
            stop,
            done: done_rx,
            group,
            _task: AbortOnDropHandle::new(task),
        });

        // The place goes back as the task is listed, so the two never count
        // together.
        self.list.send_modify(|list| {
            list.push(entry.clone());
            drop(place);
        });

        // After the list, so `kill_all` either sees the task listed or this
        // sees its flag.
        if self.killed.load(Ordering::SeqCst)
            && let Some(group) = &entry.group
        {
            group.kill();
        }

        crate::events::system().changed(beep_proto::node::events::Topic::new("processes"));

        Reply::Started { id }
    }

    /// Takes a place for a start, evicting the oldest finished task when
    /// the set is full. The count and the eviction happen under the list's
    /// lock, so two starts never take the last place together.
    fn reserve(&self) -> Result<Place, Reply> {
        let mut taken = false;
        self.list.send_if_modified(|list| {
            if list.len() + self.starting.load(Ordering::SeqCst) < MAX_TASKS {
                self.starting.fetch_add(1, Ordering::SeqCst);
                taken = true;
                return false;
            }

            // Room comes from the oldest finished task.
            let Some(oldest) = list
                .iter()
                .position(|entry| watched::read(&entry.done, |done| *done))
            else {
                return false;
            };
            list.remove(oldest);
            self.starting.fetch_add(1, Ordering::SeqCst);
            taken = true;

            true
        });

        if !taken {
            let message =
                format!("Background process limit reached ({MAX_TASKS}). Stop a process first.");
            return Err(Reply::Error { message });
        }

        Ok(Place(self.starting.clone()))
    }

    fn entry(&self, id: TaskId) -> Option<Arc<Entry>> {
        watched::read_sent(&self.list, |list| {
            list.iter().find(|entry| entry.id == id).cloned()
        })
    }

    fn status(&self, id: TaskId) -> Reply {
        match self.entry(id) {
            Some(entry) => entry.status(),
            None => unknown(id),
        }
    }

    fn kill_all(&self) {
        // Set first: a spawn still running kills its task when it lands.
        self.killed.store(true, Ordering::SeqCst);
        let entries = watched::read_sent(&self.list, Clone::clone);

        for entry in entries {
            // A finished task's leader is reaped, so its group id may be
            // someone else's now.
            if let Some(group) = entry.group.as_ref()
                && !watched::read(&entry.done, |done| *done)
            {
                group.kill();
            }
        }
    }

    async fn stop(&self, id: TaskId) -> Reply {
        let Some(entry) = self.entry(id) else {
            return unknown(id);
        };
        // A task that already ended is not listening.
        entry.stop.cancel();
        let mut done = entry.done.clone();

        if timeout(
            STOP_GRACE + STOP_WAIT,
            watched::wait_for(&mut done, |done| *done),
        )
        .await
        .is_err()
        {
            log::warn!("background task {id} did not stop in time");
        }

        self.status(id)
    }
}

impl Entry {
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
    let (program, mut child) = shell::one_shot(command);
    child
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    own_group(&mut child);

    if let Some(cwd) = cwd {
        child.current_dir(cwd);
    }

    spawn_env::apply(&mut child, Scope::Shell);
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
    stop: CancellationToken,
    done: watch::Sender<bool>,
    deadline: Option<Instant>,
) {
    let deadline = async {
        match deadline {
            Some(deadline) => sleep_until(deadline).await,
            None => std::future::pending().await,
        }
    };
    tokio::pin!(deadline);
    let mut stdout = BufReader::new(stdout);
    let mut line = Vec::new();
    // Past the first piece of a long line: the rest is dropped, as `keep`
    // only holds the start of a line anyway.
    let mut cut = false;
    let mut stopped = false;

    loop {
        line.clear();
        tokio::select! {
            () = &mut deadline => {
                stopped = true;
                break;
            }
            () = stop.cancelled() => {
                stopped = true;
                break;
            }
            read = read_piece(&mut stdout, &mut line) => match read {
                Ok(0) => break,
                Ok(_) => {
                    if !cut {
                        keep(&shared, &line);
                    }
                    cut = line.last() != Some(&b'\n');
                }
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
            () = &mut deadline => {
                leader.stop(STOP_GRACE).await;
                TaskState::Stopped
            }
            () = stop.cancelled() => {
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
    crate::events::system().changed(beep_proto::node::events::Topic::new("processes"));
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
    crate::events::system().changed(beep_proto::node::events::Topic::new("process-output"));
}

#[cfg(test)]
#[path = "../tests/run_tasks.rs"]
mod tests;
