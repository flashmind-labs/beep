//! `shell/1` sessions: one bash per stream, kept between commands so `cd`
//! and `export` carry over.

use crate::process::{GroupLeader, STOP_GRACE};
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
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{ChildStdin, ChildStdout, Command},
    time::timeout,
};

pub use beep_proto::node::shell::PROTOCOL;

/// The most output one command sends; the rest is dropped.
const MAX_OUTPUT: usize = 1024 * 1024;
/// Output is sent in pieces of about this size...
const BATCH: usize = 8 * 1024;
/// ...or this often, whichever comes first.
const FLUSH: Duration = Duration::from_millis(200);
const MAX_TIMEOUT_SECS: u32 = 60 * 60;
/// The most requests kept while a command runs.
const MAX_WAITING: usize = 4;

/// Serves one session stream until the server closes it.
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
                        // The server closed the stream, which cancels the
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
        let done = match end {
            End::Finished { exit_code, cwd } => Event::Done {
                exit_code: Some(exit_code),
                timed_out: false,
                cwd: Some(cwd),
                fresh,
                truncated,
            },
            End::ShellEnded | End::TimedOut => {
                // Stops the shell and everything it started.
                stop(&mut shell).await;
                Event::Done {
                    exit_code: None,
                    timed_out: matches!(end, End::TimedOut),
                    cwd: None,
                    fresh,
                    truncated,
                }
            }
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
    Finished { exit_code: i32, cwd: String },
    ShellEnded,
    TimedOut,
}

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
        let program = if which::which("bash").is_ok() {
            "bash"
        } else {
            "sh"
        };
        let mut command = Command::new(program);
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            // Its own process group, so stopping it stops what it started.
            .process_group(0)
            .kill_on_drop(true);
        if let Some(home) = dirs::home_dir() {
            command.current_dir(home);
        }
        let mut child = command
            .spawn()
            .with_context(|| format!("starting {program}"))?;
        let mut stdin = child.stdin.take().context("the shell has no stdin")?;
        let stdout = child.stdout.take().context("the shell has no stdout")?;
        stdin.write_all(b"exec 2>&1\n").await?;
        Ok(Self {
            child: GroupLeader::new(child),
            stdin,
            stdout: BufReader::new(stdout),
            mark: mark(),
        })
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
            if self.stdout.read_until(b'\n', &mut line).await? == 0 {
                out.push(&held).await?;
                return Ok(End::ShellEnded);
            }
            let text = String::from_utf8_lossy(&line);
            if let Some((exit_code, cwd)) = parse_mark(&text, &self.mark) {
                out.push(held.strip_suffix('\n').unwrap_or(&held)).await?;
                return Ok(End::Finished { exit_code, cwd });
            }
            out.push(&held).await?;
            held = text.into_owned();
        }
    }
}

/// What the shell is sent for one command. `eval` keeps a syntax error in
/// `command` from leaving the shell waiting for more input, and stdin is
/// empty so a command that reads it does not eat the next one.
fn script(command: &str, mark: &str) -> String {
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
mod tests {
    use super::*;

    #[test]
    fn the_mark_line_gives_the_exit_code_and_folder() {
        let mark = "__m__";
        assert_eq!(
            parse_mark("__m__ 2 /srv/my project\n", mark),
            Some((2, "/srv/my project".to_owned()))
        );
        assert_eq!(parse_mark("__m__x 2 /srv\n", mark), None);
        assert_eq!(parse_mark("hello\n", mark), None);
    }

    impl Sink for Vec<String> {
        async fn send_text(&mut self, text: String) -> Result<()> {
            self.push(text);
            Ok(())
        }
    }

    async fn run(shell: &mut Shell, command: &str) -> (String, End) {
        let mut sent = Vec::new();
        let mut out = Output::new(&mut sent);
        let end = shell.run(command, &mut out).await.unwrap();
        out.finish().await.unwrap();
        (sent.concat(), end)
    }

    #[tokio::test]
    async fn the_shell_keeps_its_folder_and_variables() {
        let mut shell = Shell::start().await.unwrap();
        let (out, end) = run(&mut shell, "cd / && export GREETING=hi && echo 'it''s set'").await;
        assert_eq!(out, "its set\n");
        assert!(matches!(end, End::Finished { exit_code: 0, ref cwd } if cwd == "/"));
        let (out, _) = run(&mut shell, "printf %s \"$GREETING\"; ls -d nowhere").await;
        assert!(out.starts_with("hi"), "{out}");
        assert!(out.contains("nowhere"), "stderr is merged: {out}");
        let (_, end) = run(&mut shell, "cat; false").await;
        assert!(
            matches!(end, End::Finished { exit_code: 1, .. }),
            "stdin is empty"
        );
        let (out, end) = run(&mut shell, "echo 'unclosed").await;
        assert!(
            matches!(end, End::Finished { exit_code, .. } if exit_code != 0),
            "a syntax error fails the command, not the shell: {out}"
        );
        let (_, end) = run(&mut shell, "exit 3").await;
        assert!(matches!(end, End::ShellEnded));
    }

    #[test]
    fn quotes_in_a_command_survive_eval() {
        let script = script("echo 'a b'", "__m__");
        assert!(script.starts_with("{ eval 'echo '\\''a b'\\'''\n} < /dev/null\n"));
    }
}
