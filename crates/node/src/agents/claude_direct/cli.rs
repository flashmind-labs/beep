//! Claude Code's CLI in stream-json mode: how it starts, the lines written to
//! its stdin and the frames read from its stdout.

use crate::agents::{Line, LineReader, acp::MAX_MESSAGE};
use crate::{
    machine::wsl::{self, DistroName, LinuxPath, Toward},
    run::process::command,
};
use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::{io::Write, path::Path, process::Stdio};
use tempfile::NamedTempFile;
use tokio::{
    io::{AsyncWriteExt, BufReader},
    process::Child,
    sync::mpsc,
};
use tokio_util::task::AbortOnDropHandle;

/// How many frames wait to be handled before the reader stops reading.
const FRAMES: usize = 256;

/// What a CLI starts with.
pub(super) struct Launch<'a> {
    pub(super) program: &'a str,
    /// The user's arguments, added last.
    pub(super) extra: &'a [String],
    pub(super) cwd: &'a Path,
    /// The WSL distribution it runs in, on a Windows machine.
    pub(super) wsl: Option<&'a DistroName>,
    /// The session it starts as, or resumes.
    pub(super) session: &'a str,
    pub(super) resume: bool,
    /// The MCP servers, as `--mcp-config` takes them. It holds their
    /// secrets, so it goes in a file only the user reads, not in argv.
    pub(super) mcp: Option<String>,
    /// Appended to its system prompt.
    pub(super) system: Option<String>,
    /// The client answers `AskUserQuestion`; without it the tool is off.
    pub(super) asks: bool,
}

impl Launch<'_> {
    /// Its arguments. The MCP servers are read from `files.mcp` and the
    /// system prompt from `files.system`: the one holds secrets, the other
    /// can be longer than a Windows command line takes.
    /// `place` names a file as the CLI sees it.
    fn args(&self, files: &Files, place: impl Fn(&Path) -> Result<String>) -> Result<Vec<String>> {
        let mut args: Vec<String> = [
            "--print",
            "--input-format",
            "stream-json",
            "--output-format",
            "stream-json",
            "--verbose",
            "--include-partial-messages",
            "--replay-user-messages",
            // Permission prompts, `AskUserQuestion` among them, come as
            // `can_use_tool` control requests.
            "--permission-prompt-tool",
            "stdio",
            "--permission-mode",
            "default",
            // So the session can switch to `bypassPermissions`.
            "--allow-dangerously-skip-permissions",
        ]
        .map(str::to_owned)
        .into();
        let session = if self.resume {
            "--resume"
        } else {
            "--session-id"
        };
        args.extend([session.to_owned(), self.session.to_owned()]);

        if let Some(mcp) = &files.mcp {
            args.extend(["--mcp-config".to_owned(), place(mcp.path())?]);
        }

        if let Some(system) = &files.system {
            args.extend([
                "--append-system-prompt-file".to_owned(),
                place(system.path())?,
            ]);
        }

        if !self.asks {
            args.extend(["--disallowedTools".to_owned(), "AskUserQuestion".to_owned()]);
        }

        args.extend(self.extra.iter().cloned());
        Ok(args)
    }
}

/// The files a CLI reads its settings from, removed when it is dropped.
struct Files {
    mcp: Option<NamedTempFile>,
    system: Option<NamedTempFile>,
}

/// `text` in a new file only the user can read (`tempfile` makes it 0600).
fn file(text: &str) -> Result<NamedTempFile> {
    let mut file = NamedTempFile::new().context("writing Claude Code's settings")?;
    file.write_all(text.as_bytes())
        .context("writing Claude Code's settings")?;
    Ok(file)
}

/// `path` on Windows as WSL sees it.
fn wsl_path(path: &Path) -> Result<LinuxPath> {
    wsl::linux_path(path)
        .with_context(|| format!("WSL does not reach {}, which is on no drive", path.display()))
}

/// A running CLI. Dropping it kills it.
pub(super) struct Cli {
    lines: mpsc::UnboundedSender<String>,
    /// It runs in WSL: the paths in what it is sent and what it writes are
    /// turned across.
    in_wsl: bool,
    /// Its stdout, one JSON frame each; closed when it exits.
    pub(super) frames: mpsc::Receiver<Value>,
    next: u64,
    _child: Child,
    _files: Files,
    _tasks: [AbortOnDropHandle<()>; 2],
}

