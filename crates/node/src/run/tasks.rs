//! `process/1`: background commands. They belong to the node, not to the
//! stream that started them, so they outlive the turn; the node keeps at
//! most `MAX_TASKS` and the last `MAX_LINES` lines of each.

use crate::{actor, lock};
use crate::{
    machine::provision::spawn_env::{self, Scope},
    run::lines::read_piece,
    run::process::{Group, GroupLeader, STOP_GRACE, own_group},
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
        Arc, LazyLock, Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::{
    io::BufReader,
    process::ChildStdout,
    sync::{oneshot, watch},
    task::{JoinError, JoinSet},
    time::{Instant, sleep_until, timeout},
};
use tokio_util::{sync::CancellationToken, task::AbortOnDropHandle};

pub use beep_proto::node::process::PROTOCOL;

/// The longest output line kept, in bytes.
const MAX_LINE_BYTES: usize = 1000;
/// How long a stop waits for the task to end, past the group's grace.
const STOP_WAIT: Duration = Duration::from_secs(5);
/// Commands waiting for the actor.
const MAILBOX: usize = 16;

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

/// A set of tasks. An actor owns it and is its only writer; the set is
/// published as `list`, and everything that only looks reads that. Starting
/// is a command. The actor starts with the first one, as a set can be made
/// outside a runtime.
struct Tasks {
    /// Written by the actor only.
    list: Arc<watch::Sender<Vec<Arc<Entry>>>>,
    actor: OnceLock<Started>,
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

struct Started {
    commands: actor::Sender<Start>,
    _task: AbortOnDropHandle<()>,
}

impl Default for Tasks {
    fn default() -> Self {
        Self {
            list: Arc::new(watch::channel(Vec::new()).0),
            actor: OnceLock::new(),
            killed: Arc::default(),
            #[cfg(test)]
            gate: Arc::default(),
        }
    }
}

struct Start {
    command: String,
    cwd: Option<String>,
    limit: Option<Duration>,
    reply: oneshot::Sender<Reply>,
}

impl From<&Start> for &'static str {
    fn from(_: &Start) -> Self {
        "Start"
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
/// task itself, so they never pass through the actor.
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
}

/// A task as the actor keeps it.
struct Job {
    entry: Arc<Entry>,
    /// Dropping the job ends its task, and the group with it.
    _task: AbortOnDropHandle<()>,
}

struct Shared {
    state: TaskState,
    lines: VecDeque<String>,
    earlier: bool,
}

/// A command being spawned off the loop.
struct Spawned {
    start: Start,
    process: Result<(GroupLeader, ChildStdout)>,
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

    async fn start_limited(
        &self,
        command: String,
        cwd: Option<String>,
        limit: Option<Duration>,
    ) -> Reply {
        let (reply, answer) = oneshot::channel();
        let start = Start {
            command,
            cwd,
            limit,
            reply,
        };

        match self.commands().send(start).await {
            Ok(()) => answer.await.unwrap_or_else(|_| Reply::Error {
                // The reply is dropped when spawning panicked.
                message: "Could not start the background process.".to_owned(),
            }),
            Err(_) => stopped(),
        }
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
            if let Some(group) = entry
                .group
                .as_ref()
                .filter(|_| !watched::read(&entry.done, |done| *done))
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

    /// The actor's mailbox, starting the actor the first time. The actor
    /// runs on the runtime that sends the first command, which is the only
    /// one in the node; it stops answering if that runtime ends.
    fn commands(&self) -> &actor::Sender<Start> {
        let started = self.actor.get_or_init(|| {
            let (commands, mailbox) = actor::channel("tasks", MAILBOX);
            let actor = Actor {
                last: first_id(),
                jobs: Vec::new(),
                spawning: JoinSet::new(),
                list: self.list.clone(),
                killed: self.killed.clone(),
                #[cfg(test)]
                gate: self.gate.clone(),
            };

            Started {
                commands,
                _task: AbortOnDropHandle::new(tokio::spawn(actor.run(mailbox))),
            }
        });

        &started.commands
    }
}

fn stopped() -> Reply {
    Reply::Error {
        message: "The background processes stopped.".to_owned(),
    }
}

/// Owns the set of tasks.
struct Actor {
    last: u32,
    jobs: Vec<Job>,
    /// Commands being spawned off the loop. Each holds a place.
    spawning: JoinSet<Spawned>,
    list: Arc<watch::Sender<Vec<Arc<Entry>>>>,
    killed: Arc<AtomicBool>,
    #[cfg(test)]
    gate: Arc<Mutex<Option<Gate>>>,
}

impl Actor {
    async fn run(mut self, mut mailbox: actor::Receiver<Start>) {
        loop {
            tokio::select! {
                start = mailbox.recv() => match start {
                    Some((start, _timing)) => self.start(start),
                    None => return,
                },
                Some(spawned) = self.spawning.join_next() => self.spawned(spawned),
            }
        }
    }

    fn start(&mut self, start: Start) {
        if self.jobs.len() + self.spawning.len() >= MAX_TASKS {
            // Room comes from the oldest finished task.
            match self
                .jobs
                .iter()
                .position(|job| watched::read(&job.entry.done, |done| *done))
            {
                Some(oldest) => {
                    self.jobs.remove(oldest);
                    self.publish();
                }
                None => {
                    let message = format!(
                        "Background process limit reached ({MAX_TASKS}). Stop a process first."
                    );
                    let _ = start.reply.send(Reply::Error { message });
                    return;
                }
            }
        }

        // Spawning a process is slow, so it runs off the loop.
        #[cfg(test)]
        let gate = self.gate.clone();
        self.spawning.spawn_blocking(move || {
            #[cfg(test)]
            if let Some(gate) = lock(&gate).take() {
                let _ = gate.reached.send(());
                let _ = gate.release.blocking_recv();
            }

            let cwd = start
                .cwd
                .as_ref()
                .map(PathBuf::from)
                .or_else(dirs::home_dir);
            let process = actor::job_blocking("tasks", "spawn", || spawn(&start.command, cwd));
            Spawned { start, process }
        });
    }

    fn spawned(&mut self, joined: Result<Spawned, JoinError>) {
        let Spawned { start, process } = match joined {
            Ok(spawned) => spawned,
            // The caller's reply is dropped with it, which it reports.
            Err(e) => return log::warn!("spawning a background task failed: {e}"),
        };
        let (leader, stdout) = match process {
            Ok(process) => process,
            Err(e) => {
                let _ = start.reply.send(Reply::Error {
                    message: format!("{e:#}"),
                });
                return;
            }
        };
        self.last = self.last.wrapping_add(1);
        let id = TaskId(self.last);
        let shared = Arc::new(Mutex::new(Shared {
            state: TaskState::Running,
            lines: VecDeque::new(),
            earlier: false,
        }));
        let stop = CancellationToken::new();
        let (done_tx, done_rx) = watch::channel(false);
        let entry = Arc::new(Entry {
            id,
            command: start.command,
            shared: shared.clone(),
            stop: stop.clone(),
            done: done_rx,
            group: leader.group(),
        });
        let task = tokio::spawn(follow(
            leader,
            stdout,
            shared,
            stop,
            done_tx,
            start.limit.map(|limit| Instant::now() + limit),
        ));
        self.jobs.push(Job {
            entry,
            _task: AbortOnDropHandle::new(task),
        });
        self.publish();

        // After the publish, so `kill_all` either sees the task listed or
        // this sees its flag.
        if self.killed.load(Ordering::SeqCst)
            && let Some(job) = self.jobs.last()
            && let Some(group) = &job.entry.group
        {
            group.kill();
        }

        crate::events::system().changed(beep_proto::node::events::Topic::new("processes"));
        // The caller may have gone, which leaves the task running.
        let _ = start.reply.send(Reply::Started { id });
    }

    fn publish(&self) {
        self.list
            .send_replace(self.jobs.iter().map(|job| job.entry.clone()).collect());
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
