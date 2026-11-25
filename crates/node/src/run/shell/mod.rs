//! `shell/1` sessions: one bash per stream (PowerShell on Windows), kept
//! between commands so `cd` and `export` carry over.

use crate::{
    machine::spawn_env::{self, Scope},
    run::lines::read_piece,
    run::process::{GroupLeader, STOP_GRACE, own_group},
};
use anyhow::{Context, Result};
use beep_proto::node::{
    LinkRx, LinkTx,
    shell::{Event, Request},
};
use std::{
    collections::VecDeque,
    process::Stdio,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokio::{
    io::{AsyncWriteExt, BufReader},
    process::{ChildStdin, ChildStdout, Command},
    time::timeout,
};

pub use beep_proto::node::shell::PROTOCOL;

#[cfg(any(windows, test))]
#[cfg_attr(not(windows), allow(dead_code))]
mod powershell;

/// The most output one command sends; the rest is dropped.
const MAX_OUTPUT: usize = 1024 * 1024;
/// Output is sent in pieces of about this size...
const BATCH: usize = 8 * 1024;
/// ...or this often, whichever comes first.
const FLUSH: Duration = Duration::from_millis(200);
const MAX_TIMEOUT_SECS: u32 = 60 * 60;
/// The most requests kept while a command runs.
const MAX_WAITING: usize = 4;

/// Serves one session stream until the caller closes it.
pub async fn serve(mut tx: LinkTx, mut rx: LinkRx) -> Result<()> {
    let mut shell: Option<Shell> = None;
    // Requests that came while a command ran.
    let mut waiting: VecDeque<Request> = VecDeque::new();

    loop {
        let request = match waiting.pop_front() {
            Some(request) => request,
            None => match rx.recv::<Request>().await? {
                Some(request) => request,
                None => {
                    stop(&mut shell).await;
                    return Ok(());
                }
            },
        };
        let Request::Run {
            command,
            timeout_secs,
        } = request
        else {
            continue;
        };
        let fresh = shell.is_none();
        let current = match &mut shell {
            Some(current) => current,
            None => shell.insert(Shell::start().await?),
        };
        let limit = Duration::from_secs(timeout_secs.clamp(1, MAX_TIMEOUT_SECS).into());
        let mut out = Output::new(&mut tx);
        let end = {
            let run = timeout(limit, current.run(&command, &mut out));
            tokio::pin!(run);

            loop {
                tokio::select! {
                    ended = &mut run => break Some(match ended {
                        Ok(end) => end?,
                        Err(_) => End::TimedOut,
                    }),
                    next = rx.recv::<Request>() => match next {
                        Ok(Some(request)) if waiting.len() < MAX_WAITING => waiting.push_back(request),
                        Ok(Some(_)) => log::debug!("dropped a request: {MAX_WAITING} already wait"),
                        // The caller closed the stream, which cancels the
                        // command and stops the shell's group.
                        Ok(None) | Err(_) => break None,
                    },
                }
            }
        };
        let Some(end) = end else {
            stop(&mut shell).await;
            return Ok(());
        };
        let truncated = out.finish().await?;
        // A shell that ended or timed out is stopped, with what it started.
        let (exit_code, timed_out, cwd, stop_shell) = match end {
            End::Finished { exit_code, cwd } => (Some(exit_code), false, Some(cwd), false),
            End::ShellEnded { exit_code } => (exit_code, false, None, true),
            End::TimedOut => (None, true, None, true),
        };

        if stop_shell {
            stop(&mut shell).await;
        }

        let done = Event::Done {
            exit_code,
            timed_out,
            cwd,
            fresh,
            truncated,
        };
        tx.send(&done).await?;
    }
}

/// Stops the shell, if one runs, and what it started: gently, then hard.
async fn stop(shell: &mut Option<Shell>) {
    if let Some(mut old) = shell.take() {
        old.child.stop(STOP_GRACE).await;
    }
}

enum End {
    Finished {
        exit_code: i32,
        cwd: String,
    },
    /// The command ended the shell, such as with `exit`.
    ShellEnded {
        exit_code: Option<i32>,
    },
    TimedOut,
}

/// How long a shell whose output ended gets to exit, for its exit code.
const SHELL_EXIT: Duration = Duration::from_secs(2);

struct Shell {
    /// Dropping it kills the shell's group; `stop` ends it gently first.
    child: GroupLeader,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    /// Ends each command's output. Unguessable, so output cannot fake it.
    mark: String,
}

impl Shell {
    async fn start() -> Result<Self> {
        let program = program();
        crate::run::process::counted(program.as_ref());
        let mut command = Command::new(program);
        #[cfg(windows)]
        command.args(powershell::session_args());
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        // Its own process group, so stopping it stops what it started.
        own_group(&mut command);

        if let Some(home) = dirs::home_dir() {
            command.current_dir(home);
        }

        spawn_env::apply(&mut command, Scope::Shell);
        let mut child = command
            .spawn()
            .with_context(|| format!("starting {program}"))?;
        let mut stdin = child.stdin.take().context("the shell has no stdin")?;
        let stdout = child.stdout.take().context("the shell has no stdout")?;
        stdin.write_all(setup().as_bytes()).await?;
        Ok(Self {
            child: GroupLeader::new(child),
            stdin,
            stdout: BufReader::new(stdout),
            mark: mark(),
        })
    }

    /// The shell's own exit code once its output ended, as `exit 2` in a
    /// command leaves it; `None` when it does not exit soon or a signal
    /// ended it.
    async fn exit_code(&mut self) -> Option<i32> {
        let status = timeout(SHELL_EXIT, self.child.child_mut().wait()).await;
        status.ok()?.ok()?.code()
    }

    async fn run<S: Sink>(&mut self, command: &str, out: &mut Output<'_, S>) -> Result<End> {
        self.stdin
            .write_all(script(command, &self.mark).as_bytes())
            .await?;
        self.stdin.flush().await?;
        // Each line is held back one step: the one before the mark ends with
        // the newline the mark's printf added, which is not output.
        let mut held = String::new();
        let mut line = Vec::new();

        loop {
            line.clear();

            if read_piece(&mut self.stdout, &mut line).await? == 0 {
                out.push(&held).await?;
                return Ok(End::ShellEnded {
                    exit_code: self.exit_code().await,
                });
            }

            let text = String::from_utf8_lossy(&line);
            // PowerShell ends its lines with CRLF.
            let text = match text.strip_suffix("\r\n") {
                Some(bare) if cfg!(windows) => format!("{bare}\n").into(),
                _ => text,
            };

            if let Some((exit_code, cwd)) = parse_mark(&text, &self.mark) {
                out.push(held.strip_suffix('\n').unwrap_or(&held)).await?;
                return Ok(End::Finished { exit_code, cwd });
            }

            out.push(&held).await?;
            held = text.into_owned();
        }
    }
}

/// The shell that runs commands: bash, else sh; PowerShell on Windows.
#[cfg(not(windows))]
fn program() -> &'static str {
    if which::which("bash").is_ok() {
        "bash"
    } else {
        "sh"
    }
}

