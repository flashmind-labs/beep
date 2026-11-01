//! `process`: the background tasks `exec` started on a machine.

use super::{
    Machines, Need,
    exec::{STEP, task_lines},
    in_parameter,
};
use crate::protocol::node::process::{Reply, Request, Task, TaskId};
use async_trait::async_trait;
use flashmind::types::tool::{Tool, ToolContext, ToolResult};
use serde::Deserialize;
use serde_json::{Value, json};

const DESCRIPTION: &str = "List background shell tasks, read their latest output and completion \
    status, or stop one. Use the process ID returned by exec with run_in_background=true, and the \
    same `in` it ran with. Output retains the last 100 lines. Up to 32 tasks are retained per \
    machine; completed tasks may be removed when new tasks start. Stopping a task stops \
    everything it started.";

pub(crate) struct ProcessTool {
    pub(crate) machines: Machines,
}

#[derive(Debug, Deserialize, PartialEq, Clone, Copy)]
#[serde(rename_all = "snake_case")]
enum Action {
    List,
    Status,
    Stop,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct Args {
    action: Action,
    #[serde(default)]
    id: Option<u32>,
    #[serde(default, rename = "in")]
    machine: Option<String>,
}

impl Args {
    /// The node request this call sends; the error is for the model.
    fn request(&self) -> Result<Request, String> {
        let id = || {
            self.id
                .map(TaskId)
                .ok_or_else(|| "id is required for status and stop".to_owned())
        };
        Ok(match self.action {
            Action::List => Request::List {},
            Action::Status => Request::Status { id: id()? },
            Action::Stop => Request::Stop { id: id()? },
        })
    }
}

#[async_trait]
impl Tool for ProcessTool {
    fn name(&self) -> &str {
        "process"
    }
    fn description(&self) -> &str {
        DESCRIPTION
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": { "type": "string", "enum": ["list", "status", "stop"] },
                "id": { "type": "integer", "description": "Process ID, required for status and stop." },
                "in": in_parameter("Omit for the user's own Linux environment.")
            },
            "required": ["action"],
            "additionalProperties": false
        })
    }
    fn humanize(&self, _: &Value) -> String {
        STEP.to_owned()
    }
    async fn execute(&self, ctx: ToolContext<'_>) -> anyhow::Result<ToolResult> {
        let args: Args = match serde_json::from_value(ctx.args.clone()) {
            Ok(args) => args,
            Err(e) => return Ok(ToolResult::failure(ctx.tool_call_id, e.to_string())),
        };
        let request = match args.request() {
            Ok(request) => request,
            Err(e) => return Ok(ToolResult::failure(ctx.tool_call_id, e)),
        };
        let picked = match self
            .machines
            .pick(args.machine.as_deref(), Need::Shell)
            .await
        {
            Ok(picked) => picked,
            Err(e) => return Ok(ToolResult::failure(ctx.tool_call_id, e)),
        };
        Ok(match picked.online.tasks(&request).await {
            Ok(reply) => match shown(reply) {
                Ok(text) => ToolResult::success(ctx.tool_call_id, text),
                Err(text) => ToolResult::failure(ctx.tool_call_id, text),
            },
            Err(e) => ToolResult::failure(ctx.tool_call_id, format!("{e:#}")),
        })
    }
}

/// A node's reply for the model.
fn shown(reply: Reply) -> Result<String, String> {
    match reply {
        Reply::Tasks { tasks } if tasks.is_empty() => Ok("No background processes.".into()),
        Reply::Tasks { tasks } => Ok(tasks.iter().map(line).collect::<Vec<_>>().join("\n")),
        Reply::Status {
            task,
            lines,
            earlier,
        } => {
            let mut out = format!("Process {}: {}", task.id, task.state);
            let lines = task_lines(&lines, earlier);
            if !lines.is_empty() {
                out.push('\n');
                out.push_str(&lines);
            }
            Ok(out)
        }
        Reply::Error { message } => Err(message),
        other => Err(format!("The node answered {other:?}")),
    }
}

fn line(task: &Task) -> String {
    format!("{}: {} ({})", task.id, task.state, task.command)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::node::process::TaskState;

    fn args(value: Value) -> Result<Args, serde_json::Error> {
        serde_json::from_value(value)
    }

    #[test]
    fn actions_become_node_requests() {
        assert_eq!(
            args(json!({ "action": "list" })).unwrap().request(),
            Ok(Request::List {})
        );
        let stop = args(json!({ "action": "stop", "id": 3, "in": "mac" })).unwrap();
        assert_eq!(stop.machine.as_deref(), Some("mac"));
        assert_eq!(stop.request(), Ok(Request::Stop { id: TaskId(3) }));
        assert_eq!(
            args(json!({ "action": "status" })).unwrap().request(),
            Err("id is required for status and stop".into())
        );
        assert!(args(json!({ "action": "kill", "id": 1 })).is_err());
    }

    #[test]
    fn replies_read_as_lines() {
        let task = |id, state| Task {
            id: TaskId(id),
            command: "sleep 300".into(),
            state,
        };
        assert_eq!(
            shown(Reply::Tasks {
                tasks: vec![task(1, TaskState::Running), task(2, TaskState::Stopped)]
            }),
            Ok("1: running (sleep 300)\n2: stopped (sleep 300)".into())
        );
        assert_eq!(
            shown(Reply::Tasks { tasks: vec![] }),
            Ok("No background processes.".into())
        );
        assert_eq!(
            shown(Reply::Status {
                task: task(4, TaskState::Exited { code: Some(0) }),
                lines: vec!["ready".into()],
                earlier: true,
            }),
            Ok(
                "Process 4: completed\n[earlier lines omitted; the last 100 are kept]\nready"
                    .into()
            )
        );
        assert_eq!(
            shown(Reply::Error {
                message: "No background process 9.".into()
            }),
            Err("No background process 9.".into())
        );
    }
}
