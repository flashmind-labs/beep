//! Opening a pseudo-terminal, and the threads that read it, write to it
//! and wait for its child.

use super::screen::{Phase, Screen};
use crate::lock;
use crate::process::spawn_env::{self, Scope};
use anyhow::{Context, Result};
use beep_proto::{node::events::Topic, watched};
use beep_proto::node::{
    EnvSecret,
    process::{Task, TaskId, TaskState},
    terminal::Size,
};
use portable_pty::{
    Child, ChildKiller, CommandBuilder, ExitStatus, MasterPty, PtySize, native_pty_system,
};
use std::{
    io::{self, Read, Write},
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::AtomicBool,
        mpsc as std_mpsc,
    },
    thread,
    time::Duration,
};
use tokio::{
    sync::{Notify, mpsc, watch},
    time::timeout,
};

/// Typed input waiting for the terminal to read it; more is dropped.
const INPUT_QUEUE: usize = 64;
/// How long the exit waits for the last output to be read.
const DRAIN_WAIT: Duration = Duration::from_millis(500);
/// Bytes read from the pseudo-terminal at a time.
const READ_SIZE: usize = 8 * 1024;

/// One terminal, as the node keeps it.
pub(super) struct Term {
    pub(super) live: Arc<Live>,
    /// Ends the child; on Unix its group is hung up instead.
    pub(super) killer: Mutex<Box<dyn ChildKiller + Send + Sync>>,
    /// The child's process group: it leads a session of its own.
    #[cfg(unix)]
    pub(super) group: Option<crate::process::Group>,
    /// The child's process id.
    pub(super) pid: Option<u32>,
    /// The folder it started in.
    pub(super) cwd: Option<PathBuf>,
}

/// What an attached stream shares with the terminal.
pub(super) struct Live {
    pub(super) id: TaskId,
    pub(super) command: String,
    pub(super) screen: Mutex<Screen>,
    /// Bumped whenever output comes or the state changes.
    pub(super) changed: watch::Sender<u64>,
    /// Typed keys, for the writer thread.
    pub(super) input: mpsc::Sender<Vec<u8>>,
    pub(super) master: Mutex<Box<dyn MasterPty + Send>>,
    #[cfg(windows)]
    pub(super) process: Option<std::os::windows::io::OwnedHandle>,
    /// Its set's `changed`.
    pub(super) started_or_ended: Arc<Notify>,
}
impl Live {
    pub(super) fn task(&self) -> Task {
        Task {
            id: self.id,
            command: self.command.clone(),
            state: lock(&self.screen).phase.state(),
        }
    }

    pub(super) fn is_running(&self) -> bool {
        lock(&self.screen).phase.state().is_running()
    }

    /// Says it started or ended, to the node's note and its set.
    pub(super) fn started_or_ended(&self) {
        crate::events::open_changed();
        self.started_or_ended.notify_one();
        crate::events::system().changed(beep_proto::node::events::Topic::new("terminals"));
    }