#[cfg(windows)]
fn program() -> &'static str {
    powershell::program()
}

/// The command that runs `script` once in the shell, stderr merged into
/// stdout, and the shell's name.
pub(crate) fn one_shot(script: &str) -> (&'static str, Command) {
    let program = program();
    crate::run::process::counted(program.as_ref());
    let mut command = Command::new(program);
    #[cfg(windows)]
    command
        .args(powershell::one_shot_args())
        .env(powershell::SCRIPT_VAR, script);
    #[cfg(not(windows))]
    command.arg("-c").arg(format!("exec 2>&1\n{script}"));
    (program, command)
}

/// What a new shell is sent first.
fn setup() -> String {
    #[cfg(windows)]
    return powershell::session_setup();
    #[cfg(not(windows))]
    "exec 2>&1\n".to_owned()
}

/// What the shell is sent for one command.
fn script(command: &str, mark: &str) -> String {
    #[cfg(windows)]
    return powershell::script(command, mark);
    #[cfg(not(windows))]
    sh_script(command, mark)
}

#[cfg_attr(windows, allow(dead_code))]
/// What sh is sent for one command. `eval` keeps a syntax error in
/// `command` from leaving the shell waiting for more input, and stdin is
/// empty so a command that reads it does not eat the next one.
fn sh_script(command: &str, mark: &str) -> String {
    let quoted = command.replace('\'', r"'\''");
    format!(
        "{{ eval '{quoted}'\n}} < /dev/null\nprintf '\\n%s %d %s\\n' '{mark}' \"$?\" \"$PWD\"\n"
    )
}

/// The exit code and folder in the line the mark's printf wrote.
fn parse_mark(line: &str, mark: &str) -> Option<(i32, String)> {
    let rest = line.strip_prefix(mark)?.strip_prefix(' ')?;
    let (code, cwd) = rest.trim_end_matches('\n').split_once(' ')?;
    Some((code.parse().ok()?, cwd.to_owned()))
}

fn mark() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos());
    format!("__beep_done_{nanos:x}_{:x}__", std::process::id())
}

/// Where a command's output goes: the session stream, or a list in tests.
trait Sink {
    async fn send_text(&mut self, text: String) -> Result<()>;
}

impl Sink for LinkTx {
    async fn send_text(&mut self, text: String) -> Result<()> {
        self.send(&Event::Output { text }).await
    }
}

/// Sends a command's output in batches, up to `MAX_OUTPUT`.
struct Output<'a, S> {
    tx: &'a mut S,
    batch: String,
    sent: usize,
    truncated: bool,
    flushed: Instant,
}

impl<'a, S: Sink> Output<'a, S> {
    fn new(tx: &'a mut S) -> Self {
        Self {
            tx,
            batch: String::new(),
            sent: 0,
            truncated: false,
            flushed: Instant::now(),
        }
    }

    async fn push(&mut self, text: &str) -> Result<()> {
        let room = MAX_OUTPUT.saturating_sub(self.sent + self.batch.len());
        if text.len() > room {
            self.truncated = true;
            let mut end = room;

            while !text.is_char_boundary(end) {
                end -= 1;
            }

            self.batch.push_str(&text[..end]);
        } else {
            self.batch.push_str(text);
        }

        if self.batch.len() >= BATCH || self.flushed.elapsed() >= FLUSH {
            self.flush().await?;
        }

        Ok(())
    }

    async fn flush(&mut self) -> Result<()> {
        self.flushed = Instant::now();

        if self.batch.is_empty() {
            return Ok(());
        }

        let text = std::mem::take(&mut self.batch);
        self.sent += text.len();
        self.tx.send_text(text).await
    }

    /// Sends what is left and says whether any output was dropped.
    async fn finish(mut self) -> Result<bool> {
        self.flush().await?;
        Ok(self.truncated)
    }
}

#[cfg(test)]
#[path = "../../tests/run_shell.rs"]
mod tests;
