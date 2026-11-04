//! `exec`: a shell command on one of the user's machines, in the foreground or background.

use super::{Machines, Need, command_name, in_parameter};
use crate::{
    nodes::{Exited, NODE_BACK, Repeat},
    permissions::shorten,
    protocol::node::process::{MAX_LINES, Reply, Request},
};
use async_trait::async_trait;
use flashmind::types::tool::{Tool, ToolContext, ToolResult};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{collections::VecDeque, time::Duration};
use tokio::time::Instant;

pub(super) const NAME: &str = "exec";
/// What every command's step says, so the command itself stays out of the chat.
pub(super) const STEP: &str = "Running code";

/// The longest `timeout_secs` the model may set.
const MAX_TIMEOUT_SECS: u64 = 1800;
/// How long a command runs when the model sets no timeout.
const DEFAULT_LIMIT: Duration = Duration::from_secs(60 * 60);
/// The most lines `head` and `tail` keep together.
const MAX_PREVIEW_LINES: usize = 1000;

const DESCRIPTION: &str = "Execute a shell command using bash, or sh if bash is unavailable. \
    Set run_in_background=true for servers, watchers, or other long-running commands. Background \
    execution returns a process ID immediately; use process to read output or stop it. Run the \
    command directly, without nohup, setsid, or a trailing &. Background tasks survive turn \
    cancellation and keep running on the machine until they end or are stopped. Starting a task \
    does not confirm readiness: check its output and verify the service before reporting success. \
    Foreground commands return stdout and stderr with the exit code and stop after an hour \
    unless timeout_secs sets a shorter limit (max 1800). Stdin is closed: use noninteractive \
    flags, pipes, or input redirection for commands that need input. Set head and/or tail to \
    limit returned output (at most 1000 lines total) instead of piping through head or tail. \
    Commands run on the user's own Linux environment unless `in` names another of their \
    machines, such as their Mac. They start in the user's home folder there, so do not prefix \
    them with cd to it. Each command runs in a fresh shell: `cd` and `export` do not carry over. \
    The environment has network access, curl and wget, Python with numpy, pandas, matplotlib, \
    openpyxl, pypdf and pdfplumber, and pdftotext; `pip install` and `sudo apt-get install -y` \
    work there. Files the user sends and files apps download are saved in its home folder, and \
    send_file and send_image take paths there.";

pub(crate) struct ExecTool {
    pub(crate) machines: Machines,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct Args {
    command: String,
    #[serde(default)]
    timeout_secs: Option<u64>,
    #[serde(default)]
    head: Option<usize>,
    #[serde(default)]
    tail: Option<usize>,
    #[serde(default)]
    run_in_background: bool,
    #[serde(default, rename = "in")]
    machine: Option<String>,
}

impl Args {
    fn limit(&self) -> Duration {
        self.timeout_secs.map_or(DEFAULT_LIMIT, |secs| {
            Duration::from_secs(secs.clamp(1, MAX_TIMEOUT_SECS))
        })
    }

