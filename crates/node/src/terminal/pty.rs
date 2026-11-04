//! Opening a pseudo-terminal, and the threads that read it, write to it
//! and wait for its child.

use super::{
    lock,
    screen::Screen,
    terminals::{Live, Term, Terminals},
};
use crate::provision::spawn_env::{self, Scope};
use anyhow::{Context, Result};
use beep_proto::node::{
    process::{TaskId, TaskState},
    terminal::Size,
};
use portable_pty::{Child, CommandBuilder, PtySize, native_pty_system};
use std::{
    io::{Read, Write},
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc as std_mpsc,
    },
    thread,
    time::Duration,
};
use tokio::sync::{mpsc, watch};

/// Typed input waiting for the terminal to read it; more is dropped.
const INPUT_QUEUE: usize = 64;
/// How long the exit waits for the last output to be read.
const DRAIN_WAIT: Duration = Duration::from_millis(500);
/// Bytes read from the pseudo-terminal at a time.
const READ_SIZE: usize = 8 * 1024;

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
impl Terminals {
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
        match &*lock(&self.env) {
            Some(env) => {
                for var in env {
                    builder.env(&var.name, var.value.expose());
                }
            }
            None => {
                for (name, value) in spawn_env::vars(Scope::Shell) {
                    builder.env(name, value);
                }
            }
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
        let mut screen = Screen::new();
        screen.push(banner.unwrap_or_default().as_bytes());
        let live = Arc::new(Live {
            id,
            command: name,
            screen: Mutex::new(screen),
            changed: watch::channel(0).0,
            input: input_tx,
            master: Mutex::new(pair.master),
            stopping: AtomicBool::new(false),
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
            killer,
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
    live.started_or_ended();
}