impl Cli {
    pub(super) fn start(launch: &Launch) -> Result<Self> {
        let files = Files {
            mcp: launch.mcp.as_deref().map(file).transpose()?,
            system: launch.system.as_deref().map(file).transpose()?,
        };
        let mut process = match launch.wsl {
            Some(distro) => {
                let args = launch.args(&files, |path| Ok(wsl_path(path)?.to_string()))?;
                let program: LinuxPath = launch.program.parse()?;
                wsl::agent_command(distro, Some(&wsl_path(launch.cwd)?), &program, &args)
            }
            None => {
                let mut process = command(launch.program);
                process.args(launch.args(&files, |path| Ok(path.display().to_string()))?);
                process
            }
        };
        process
            .current_dir(launch.cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            // The node reads it as the agent's.
            .stderr(Stdio::inherit())
            .kill_on_drop(true);
        let mut child = process
            .spawn()
            .with_context(|| format!("starting {}", launch.program))?;
        let mut stdin = child.stdin.take().context("Claude Code has no stdin")?;
        let stdout = child.stdout.take().context("Claude Code has no stdout")?;
        let (lines, mut written) = mpsc::unbounded_channel::<String>();
        let (sent, frames) = mpsc::channel(FRAMES);

        let writer = tokio::spawn(async move {
            while let Some(line) = written.recv().await {
                let wrote = async {
                    stdin.write_all(line.as_bytes()).await?;
                    stdin.write_all(b"\n").await?;
                    stdin.flush().await
                };
                if let Err(e) = wrote.await {
                    log::warn!("could not write to Claude Code: {e}");
                    return;
                }
            }
        });
        let in_wsl = launch.wsl.is_some();
        let reader = tokio::spawn(async move {
            let mut lines = LineReader::new(BufReader::new(stdout), MAX_MESSAGE);

            while let Ok(Some(line)) = lines.next().await {
                let text = match line {
                    Line::Text(text) => text,
                    Line::TooLong(length) => {
                        log::warn!("dropped a {length} byte line from Claude Code");
                        continue;
                    }
                };

                match serde_json::from_str::<Value>(&text) {
                    Ok(mut frame) => {
                        if in_wsl {
                            wsl::translate(&mut frame, Toward::Windows);
                        }

                        if sent.send(frame).await.is_err() {
                            return;
                        }
                    }
                    Err(_) if text.trim().is_empty() => {}
                    Err(_) => log::debug!("Claude Code wrote: {}", crate::agents::cut(&text, 200)),
                }
            }
        });

        Ok(Self {
            lines,
            in_wsl,
            frames,
            next: 0,
            _child: child,
            _files: files,
            _tasks: [AbortOnDropHandle::new(writer), AbortOnDropHandle::new(reader)],
        })
    }

    /// Writes a user message of `content` blocks.
    pub(super) fn user(&self, content: Vec<Value>) {
        self.write(&json!({
            "type": "user",
            "message": {"role": "user", "content": content},
            "parent_tool_use_id": null,
        }));
    }

    /// Sends the control request `request`; gives its id, which its answer
    /// carries.
    pub(super) fn control(&mut self, request: Value) -> String {
        self.next += 1;
        let id = format!("beep-{}", self.next);
        self.write(&json!({"type": "control_request", "request_id": id, "request": request}));
        id
    }

    /// Answers the CLI's control request `id`.
    pub(super) fn respond(&self, id: &str, response: Value) {
        self.write(&json!({
            "type": "control_response",
            "response": {"subtype": "success", "request_id": id, "response": response},
        }));
    }

    /// Refuses the CLI's control request `id`.
    pub(super) fn refuse(&self, id: &str, error: &str) {
        self.write(&json!({
            "type": "control_response",
            "response": {"subtype": "error", "request_id": id, "error": error},
        }));
    }

    /// A closed stdin shows as the CLI's exit on stdout.
    fn write(&self, line: &Value) {
        let line = match self.in_wsl {
            true => {
                let mut line = line.clone();
                wsl::translate(&mut line, Toward::Linux);
                line.to_string()
            }
            false => line.to_string(),
        };
        let _ = self.lines.send(line);
    }
}