    /// Head and tail within their shared cap, `None` for all the output.
    fn preview(&self) -> Option<Preview> {
        if self.head.is_none() && self.tail.is_none() {
            return None;
        }
        let head = self.head.unwrap_or(0).min(MAX_PREVIEW_LINES);
        let tail = self
            .tail
            .unwrap_or(0)
            .min(MAX_PREVIEW_LINES.saturating_sub(head));
        Some(Preview::new(head, tail))
    }
}

#[async_trait]
impl Tool for ExecTool {
    fn name(&self) -> &str {
        NAME
    }
    fn description(&self) -> &str {
        DESCRIPTION
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "command": { "type": "string", "description": "The shell command to execute" },
                "timeout_secs": {
                    "type": "integer",
                    "description": "Optional limit in seconds (max 1800). Omit to allow up to an hour."
                },
                "tail": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 1000,
                    "description": "Return the last N combined stdout/stderr lines, plus any requested head and the exit code. Head and tail may total up to 1000 lines."
                },
                "head": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 1000,
                    "description": "Return the first N combined stdout/stderr lines, plus any requested tail and the exit code. The command still runs to completion."
                },
                "run_in_background": {
                    "type": "boolean",
                    "description": "Start a managed background task and return immediately. Defaults to false."
                },
                "in": in_parameter("Omit for the user's own Linux environment.")
            },
            "required": ["command"],
            "additionalProperties": false
        })
    }
    fn max_output_bytes(&self) -> usize {
        128 * 1024
    }
    fn max_output_lines(&self) -> usize {
        1002
    }
    fn timeout_secs(&self) -> Option<u64> {
        Some(DEFAULT_LIMIT.as_secs() + 60)
    }
    fn humanize(&self, args: &Value) -> String {
        let named = args.get("in").and_then(Value::as_str).map(command_name);
        let machine = named.or_else(|| {
            let workspace = self.machines.app.nodes.workspace(self.machines.user.id())?;
            Some(command_name(&workspace.name))
        });
        step(args, machine.as_deref())
    }
    async fn execute(&self, ctx: ToolContext<'_>) -> anyhow::Result<ToolResult> {
        let args: Args = match serde_json::from_value(ctx.args.clone()) {
            Ok(args) => args,
            Err(e) => return Ok(ToolResult::failure(ctx.tool_call_id, e.to_string())),
        };
        if args.command.trim().is_empty() {
            return Ok(ToolResult::failure(ctx.tool_call_id, "command is empty"));
        }
        let picked = match self
            .machines
            .pick(args.machine.as_deref(), Need::Shell)
            .await
        {
            Ok(picked) => picked,
            Err(e) => return Ok(ToolResult::failure(ctx.tool_call_id, e)),
        };
        if args.run_in_background {
            let request = Request::Start {
                command: args.command.clone(),
                cwd: picked.cwd().map(str::to_owned),
            };
            return Ok(match picked.online.tasks(&request).await {
                Ok(Reply::Started { id }) => ToolResult::success(
                    ctx.tool_call_id,
                    format!(
                        "Started background process {id} on {}. Use process with action=status \
                         to read output, or action=stop to stop it. Startup has not been \
                         verified.",
                        picked.shown()
                    ),
                ),
                Ok(Reply::Error { message }) => ToolResult::failure(ctx.tool_call_id, message),
                Ok(other) => {
                    ToolResult::failure(ctx.tool_call_id, format!("The node answered {other:?}"))
                }
                Err(e) => ToolResult::failure(ctx.tool_call_id, format!("{e:#}")),
            });
        }
        let command = match in_folder(picked.cwd(), &args.command) {
            Ok(command) => command,
            Err(e) => return Ok(ToolResult::failure(ctx.tool_call_id, e)),
        };
        let limit = args.limit();
        let mut output = Output::new(args.preview());
        // Output goes to the model only; the step shows just `STEP`.
        let mut on_output = |text: &str| output.push(text);
        // A command the node never got goes again once its link is back.
        let until = Instant::now() + NODE_BACK;
        let mut on = picked.online.clone();
        let exited = loop {
            let e = match on.exec(&command, limit, &mut on_output).await {
                Ok(exited) => break exited,
                Err(e) => e,
            };
            match on.again(&e, Repeat::IfUnsent, until).await {
                Some(back) => on = back,
                None => {
                    return Ok(ToolResult::failure(
                        ctx.tool_call_id,
                        format!("The command did not run on {}: {e:#}", picked.shown()),
                    ));
                }
            }
        };
        let text = report(&exited, output.finish(), limit);
        Ok(if exited.exit_code == Some(0) {
            ToolResult::success(ctx.tool_call_id, text)
        } else {
            ToolResult::failure(ctx.tool_call_id, text)
        })
    }
}

/// `command`, run in `cwd` when the machine has a workspace.
fn in_folder(cwd: Option<&str>, command: &str) -> Result<String, String> {
    match cwd {
        Some(cwd) => {
            let quoted = shlex::try_quote(cwd).map_err(|e| e.to_string())?;
            Ok(format!("cd -- {quoted} || exit 1\n{command}"))
        }
        None => Ok(command.to_owned()),
    }
}

/// The result for the model, as flashmind's `exec` writes it.
fn report(exited: &Exited, output: String, limit: Duration) -> String {
    let status = match (exited.exit_code, exited.timed_out) {
        (_, true) => format!("timed out after {}s and was stopped", limit.as_secs()),
        (Some(code), false) => format!("exit code: {code}"),
        (None, false) => "the shell ended before the command finished".to_owned(),
    };
    let mut text = format!("[{status}]\n{}", output.trim());
    if exited.truncated {
        text.push_str("\n[the node dropped output past 1 MiB]");
    }
    text
}

