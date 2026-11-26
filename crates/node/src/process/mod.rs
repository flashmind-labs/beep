//! Children that lead their own process group (`process`) and the
//! environment they start with (`spawn_env`). On Unix the group is a process
//! group; on Windows it is a job object, so stopping one stops everything it
//! started: the shell's commands, an agent's tools.

use spawn_env::Scope;
use std::{ffi::OsStr, process::{Output, Stdio}, time::Duration};
use tokio::{
    process::{Child, Command},
    time::timeout,
};

pub(crate) mod spawn_env;

#[cfg(unix)]
mod unix;
#[cfg(unix)]
use unix as imp;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows as imp;

pub(crate) use imp::{Group, Held};

/// The most of a CLI's output kept for a message.
const MAX_DETAIL: usize = 300;
/// How long a child has to exit after SIGTERM before the group is killed.
pub(crate) const STOP_GRACE: Duration = Duration::from_secs(2);

/// `program` as a command to start. Windows starts only `.exe` files by
/// bare name, so there it is looked up on the node's search path (`PATH`
/// and Beep's tool folders) with every extension
/// `PATHEXT` lists: npm installs tools such as `npx` and `claude` as `.cmd`
/// files.
pub(crate) fn command(program: impl AsRef<OsStr>) -> Command {
    let program = program.as_ref();
    #[cfg(windows)]
    if let Ok(found) = which::which_in(
        program,
        crate::process::spawn_env::search_path(),
        "/",
    ) {
        return Command::from(background_command(found));
    }

    Command::from(background_command(program))
}

/// `program` with `args`, the tools' folders on `PATH` and the GitHub token,
/// killed when dropped.
pub(crate) fn tool_command(program: &str, args: &[String]) -> Command {
    let mut command = command(program);
    command.args(args).stdin(Stdio::null()).kill_on_drop(true);
    spawn_env::apply(&mut command, Scope::Agent(None));
    command
}

/// The end of what a CLI said, on one line.
pub(crate) fn tail(text: &str) -> String {
    let text = text.trim().replace('\n', " ");
    let start = text
        .char_indices()
        .rev()
        .nth(MAX_DETAIL)
        .map_or(0, |(at, _)| at);
    text[start..].to_owned()
}

/// A background command, with no console window allocated on Windows.
/// Interactive terminals use their PTY's command builder instead.
pub(crate) fn background_command(program: impl AsRef<OsStr>) -> std::process::Command {
    counted(program.as_ref());
    let command = std::process::Command::new(program);
    #[cfg(windows)]
    let command = {
        use std::os::windows::process::CommandExt;
        let mut command = command;
        command.creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW);
        command
    };
    command
}

/// Programs the node starts often enough to watch. Anything else counts as
/// `other`, so a configured agent path cannot grow the metric's labels.
const COUNTED: [&str; 16] = [
    "git", "sh", "bash", "zsh", "pwsh", "powershell", "cmd", "ps", "lsof", "node", "npm",
    "npx", "claude", "codex", "systemctl", "launchctl",
];

/// Counts a program the node is about to start, by its bare name. This
/// counts attempts: `launched` and `finished` count how they went.
pub(crate) fn counted(program: &OsStr) {
    metrics::counter!("beep_spawn_total", "program" => label(program)).increment(1);
}

/// Counts how starting `program` went: `beep_spawn_started_total` when it
/// ran, else `beep_spawn_failed_total` by the kind of error.
pub(crate) fn launched<T>(program: impl AsRef<OsStr>, started: &std::io::Result<T>) {
    let program = label(program.as_ref());

    match started {
        Ok(_) => metrics::counter!("beep_spawn_started_total", "program" => program).increment(1),
        Err(e) => metrics::counter!(
            "beep_spawn_failed_total",
            "program" => program,
            "kind" => format!("{:?}", e.kind()),
        )
        .increment(1),
    }
}

