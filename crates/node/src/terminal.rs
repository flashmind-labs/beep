//! `terminal/1`: interactive terminals. Each runs in a pseudo-terminal the
//! node owns, not the stream that started it, so it outlives the stream and
//! can be attached to again. The node keeps at most `MAX_TERMINALS` and the
//! last `MAX_SCROLLBACK` bytes of each.
//!
//! A terminal has three threads, each ending with it: one reads the
//! pseudo-terminal into the scrollback, one writes typed keys to it, and
//! one waits for the child to exit. The pseudo-terminal's calls block, so
//! they stay off the async workers.

use crate::provision::spawn_env::{self, Scope};
use anyhow::{Context, Result};
use beep_proto::node::{
    LinkRx, LinkTx,
    process::{Task, TaskId, TaskState},
    terminal::{MAX_INPUT, MAX_OUTPUT, MAX_SCROLLBACK, MAX_TERMINALS, Reply, Request, Size},
};
use portable_pty::{Child, ChildKiller, CommandBuilder, MasterPty, PtySize, native_pty_system};
use serde::{Deserialize, Serialize};
use serde_bytes::ByteBuf;
use std::{
    collections::VecDeque,
    io::{Read, Write},
    path::PathBuf,
    sync::{
        Arc, LazyLock, Mutex, MutexGuard, PoisonError,
        atomic::{AtomicBool, Ordering},
        mpsc as std_mpsc,
    },
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::{
    sync::{mpsc, watch},
    time::timeout,
};

pub use beep_proto::node::terminal::PROTOCOL;

/// How long a hung-up terminal has to exit before it is killed.
const STOP_GRACE: Duration = Duration::from_secs(2);
/// How long a stop waits for the kill to land.
const STOP_WAIT: Duration = Duration::from_secs(5);
/// Typed input waiting for the terminal to read it; more is dropped.
const INPUT_QUEUE: usize = 64;
/// How long the exit waits for the last output to be read.
const DRAIN_WAIT: Duration = Duration::from_millis(500);
/// Bytes read from the pseudo-terminal at a time.
const READ_SIZE: usize = 8 * 1024;

/// The node's terminals: one set per process, like the background tasks.
static TERMINALS: LazyLock<Terminals> = LazyLock::new(Terminals::default);

/// Kills every terminal still running, at once, for a host about to exit.
pub fn kill_all() {
    TERMINALS.kill_all();
}

/// A terminal open when the node stopped, to open again as a new shell
/// under the same id when it starts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct OpenTerminal {
    id: u32,
    /// Where its shell was last, else where it started.
    cwd: Option<PathBuf>,
    /// What it ran.
    command: String,
}

/// The terminals running now.
pub(crate) fn running() -> Vec<OpenTerminal> {
    TERMINALS.running()
}

/// Opens `terminals` again, each a new shell in its folder that says so.
pub(crate) fn reopen(terminals: Vec<OpenTerminal>) {
    for open in terminals {
        if let Err(e) = TERMINALS.reopen(&open) {
            log::warn!("could not reopen terminal {}: {e:#}", open.id);
        }
    }
}

/// Serves one session stream until the server closes it. An `Attach`
/// keeps the stream to itself until the terminal ends.
pub async fn serve(tx: LinkTx, rx: LinkRx) -> Result<()> {
    serve_with(&TERMINALS, tx, rx).await
}

async fn serve_with(terminals: &Terminals, mut tx: LinkTx, mut rx: LinkRx) -> Result<()> {
    while let Some(request) = rx.recv::<Request>().await? {
        if let Request::Attach { id, from } = request {
            return match terminals.live(id) {
                Some(live) => attach(live, from, tx, rx).await,
                None => tx.send(&unknown(id)).await,
            };
        }
        let reply = terminals.answer(request).await;
        tx.send(&reply).await?;
    }
    Ok(())
}

#[derive(Default)]
struct Terminals {
    inner: Mutex<Inner>,
}

struct Inner {
    last: u32,
    terms: Vec<Term>,
}

impl Default for Inner {
    fn default() -> Self {
        Self {
            // From the clock, so a restarted node does not hand out the
            // number of a terminal that died with the old one.
            last: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |since| {
                    u32::try_from(since.as_secs() % 1_000_000 * 100).unwrap_or(0)
                }),
            terms: Vec::new(),
        }
    }
}

