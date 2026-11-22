//! Claude Code's CLI in stream-json mode: how it starts, the lines written to
//! its stdin and the frames read from its stdout.

use crate::agents::{launch, pipe::Pipe};
use crate::machine::wsl::{self, DistroName, LinuxPath};
use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::{io::Write, path::Path};
use tempfile::NamedTempFile;

/// What a CLI starts with.
pub(super) struct Launch<'a> {
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
    /// Tools the client switched off, as `claudeCode.options.disallowedTools`
    /// in a session's `_meta` names them, such as the home assistant's
    /// subagent tools.
    pub(super) disallowed: Vec<String>,
}

impl Launch<'_> {
    /// Its arguments. The MCP servers are read from `files.mcp` and the
    /// system prompt from `files.system`: the one holds secrets, the other
    /// can be longer than a Windows command line takes.
    /// `place` names a file as the CLI sees it.
    /// The user's arguments, `extra`, come last.
    fn args(&self, extra: Vec<String>, files: &Files, place: impl Fn(&Path) -> Result<String>) -> Result<Vec<String>> {
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

        // One list: the flag takes the names after it up to the next flag.
        if let Some(off) = off_tools(self.asks, &self.disallowed) {
            args.extend(["--disallowedTools".to_owned(), off]);
        }

        args.extend(extra);
        Ok(args)
    }
}

/// What `--disallowedTools` names: `disallowed`, and `AskUserQuestion`
/// when the client does not answer it. None when nothing is off.
pub(super) fn off_tools(asks: bool, disallowed: &[String]) -> Option<String> {
    let mut off: Vec<&str> = disallowed.iter().map(String::as_str).collect();

    if !asks {
        off.push("AskUserQuestion");
    }

    (!off.is_empty()).then(|| off.join(","))
}

/// The tools a session's `_meta` switches off, read leniently: anything but
/// a list of names is none.
pub(super) fn disallowed_tools(meta: Option<&serde_json::Map<String, Value>>) -> Vec<String> {
    meta.and_then(|meta| meta.get("claudeCode"))
        .and_then(|claude| claude["options"]["disallowedTools"].as_array())
        .map(|tools| tools.iter().filter_map(Value::as_str).map(str::to_owned).collect())
        .unwrap_or_default()
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
    pub(super) pipe: Pipe,
    next: u64,
    _files: Files,
}

impl Cli {
    pub(super) fn start(cli: &mut launch::Cli, launch: &Launch) -> Result<Self> {
        let files = Files {
            mcp: launch.mcp.as_deref().map(file).transpose()?,
            system: launch.system.as_deref().map(file).transpose()?,
        };
        let args = match launch.wsl {
            Some(_) => launch.args(cli.args(), &files, |path| Ok(wsl_path(path)?.to_string()))?,
            None => launch.args(cli.args(), &files, |path| Ok(path.display().to_string()))?,
        };
        let process = cli.command(launch.wsl, launch.cwd, &args)?;

        Ok(Self {
            pipe: Pipe::start(cli, process, "Claude Code", launch.wsl.is_some())?,
            next: 0,
            _files: files,
        })
    }

    /// Writes a user message of `content` blocks.
    pub(super) fn user(&self, content: Vec<Value>) {
        self.pipe.write(&json!({
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
        self.pipe.write(&json!({"type": "control_request", "request_id": id, "request": request}));
        id
    }

    /// Answers the CLI's control request `id`.
    pub(super) fn respond(&self, id: &str, response: Value) {
        self.pipe.write(&json!({
            "type": "control_response",
            "response": {"subtype": "success", "request_id": id, "response": response},
        }));
    }

    /// Refuses the CLI's control request `id`.
    pub(super) fn refuse(&self, id: &str, error: &str) {
        self.pipe.write(&json!({
            "type": "control_response",
            "response": {"subtype": "error", "request_id": id, "error": error},
        }));
    }
}