/// `launched` for a command run to its end, also counting an exit that was
/// not a success in `beep_spawn_unsuccessful_total`.
pub(crate) fn finished(program: impl AsRef<OsStr>, output: &std::io::Result<Output>) {
    let program = program.as_ref();
    launched(program, output);

    if let Ok(output) = output
        && !output.status.success()
    {
        metrics::counter!("beep_spawn_unsuccessful_total", "program" => label(program))
            .increment(1);
    }
}

/// The metric label for `program`: its bare name when counted, else `other`.
fn label(program: &OsStr) -> &'static str {
    let name = std::path::Path::new(program)
        .file_stem()
        .and_then(OsStr::to_str)
        .map(str::to_ascii_lowercase);
    name.as_deref()
        .and_then(|name| COUNTED.iter().find(|known| **known == name))
        .copied()
        .unwrap_or("other")
}

/// Starts a background probe without opening a console on Windows.
pub(crate) fn probe(program: impl AsRef<OsStr>) -> Command {
    command(program)
}

/// Makes `command` start in a group of its own, for `GroupLeader`.
pub(crate) fn own_group(command: &mut Command) -> &mut Command {
    imp::own_group(command)
}

/// A child that leads its own group (spawned after `own_group`). `stop`
/// ends the group gently, then hard; dropping it without `stop` kills the
/// group at once, since `Drop` cannot wait.
pub(crate) struct GroupLeader {
    child: Child,
    group: Option<Group>,
}

impl GroupLeader {
    pub(crate) fn new(child: Child) -> Self {
        let group = match Group::of(&child) {
            Ok(group) => group,
            Err(e) => {
                log::warn!("could not group a child's processes: {e}");
                None
            }
        };
        Self { child, group }
    }

    /// The group, to kill it from elsewhere.
    pub(crate) fn group(&self) -> Option<Group> {
        self.group.clone()
    }

    pub(crate) fn id(&self) -> Option<u32> {
        self.child.id()
    }

    pub(crate) fn child_mut(&mut self) -> &mut Child {
        &mut self.child
    }

    /// Copies of the read ends of the child's stdout and stderr. Held while
    /// it stops, they keep a write on its way out landing in the pipe after
    /// the readers are gone, rather than killing it with SIGPIPE.
    pub(crate) fn hold_output(&self) -> Vec<Held> {
        imp::hold_output(&self.child)
    }

    /// Asks the group to end, waits up to `grace` for the child to exit,
    /// then kills whatever is left of the group and reaps the child, so no
    /// zombie remains. Windows has no gentle way: the group is killed at
    /// once.
    pub(crate) async fn stop(&mut self, grace: Duration) {
        if self.reaped() {
            return;
        }

        self.signal(Group::terminate);

        if timeout(grace, self.child.wait()).await.is_err() {
            log::debug!("the process group outlived SIGTERM; killing it");
            self.signal(Group::kill);

            if let Err(e) = self.child.wait().await {
                log::warn!("could not reap a stopped child: {e}");
            }

            return;
        }
        // The child exited, but something it started may still run. POSIX
        // does not reuse a process group's id while the group has members,
        // so this reaches what is left of the group or no one.
        if let Some(group) = &self.group {
            group.kill();
        }
    }

    /// The child was waited for, so its pid may name another process now.
    fn reaped(&self) -> bool {
        self.child.id().is_none()
    }

    /// Signals the group while its leader is not yet reaped: until then the
    /// leader, a zombie at worst, keeps the id from being reused.
    fn signal(&self, send: fn(&Group)) {
        if let Some(group) = &self.group
            && !self.reaped()
        {
            send(group);
        }
    }
}

impl Drop for GroupLeader {
    fn drop(&mut self) {
        // Only a signal, so it is fine here. After `stop` this sends nothing.
        self.signal(Group::kill);
    }
}

#[cfg(all(test, unix))]
#[path = "../tests/process.rs"]
mod tests;
#[cfg(all(test, windows))]
#[path = "../tests/process_windows_tests.rs"]
mod windows_tests;