/// One terminal, as the node keeps it.
struct Term {
    live: Arc<Live>,
    /// Ends the child; on Unix its group is hung up instead.
    killer: Box<dyn ChildKiller + Send + Sync>,
    /// The child's process group: it leads a session of its own.
    #[cfg(unix)]
    group: Option<crate::process::Group>,
    /// The child's process id.
    pid: Option<u32>,
    /// The folder it started in.
    cwd: Option<PathBuf>,
}

/// What an attached stream shares with the terminal.
struct Live {
    id: TaskId,
    command: String,
    screen: Mutex<Screen>,
    /// Bumped whenever output comes or the state changes.
    changed: watch::Sender<u64>,
    /// Typed keys, for the writer thread.
    input: mpsc::Sender<Vec<u8>>,
    master: Mutex<Box<dyn MasterPty + Send>>,
    /// Set by a stop, so the exit reads as stopped.
    stopping: AtomicBool,
}

/// The kept output and how the terminal stands.
struct Screen {
    bytes: VecDeque<u8>,
    /// The offset of `bytes[0]` in all the output so far.
    start: u64,
    state: TaskState,
}

impl Screen {
    fn end(&self) -> u64 {
        self.start + self.bytes.len() as u64
    }

    fn push(&mut self, data: &[u8]) {
        self.bytes.extend(data);
        let over = self.bytes.len().saturating_sub(MAX_SCROLLBACK);
        if over > 0 {
            self.bytes.drain(..over);
            self.start += over as u64;
        }
    }