    /// Sizes the terminal to `size`, unless it already is: a resize makes
    /// some shells draw their prompt again, and every attach sends one.
    /// Returns whether it changed.
    pub(super) fn resize(&self, size: Size) -> bool {
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
    pub(super) fn type_keys(&self, data: Vec<u8>) {
        if let Err(e) = self.input.try_send(data) {
            log::debug!("dropped input to terminal {}: {e}", self.id);
        }
    }

    pub(super) async fn wait_ended(&self, limit: Duration) -> bool {
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

/// What opening a terminal needs, apart from the set.
pub(super) struct Opener {
    /// What terminals get on top of this process's environment, when not
    /// the node's own: a terminal host gets the node's.
    pub(super) env: watch::Sender<Option<Vec<EnvSecret>>>,
    /// Woken when a terminal starts or ends.
    pub(super) changed: Arc<Notify>,
    /// Set by `kill_all`: terminals that finish opening after it are killed.
    pub(super) killed: AtomicBool,
}

pub(super) fn pty_size(size: Size) -> PtySize {
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
impl Opener {
    /// Opens a pseudo-terminal of `size` and starts the shell in it, its
    /// output starting with `banner` when given.
    pub(super) fn open(
        &self,
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

        watched::read_sent(&self.env, |env| match env {
            Some(env) => {
                for var in env {
                    builder.env(&var.name, var.value.expose());
                }
            }
            None => {
                for (name, value) in spawn_env::vars(&Scope::Shell) {
                    builder.env(name, value);
                }
            }
        });

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
        #[cfg(windows)]
        let process = child.as_raw_handle().and_then(|raw| {
            use std::os::windows::io::BorrowedHandle;

            // SAFETY: The child owns the handle until the duplicate is made.
            unsafe { BorrowedHandle::borrow_raw(raw) }
                .try_clone_to_owned()
                .ok()
        });
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
        let mut screen = Screen::new();
        screen.push(banner.unwrap_or_default().as_bytes());
        let live = Arc::new(Live {
            id,
            command: name,
            screen: Mutex::new(screen),
            changed: watch::channel(0).0,
            input: input_tx,
            master: Mutex::new(pair.master),
            #[cfg(windows)]
            process,
            started_or_ended: self.changed.clone(),
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
        live.started_or_ended();
        Ok(Term {
            live,
            killer: Mutex::new(killer),
            #[cfg(unix)]
            group,
            pid,
            cwd,
        })
    }
}

fn spawn_thread(name: &str, run: impl FnOnce() + Send + 'static) -> Result<()> {
    thread::Builder::new()
        .name(name.to_owned())
        .spawn(run)
        .with_context(|| format!("starting {name}"))?;
    Ok(())
}

/// The topic that says terminal `id` printed. A reader of every terminal's
/// output follows `terminal-output` whole; one terminal's follows this.
pub(crate) fn output_topic(id: TaskId) -> Topic {
    Topic::keyed("terminal-output", id.to_string())
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
                crate::events::system().changed(output_topic(live.id));
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
    reap(live, &mut *child);
    live.changed.send_modify(|n| *n += 1);
    live.started_or_ended();
    // Something the child started may hold the terminal open; its output
    // then keeps coming, but the terminal counts as ended.
    if drained.recv_timeout(DRAIN_WAIT).is_err() {
        log::debug!("terminal {} ended with its output still open", live.id);
    }

    {
        let mut screen = lock(&live.screen);
        if let Phase::Draining(state) = screen.phase {
            screen.phase = Phase::Ended(state);
        }
    }
    live.changed.send_modify(|n| *n += 1);
    crate::events::system().changed(output_topic(live.id));
    live.started_or_ended();
}

fn reap(live: &Live, child: &mut dyn Child) {
    #[cfg(windows)]
    if live.process.is_some() {
        // Stop checks the same process object's signaled state before changing
        // the phase. Waiting on it can block without holding the screen lock.
        let status = child.wait();
        mark_draining(&mut lock(&live.screen), live.id, status);
        return;
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    if let Some(pid) = child
        .process_id()
        .and_then(|pid| i32::try_from(pid).ok())
        .and_then(rustix::process::Pid::from_raw)
        && wait_to_reap(pid)
    {
        // The child is a zombie, so `wait` returns at once. Hold the same
        // lock as signal delivery until the phase no longer permits signals.
        let mut screen = lock(&live.screen);
        mark_draining(&mut screen, live.id, child.wait());
        return;
    }

    // Fall back to checking under the signal lock if the OS exit wait is unavailable.
    loop {
        {
            let mut screen = lock(&live.screen);
            match child.try_wait() {
                Ok(None) => {}
                Ok(Some(status)) => {
                    mark_draining(&mut screen, live.id, Ok(status));
                    return;
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(error) => {
                    mark_draining(&mut screen, live.id, Err(error));
                    return;
                }
            }
        }

        thread::sleep(Duration::from_millis(25));
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn wait_to_reap(pid: rustix::process::Pid) -> bool {
    use rustix::process::{WaitId, WaitIdOptions, waitid};

    loop {
        match waitid(
            WaitId::Pid(pid),
            WaitIdOptions::EXITED | WaitIdOptions::NOWAIT,
        ) {
            Ok(Some(_)) => return true,
            Err(rustix::io::Errno::INTR) => {}
            _ => return false,
        }
    }
}

fn mark_draining(screen: &mut Screen, id: TaskId, status: io::Result<ExitStatus>) {
    let stopped = matches!(screen.phase, Phase::Stopping);
    let state = match status {
        _ if stopped => TaskState::Stopped,
        Ok(status) if status.signal().is_some() => TaskState::Exited { code: None },
        Ok(status) => TaskState::Exited {
            code: i32::try_from(status.exit_code()).ok(),
        },
        Err(e) => {
            log::warn!("could not wait for terminal {id}: {e}");
            TaskState::Exited { code: None }
        }
    };
    screen.phase = Phase::Draining(state);
}
