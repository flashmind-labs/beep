//! How the node starts an agent that speaks ACP: as a process of its own, or
//! on one of the node's bridges, a task inside the node that speaks ACP to
//! the session over an in-memory pipe and starts the agent's CLI itself.
//! The node never starts its own executable to run an agent.

use super::{Line, LineReader};
use crate::machine::wsl::{self, DistroName, LinuxPath};
use crate::run::process::{GroupLeader, Held, command, own_group};
use anyhow::{Context, Result};
use std::{
    ffi::OsStr,
    future::{Future, pending},
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::{
    io::{BufReader, DuplexStream},
    process::{ChildStdin, ChildStdout, Command},
    sync::{mpsc, oneshot},
    time::timeout,
};
use tokio_util::{either::Either, task::AbortOnDropHandle};

/// The longest stderr line read whole; a longer one is skipped.
const MAX_STDERR_LINE: usize = 64 * 1024;
/// How many stderr lines wait to be read.
pub(super) const STDERR_LINES: usize = 64;
/// How many bytes of ACP wait in the pipe between the session and a bridge.
const PIPE: usize = 256 * 1024;

/// Where the session writes the agent's ACP.
pub type AgentIn = Either<ChildStdin, DuplexStream>;
/// Where the session reads it.
pub type AgentOut = Either<ChildStdout, DuplexStream>;

/// A started agent.
pub struct Started {
    pub stdin: AgentIn,
    pub stdout: AgentOut,
    /// What the agent writes to stderr, a line each, and why a bridge ended
    /// when it failed.
    pub stderr: mpsc::Receiver<String>,
    pub running: Running,
}

/// A one-way pipe in memory: the first end writes, the second reads. Either
/// end dropped closes it, as a process's stdio closes.
pub(super) fn pipe() -> (DuplexStream, DuplexStream) {
    tokio::io::duplex(PIPE)
}

/// A started agent's process, or its bridge's task. Dropping it kills
/// them.
pub struct Running {
    /// The process started with the session.
    pub(super) process: Option<Process>,
    /// The bridge or relay.
    pub(super) task: Option<AbortOnDropHandle<()>>,
    /// The pid of the CLI a bridge starts, once it has.
    pub(super) started: Option<oneshot::Receiver<u32>>,
}

impl Running {
    /// The pid of the process started with the session.
    pub fn pid(&self) -> Option<u32> {
        self.process
            .as_ref()
            .and_then(|process| process.leader.id())
    }

    /// The pid of the CLI a bridge starts, when it starts it; never for a
    /// process started with the session.
    pub async fn started(&mut self) -> u32 {
        let Some(started) = &mut self.started else {
            return pending().await;
        };
        let pid = started.await;
        self.started = None;

        match pid {
            Ok(pid) => pid,
            Err(_) => pending().await,
        }
    }

    /// Waits up to `grace` for a bridge that ended on its own to finish, so
    /// what it said on its way out is read.
    pub async fn settle(&mut self, grace: Duration) {
        if let Some(task) = &mut self.task
            && timeout(grace, task).await.is_ok()
        {
            self.task = None;
        }
    }

    /// Ends the agent once the session's side of its ACP closed: its
    /// process's group gently, then hard after `grace`. A bridge stops its
    /// CLI the same way when the session's side closes, so it gets twice
    /// that before it is aborted, which kills the CLI's group at once.
    pub async fn stop(&mut self, grace: Duration) {
        if let Some(process) = self.process.take() {
            process.stop(grace).await;
        }

        if let Some(mut task) = self.task.take()
            && timeout(grace * 2, &mut task).await.is_err()
        {
            log::debug!("the agent's bridge outlived its stop; aborting it");
        }
    }
}

/// A process the node started in its own group, with its stderr read a
/// line at a time. Dropping it kills the group.
pub(super) struct Process {
    leader: GroupLeader,
    held: Vec<Held>,
    _stderr: AbortOnDropHandle<()>,
}

impl Process {
    pub(super) fn id(&self) -> Option<u32> {
        self.leader.id()
    }

    /// Asks the group to end, then kills what is left of it after `grace`.
    pub(super) async fn stop(mut self, grace: Duration) {
        self.leader.stop(grace).await;
        drop(self.held);
    }
}

/// Starts `process`, the agent `name`, in its own group, with its stdio
/// piped: its stderr goes to `lines`, a line each.
pub(super) fn spawn(
    mut process: Command,
    name: &str,
    lines: mpsc::Sender<String>,
) -> Result<(Process, ChildStdin, ChildStdout)> {
    let program = process
        .as_std()
        .get_program()
        .to_string_lossy()
        .into_owned();
    own_group(&mut process)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let child = process
        .spawn()
        .with_context(|| format!("starting {program}"))?;
    let mut leader = GroupLeader::new(child);
    let held = leader.hold_output();
    let child = leader.child_mut();
    let stdin = child
        .stdin
        .take()
        .with_context(|| format!("{name} has no stdin"))?;
    let stdout = child
        .stdout
        .take()
        .with_context(|| format!("{name} has no stdout"))?;
    let stderr = child
        .stderr
        .take()
        .with_context(|| format!("{name} has no stderr"))?;
    let forward = async move {
        let mut stderr = LineReader::new(BufReader::new(stderr), MAX_STDERR_LINE);

        // Read to the end, dropping what no one reads in time, as the model
        // probe reads none: a full pipe would stop the agent.
        while let Ok(Some(line)) = stderr.next().await {
            if let Line::Text(line) = line {
                let _ = lines.try_send(line);
            }
        }
    };
    let process = Process {
        leader,
        held,
        _stderr: AbortOnDropHandle::new(tokio::spawn(forward)),
    };

    Ok((process, stdin, stdout))
}

/// The agent's CLI as the session set it up: its program, the user's
/// arguments, its environment and its folder. A bridge starts it with
/// arguments of its own.
pub(super) struct Cli {
    template: Command,
    /// Where its stderr goes, a line each.
    lines: mpsc::Sender<String>,
    /// Where the session hears its pid once it started.
    pid: Option<oneshot::Sender<u32>>,
}

impl Cli {
    pub(super) fn new(
        template: Command,
        lines: mpsc::Sender<String>,
        pid: Option<oneshot::Sender<u32>>,
    ) -> Self {
        Self {
            template,
            lines,
            pid,
        }
    }

    pub(super) fn program(&self) -> String {
        self.template
            .as_std()
            .get_program()
            .to_string_lossy()
            .into_owned()
    }

    /// The user's arguments.
    pub(super) fn args(&self) -> Vec<String> {
        self.template
            .as_std()
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect()
    }

    pub(super) fn cwd(&self) -> Result<PathBuf> {
        self.template
            .as_std()
            .get_current_dir()
            .map(Path::to_path_buf)
            .context("the agent has no folder")
    }

    /// The variable `name` as the CLI sees it: set or removed for it, else
    /// the node's own.
    pub(super) fn var(&self, name: &str) -> Option<String> {
        let set = self
            .template
            .as_std()
            .get_envs()
            .find(|(var, _)| *var == OsStr::new(name));

        match set {
            Some((_, value)) => value.map(|value| value.to_string_lossy().into_owned()),
            None => std::env::var(name).ok(),
        }
    }

    /// The CLI with `args` in `cwd`, inside `wsl` when given, with its
    /// environment. In WSL it reads the variables both the session's and
    /// the node's `WSLENV` name.
    pub(super) fn command(
        &self,
        wsl: Option<&DistroName>,
        cwd: &Path,
        args: &[String],
    ) -> Result<Command> {
        let mut process = match wsl {
            Some(distro) => {
                let program: LinuxPath = self.program().parse()?;
                let inside = wsl_path(cwd)?;
                wsl::agent_command(distro, Some(&inside), &program, args)
            }
            None => {
                let mut process = command(self.program());
                process.args(args);
                process
            }
        };
        let node_names = env_of(&process, "WSLENV");

        for (name, value) in self.template.as_std().get_envs() {
            match value {
                Some(value) => process.env(name, value),
                None => process.env_remove(name),
            };
        }

        if wsl.is_some() {
            let session = env_of(&self.template, "WSLENV");
            let names = node_names.as_deref().unwrap_or_default().split(':');
            process.env("WSLENV", wsl::joined_wslenv(session.as_deref(), names));
        }

        process.current_dir(cwd);
        Ok(process)
    }

    /// Starts `process`, the agent `name`, in its own group; the session
    /// hears its pid.
    pub(super) fn spawn(
        &mut self,
        process: Command,
        name: &str,
    ) -> Result<(Process, ChildStdin, ChildStdout)> {
        let spawned = spawn(process, name, self.lines.clone())?;

        if let (Some(pid), Some(id)) = (self.pid.take(), spawned.0.id()) {
            let _ = pid.send(id);
        }

        Ok(spawned)
    }
}

/// `path` on Windows as WSL sees it.
pub(in crate::agents) fn wsl_path(path: &Path) -> Result<LinuxPath> {
    wsl::linux_path(path).with_context(|| {
        format!(
            "WSL does not reach {}, which is on no drive",
            path.display()
        )
    })
}

/// The variable `name` set on `process`.
fn env_of(process: &Command, name: &str) -> Option<String> {
    process
        .as_std()
        .get_envs()
        .find(|(var, _)| *var == OsStr::new(name))
        .and_then(|(_, value)| value)
        .map(|value| value.to_string_lossy().into_owned())
}