/// A command's output, whole or as its first and last lines.
struct Output {
    whole: String,
    preview: Option<Preview>,
    /// The end of a line that has not ended yet.
    partial: String,
}

impl Output {
    fn new(preview: Option<Preview>) -> Self {
        Self {
            whole: String::new(),
            preview,
            partial: String::new(),
        }
    }

    fn push(&mut self, text: &str) {
        let Some(preview) = &mut self.preview else {
            self.whole.push_str(text);
            return;
        };
        self.partial.push_str(text);
        while let Some(end) = self.partial.find('\n') {
            let line: String = self.partial.drain(..=end).collect();
            preview.push(line.trim_end_matches('\n').to_owned());
        }
    }

    fn finish(mut self) -> String {
        match self.preview.take() {
            None => self.whole,
            Some(mut preview) => {
                if !self.partial.is_empty() {
                    preview.push(std::mem::take(&mut self.partial));
                }
                preview.finish()
            }
        }
    }
}

/// The first `head` lines and last `tail`, as flashmind keeps them.
struct Preview {
    head_limit: usize,
    tail_limit: usize,
    head: Vec<String>,
    tail: VecDeque<String>,
    omitted: bool,
}

impl Preview {
    fn new(head_limit: usize, tail_limit: usize) -> Self {
        Self {
            head_limit,
            tail_limit,
            head: Vec::new(),
            tail: VecDeque::new(),
            omitted: false,
        }
    }

    fn push(&mut self, line: String) {
        if self.head.len() < self.head_limit {
            self.head.push(line);
        } else if self.tail_limit > 0 {
            if self.tail.len() == self.tail_limit {
                self.tail.pop_front();
                self.omitted = true;
            }
            self.tail.push_back(line);
        } else {
            self.omitted = true;
        }
    }

    fn finish(self) -> String {
        let mut lines = self.head;
        if self.omitted {
            lines.push("[output lines omitted]".to_owned());
        }
        lines.extend(self.tail);
        lines.join("\n")
    }
}

/// The background task's lines for `process`, as `status` shows them.
pub(super) fn task_lines(lines: &[String], earlier: bool) -> String {
    let mut out = String::new();
    if earlier {
        out.push_str(&format!(
            "[earlier lines omitted; the last {MAX_LINES} are kept]\n"
        ));
    }
    out.push_str(&lines.join("\n"));
    out
}

/// The step's label: "Running on build-box:" and the command's first line, else `STEP`.
fn step(args: &Value, machine: Option<&str>) -> String {
    let command = args.get("command").and_then(Value::as_str).unwrap_or("");
    let line = command.lines().map(str::trim).find(|line| !line.is_empty());
    match (line, machine) {
        (Some(line), Some(machine)) => {
            format!(
                "Running on {machine}: {}",
                shorten(line, STEP_COMMAND_CHARS)
            )
        }
        (Some(line), None) => format!("Running {}", shorten(line, STEP_COMMAND_CHARS)),
        (None, _) => STEP.to_owned(),
    }
}

/// The most of a command a step's label shows.
const STEP_COMMAND_CHARS: usize = 80;

#[cfg(test)]
mod tests {
    use super::*;

    fn args(value: Value) -> Result<Args, serde_json::Error> {
        serde_json::from_value(value)
    }

    #[test]
    fn head_and_tail_keep_the_ends_of_the_output() {
        let parsed = args(json!({ "command": "seq 10", "head": 2, "tail": 3 })).unwrap();
        let mut output = Output::new(parsed.preview());
        output.push("1\n2\n3\n4\n5\n6\n7");
        output.push("\n8\n9\n10");
        assert_eq!(output.finish(), "1\n2\n[output lines omitted]\n8\n9\n10");
        let parsed = args(json!({ "command": "x", "head": 900, "tail": 900 })).unwrap();
        let preview = parsed.preview().unwrap();
        assert_eq!(preview.head_limit + preview.tail_limit, MAX_PREVIEW_LINES);
    }

    #[test]
    fn commands_start_in_the_workspace() {
        assert_eq!(
            in_folder(Some("/home/user"), "ls").unwrap(),
            "cd -- /home/user || exit 1\nls"
        );
        assert_eq!(
            in_folder(Some("/srv/my work"), "ls").unwrap(),
            "cd -- '/srv/my work' || exit 1\nls"
        );
        assert_eq!(in_folder(None, "ls").unwrap(), "ls");
    }
}