    /// Up to `MAX_OUTPUT` bytes from `from`, or from the oldest kept when
    /// `from` is no longer kept, with where they start.
    fn read_from(&self, from: u64) -> Option<(u64, Vec<u8>)> {
        let at = from.clamp(self.start, self.end());
        let skip = usize::try_from(at - self.start).ok()?;
        let take = (self.bytes.len() - skip).min(MAX_OUTPUT);
        (take > 0).then(|| (at, self.bytes.range(skip..skip + take).copied().collect()))
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Live {
    fn task(&self) -> Task {
        Task {
            id: self.id,
            command: self.command.clone(),
            state: lock(&self.screen).state,
        }
    }

    fn is_running(&self) -> bool {
        lock(&self.screen).state.is_running()
    }

    /// Sizes the terminal to `size`, unless it already is: a resize makes
    /// some shells draw their prompt again, and every attach sends one.
    /// Returns whether it changed.
    fn resize(&self, size: Size) -> bool {
        let master = lock(&self.master);
        if master
            .get_size()
            .is_ok_and(|now| now.cols == size.cols && now.rows == size.rows)
        {
            return false;
        }
        if let Err(e) = master.resize(pty_size(size)) {
            log::debug!("could not resize terminal {}: {e:#}", self.id);
            return false;
        }
        true
    }

    /// Hands typed keys to the writer; dropped when the terminal is not
    /// reading, so a stuck program never holds up its output.
    fn type_keys(&self, data: Vec<u8>) {
        if let Err(e) = self.input.try_send(data) {
            log::debug!("dropped input to terminal {}: {e}", self.id);
        }
    }

    async fn wait_ended(&self, limit: Duration) -> bool {
        let mut changed = self.changed.subscribe();
        timeout(limit, async {
            while self.is_running() {
                if changed.changed().await.is_err() {
                    return;
                }
            }
        })
        .await
        .is_ok()
    }
}

impl Terminals {
    fn live(&self, id: TaskId) -> Option<Arc<Live>> {
        lock(&self.inner)
            .terms
            .iter()
            .find(|term| term.live.id == id)
            .map(|term| term.live.clone())
    }

    async fn answer(&self, request: Request) -> Reply {
        match request {
            Request::Start { command, cwd, size } => self.start(command, cwd, size),
            Request::List {} => Reply::Terminals {
                terminals: self.list(),
            },
            Request::Stop { id } => self.stop(id).await,
            other => Reply::Error {
                message: format!("{other:?} needs an attached terminal"),
            },
        }
    }

    /// Every terminal as it stands. One that ended is listed this once,
    /// with how it ended, then dropped: a stream still attached to it holds
    /// its own handle until it has sent the end.
    fn list(&self) -> Vec<Task> {
        let mut inner = lock(&self.inner);
        let listed = inner.terms.iter().map(|term| term.live.task()).collect();
        inner.terms.retain(|term| term.live.is_running());
        listed
    }

    fn start(&self, command: Option<String>, cwd: Option<String>, size: Size) -> Reply {
        let mut inner = lock(&self.inner);
        if inner.terms.len() >= MAX_TERMINALS {
            match inner.terms.iter().position(|term| !term.live.is_running()) {
                Some(oldest) => {
                    inner.terms.remove(oldest);
                }
                None => {
                    return Reply::Error {
                        message: format!(
                            "Terminal limit reached ({MAX_TERMINALS}). Close a terminal first."
                        ),
                    };
                }
            }
        }
        inner.last = inner.last.wrapping_add(1);
        let id = TaskId(inner.last);
        match open(id, command.as_deref(), cwd, size, None) {
            Ok(term) => {
                let task = term.live.task();
                inner.terms.push(term);
                Reply::Started { task }
            }
            Err(e) => Reply::Error {
                message: format!("{e:#}"),
            },
        }
    }

    async fn stop(&self, id: TaskId) -> Reply {
        let Some(live) = self.live(id) else {
            return unknown(id);
        };
        if live.is_running() {
            live.stopping.store(true, Ordering::Release);
            self.signal(id, false);
            if !live.wait_ended(STOP_GRACE).await {
                log::debug!("terminal {id} outlived its hang-up; killing it");
                self.signal(id, true);
                if !live.wait_ended(STOP_WAIT).await {
                    log::warn!("terminal {id} did not stop in time");
                }
            }
        }
        Reply::Stopped { task: live.task() }
    }

    /// Hangs up terminal `id`, or kills it when `hard`, while it runs: once
    /// its child is reaped the group's id may be someone else's.
    fn signal(&self, id: TaskId, hard: bool) {
        let mut inner = lock(&self.inner);
        let Some(term) = inner.terms.iter_mut().find(|term| term.live.id == id) else {
            return;
        };
        if term.live.is_running() {
            term.signal(hard);
        }
    }

    fn running(&self) -> Vec<OpenTerminal> {
        lock(&self.inner)
            .terms
            .iter()
            .filter(|term| term.live.is_running())
            .map(|term| OpenTerminal {
                id: term.live.id.0,
                cwd: term.pid.and_then(current_dir).or_else(|| term.cwd.clone()),
                command: term.live.command.clone(),
            })
            .collect()
    }

    /// Opens a new shell for `open` under its id, unless one runs there.
    fn reopen(&self, was: &OpenTerminal) -> Result<()> {
        let mut inner = lock(&self.inner);
        let id = TaskId(was.id);
        anyhow::ensure!(
            inner.terms.len() < MAX_TERMINALS,
            "{MAX_TERMINALS} terminals already run"
        );
        anyhow::ensure!(
            inner.terms.iter().all(|term| term.live.id != id),
            "terminal {id} already runs"
        );
        let cwd = was.cwd.as_ref().filter(|cwd| cwd.is_dir());
        let banner = reopened_banner(was, cwd);
        let cwd = cwd.map(|cwd| cwd.to_string_lossy().into_owned());
        let term = open(id, None, cwd, Size::DEFAULT, Some(&banner))?;
        // Ids handed out later stay clear of it.
        inner.last = inner.last.max(was.id);
        inner.terms.push(term);
        Ok(())
    }

    fn kill_all(&self) {
        for term in &mut lock(&self.inner).terms {
            if term.live.is_running() {
                term.signal(true);
            }
        }
    }
}

impl Term {
    fn signal(&mut self, hard: bool) {
        #[cfg(unix)]
        if let Some(group) = &self.group {
            if hard {
                group.kill();
            } else {
                group.hang_up();
            }
            return;
        }
        // Windows has no gentle way; elsewhere SIGHUP to the child alone.
        let _ = hard;
        if let Err(e) = self.killer.kill() {
            log::debug!("could not end terminal {}: {e}", self.live.id);
        }
    }
}

fn unknown(id: TaskId) -> Reply {
    Reply::Error {
        message: format!("No terminal {id}."),
    }
}

fn pty_size(size: Size) -> PtySize {
    PtySize {
        rows: size.rows,
        cols: size.cols,
        pixel_width: 0,
        pixel_height: 0,
    }
}

/// The shell terminals run in, and the arguments that run `command` with
/// it or, without one, start it interactive.
fn shell_command(command: Option<&str>) -> (String, CommandBuilder) {
    #[cfg(windows)]
    {
        let program = "powershell.exe";
        let mut builder = CommandBuilder::new(program);
        builder.arg("-NoLogo");
        if let Some(command) = command {
            builder.args(["-Command", command]);
        }
        (program.to_owned(), builder)
    }
    #[cfg(not(windows))]
    {
        let program = std::env::var("SHELL")
            .ok()
            .filter(|shell| !shell.is_empty() && std::path::Path::new(shell).exists())
            .or_else(|| {
                which::which("bash")
                    .ok()
                    .map(|p| p.to_string_lossy().into_owned())
            })
            .unwrap_or_else(|| "/bin/sh".to_owned());
        let mut builder = CommandBuilder::new(&program);
        // A login shell reads the profile, as a terminal app's would.
        builder.arg("-l");
        if let Some(command) = command {
            builder.args(["-c", command]);
        }
        (program, builder)
    }
}

/// The first line of a terminal opened again after a restart.
fn reopened_banner(open: &OpenTerminal, cwd: Option<&PathBuf>) -> String {
    let place = cwd.map_or_else(
        || "the home folder".to_owned(),
        |cwd| cwd.display().to_string(),
    );
    format!(
        "\x1b[2m[beep-node restarted: this is a new shell in {place}; it was {}]\x1b[0m\r\n",
        open.command
    )
}

/// The folder process `pid` is in, where the system says.
fn current_dir(pid: u32) -> Option<PathBuf> {
    if cfg!(target_os = "linux") {
        std::fs::read_link(format!("/proc/{pid}/cwd")).ok()
    } else {
        None
    }
}

/// Opens a pseudo-terminal of `size` and starts the shell in it, its
/// output starting with `banner` when given.
fn open(
    id: TaskId,
    command: Option<&str>,
    cwd: Option<String>,
    size: Size,
    banner: Option<&str>,
) -> Result<Term> {
    let command = command.map(str::trim).filter(|command| !command.is_empty());
    let (program, mut builder) = shell_command(command);
    let cwd = cwd.map(PathBuf::from).or_else(dirs::home_dir);
    if let Some(cwd) = &cwd {
        builder.cwd(cwd);
    }
    builder.env("TERM", "xterm-256color");
    builder.env("COLORTERM", "truecolor");
    for (name, value) in spawn_env::vars(Scope::Shell) {
        builder.env(name, value);
    }
    let pair = native_pty_system()
        .openpty(pty_size(size))
        .context("opening a pseudo-terminal")?;
    let child = pair
        .slave
        .spawn_command(builder)
        .with_context(|| format!("starting {program}"))?;
    // Only the child holds the terminal's other end now, so reading ends
    // once it and what it started are gone.
    drop(pair.slave);
    let reader = pair
        .master
        .try_clone_reader()
        .context("reading the terminal")?;
    let writer = pair
        .master
        .take_writer()
        .context("writing to the terminal")?;
    let killer = child.clone_killer();
    let pid = child.process_id();
    #[cfg(unix)]
    let group = pid.and_then(crate::process::Group::led_by);
    let name = match command {
        Some(command) => command.to_owned(),
        None => program
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or(&program)
            .to_owned(),
    };
    let (input_tx, input_rx) = mpsc::channel(INPUT_QUEUE);
    let live = Arc::new(Live {
        id,
        command: name,
        screen: Mutex::new(Screen {
            bytes: banner.map(|b| b.bytes().collect()).unwrap_or_default(),
            start: 0,
            state: TaskState::Running,
        }),
        changed: watch::channel(0).0,
        input: input_tx,
        master: Mutex::new(pair.master),
        stopping: AtomicBool::new(false),
    });
    let (drained_tx, drained_rx) = std_mpsc::sync_channel(1);
    spawn_thread("terminal-read", {
        let live = live.clone();
        move || read_output(&live, reader, &drained_tx)
    })?;
    spawn_thread("terminal-write", move || write_input(input_rx, writer))?;
    spawn_thread("terminal-wait", {
        let live = live.clone();
        move || wait_exit(&live, child, &drained_rx)
    })?;
    // Seen once the caller, which holds the terminals, kept it.
    crate::restart::changed();
    Ok(Term {
        live,
        killer,
        #[cfg(unix)]
        group,
        pid,
        cwd,
    })
}

fn spawn_thread(name: &str, run: impl FnOnce() + Send + 'static) -> Result<()> {
    thread::Builder::new()
        .name(name.to_owned())
        .spawn(run)
        .with_context(|| format!("starting {name}"))?;
    Ok(())
}

/// Keeps the terminal's output until it closes.
fn read_output(live: &Live, mut reader: Box<dyn Read + Send>, drained: &std_mpsc::SyncSender<()>) {
    let mut buf = vec![0; READ_SIZE];
    loop {
        match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                lock(&live.screen).push(&buf[..n]);
                live.changed.send_modify(|n| *n += 1);
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            // Linux says EIO once the other end closed.
            Err(e) => {
                log::debug!("terminal {} closed: {e}", live.id);
                break;
            }
        }
    }
    // The waiter may have given up waiting already.
    let _ = drained.try_send(());
}

/// Writes typed keys until the terminal is gone.
fn write_input(mut input: mpsc::Receiver<Vec<u8>>, mut writer: Box<dyn Write + Send>) {
    while let Some(data) = input.blocking_recv() {
        if let Err(e) = writer.write_all(&data).and_then(|()| writer.flush()) {
            log::debug!("writing to a terminal failed: {e}");
            return;
        }
    }
}

/// Waits for the child to exit and marks how it ended, after its last
/// output was read.
fn wait_exit(
    live: &Live,
    mut child: Box<dyn Child + Send + Sync>,
    drained: &std_mpsc::Receiver<()>,
) {
    let status = child.wait();
    // Something the child started may hold the terminal open; its output
    // then keeps coming, but the terminal counts as ended.
    if drained.recv_timeout(DRAIN_WAIT).is_err() {
        log::debug!("terminal {} ended with its output still open", live.id);
    }
    let state = if live.stopping.load(Ordering::Acquire) {
        TaskState::Stopped
    } else {
        match status {
            Ok(status) if status.signal().is_some() => TaskState::Exited { code: None },
            Ok(status) => TaskState::Exited {
                code: i32::try_from(status.exit_code()).ok(),
            },
            Err(e) => {
                log::warn!("could not wait for terminal {}: {e}", live.id);
                TaskState::Exited { code: None }
            }
        }
    };
    lock(&live.screen).state = state;
    live.changed.send_modify(|n| *n += 1);
    crate::restart::changed();
}

/// Streams terminal `live` from byte `from` and types what the server
/// sends into it, until it ends or the server closes the stream.
async fn attach(live: Arc<Live>, from: u64, mut tx: LinkTx, mut rx: LinkRx) -> Result<()> {
    let mut changed = live.changed.subscribe();
    let mut next = from;
    loop {
        // Marked seen before reading, so output that lands after the read
        // still wakes the wait below.
        changed.borrow_and_update();
        let (chunk, state) = {
            let screen = lock(&live.screen);
            (screen.read_from(next), screen.state)
        };
        if let Some((at, data)) = chunk {
            next = at + data.len() as u64;
            tx.send(&Reply::Output {
                at,
                data: ByteBuf::from(data),
            })
            .await?;
            continue;
        }
        if !state.is_running() {
            return tx.send(&Reply::Ended { task: live.task() }).await;
        }
        tokio::select! {
            woke = changed.changed() => if woke.is_err() {
                return Ok(());
            },
            request = rx.recv::<Request>() => match request {
                Ok(Some(Request::Input { data })) if data.len() <= MAX_INPUT => {
                    live.type_keys(data.into_vec());
                }
                Ok(Some(Request::Input { data })) => {
                    log::debug!("dropped {} bytes of input past the limit", data.len());
                }
                Ok(Some(Request::Resize { size })) => {
                    live.resize(size);
                }
                Ok(Some(other)) => log::debug!("ignored {other:?} on an attached terminal"),
                // The server detached.
                Ok(None) | Err(_) => return Ok(()),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    fn started(reply: Reply) -> TaskId {
        match reply {
            Reply::Started { task } => task.id,
            other => panic!("not started: {other:?}"),
        }
    }

    /// Waits until the terminal's output so far holds `needle`.
    #[cfg(unix)]
    async fn wait_for_output(live: &Live, needle: &str) -> String {
        let mut changed = live.changed.subscribe();
        timeout(Duration::from_secs(20), async {
            loop {
                changed.borrow_and_update();
                let bytes: Vec<u8> = lock(&live.screen).bytes.iter().copied().collect();
                let text = String::from_utf8_lossy(&bytes).into_owned();
                if text.contains(needle) {
                    return text;
                }
                changed.changed().await.expect("the terminal is kept");
            }
        })
        .await
        .unwrap_or_else(|_| panic!("no {needle:?} in the terminal"))
    }

    #[test]
    fn the_screen_keeps_its_last_bytes_and_their_offsets() {
        let mut screen = Screen {
            bytes: VecDeque::new(),
            start: 0,
            state: TaskState::Running,
        };
        screen.push(&vec![b'a'; MAX_SCROLLBACK]);
        screen.push(b"tail");
        assert_eq!(screen.start, 4);
        assert_eq!(screen.end(), MAX_SCROLLBACK as u64 + 4);
        // From before what is kept: the oldest kept byte.
        let (at, data) = screen.read_from(0).expect("bytes are kept");
        assert_eq!(at, 4);
        assert_eq!(data.len(), MAX_OUTPUT);
        let (at, data) = screen.read_from(screen.end() - 4).expect("the tail");
        assert_eq!((at, data.as_slice()), (MAX_SCROLLBACK as u64, &b"tail"[..]));
        assert!(screen.read_from(screen.end()).is_none());
        assert!(screen.read_from(screen.end() + 10).is_none());
    }

    // A pseudo-terminal round trip: keys typed reach the shell, and what it
    // prints comes back.
    #[cfg(unix)]
    #[tokio::test]
    async fn typed_keys_reach_the_shell_and_its_output_comes_back() {
        let terminals = Terminals::default();
        let id = started(
            terminals
                .answer(Request::Start {
                    command: Some("read line; echo \"got:$line\"; read wait".into()),
                    cwd: None,
                    size: Size {
                        cols: 100,
                        rows: 30,
                    },
                })
                .await,
        );
        let live = terminals.live(id).expect("the terminal is kept");
        live.type_keys(b"hello\r".to_vec());
        wait_for_output(&live, "got:hello").await;
        assert!(
            live.resize(Size { cols: 50, rows: 10 }),
            "a new size applies"
        );
        assert!(
            !live.resize(Size { cols: 50, rows: 10 }),
            "the same size again is left alone, so the shell does not redraw"
        );
        let size = lock(&live.master).get_size().expect("a size");
        assert_eq!((size.cols, size.rows), (50, 10));
        let Reply::Stopped { task } = terminals.answer(Request::Stop { id }).await else {
            panic!("not stopped");
        };
        assert_eq!(task.state, TaskState::Stopped);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_terminal_ends_with_its_command() {
        let terminals = Terminals::default();
        let id = started(
            terminals
                .answer(Request::Start {
                    command: Some("echo done; exit 3".into()),
                    cwd: None,
                    size: Size::DEFAULT,
                })
                .await,
        );
        let live = terminals.live(id).expect("the terminal is kept");
        assert!(live.wait_ended(Duration::from_secs(20)).await);
        assert_eq!(live.task().state, TaskState::Exited { code: Some(3) });
        wait_for_output(&live, "done").await;
        let Reply::Terminals { terminals: listed } = terminals.answer(Request::List {}).await
        else {
            panic!("no list");
        };
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].command, "echo done; exit 3");
        // Listed with how it ended once, then gone.
        let Reply::Terminals { terminals: listed } = terminals.answer(Request::List {}).await
        else {
            panic!("no list");
        };
        assert!(listed.is_empty());
        assert!(terminals.live(id).is_none());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_terminal_opens_again_as_a_new_shell_in_its_folder() {
        let dir = tempfile::tempdir().expect("a temporary folder");
        let folder = dir.path().canonicalize().expect("the folder resolves");
        let before = Terminals::default();
        let id = started(
            before
                .answer(Request::Start {
                    command: Some("read wait".into()),
                    cwd: Some(folder.display().to_string()),
                    size: Size::DEFAULT,
                })
                .await,
        );
        let open = before.running();
        assert_eq!(open.len(), 1);
        assert_eq!(open[0].id, id.0);
        assert_eq!(open[0].cwd.as_deref(), Some(folder.as_path()));
        before.kill_all();

        let after = Terminals::default();
        after.reopen(&open[0]).expect("the terminal opens again");
        let live = after.live(id).expect("the terminal keeps its id");
        assert!(live.is_running());
        let shown = wait_for_output(&live, "beep-node restarted").await;
        assert!(shown.contains(&folder.display().to_string()), "{shown}");
        assert!(after.reopen(&open[0]).is_err(), "one runs under that id");
        after.kill_all();
    }
}
