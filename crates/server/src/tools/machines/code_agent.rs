//! `code_agent`: coding agent sessions on the user's machines.

use super::{
    Machines, Need,
    agents::{self, Asker, DEFAULT_LAST, DEFAULT_WAIT_SECS, MAX_WAIT_SECS, Order},
    finish, in_parameter,
};
use crate::protocol::node::agent::{self, Steer};
use async_trait::async_trait;
use flashmind::types::tool::{Tool, ToolContext, ToolResult};
use serde::Deserialize;
use serde_json::{Value, json};
use std::time::Duration;
use uuid::Uuid;

const DESCRIPTION: &str = "Run coding agents (Claude Code, Codex, Gemini CLI, pi, flsh, OpenCode) on the user's \
    machines, in sessions that keep running between calls. `start` begins a session with \
    `prompt` (in `cwd` if given) and waits up to `wait_secs` (default 90) for the turn to end or \
    a question, then returns the session id and its state: status, what it is doing, its plan, \
    files changed, its last message. `status` checks one session by `id` (add `wait_secs` to \
    wait for its turn), or without `id` lists them; `prompt` sends `prompt` to session `id` \
    (waiting behind a running turn); `answer` answers its open question with `prompt`, an \
    option number, id or name (any other text declines and goes to the agent as its next \
    prompt); `cancel` stops the turn; `close` ends the session; `transcript` reads the last \
    `last` entries (default 20); `sessions` lists every session on the machine. A session \
    started, prompted or taken over from this chat reports back here on its own when a turn \
    ends or it asks something after the call stopped waiting, so do not poll `status` for it; \
    its questions also reach the user as a card they can answer. Claude Code sessions the user \
    started themselves (in a terminal or IDE): `sessions` with `all: true` lists them too, \
    `transcript` reads one, `follow` posts in this chat when it finishes a turn or waits for \
    the user (`unfollow` stops), and `takeover` resumes one that nothing runs any more as a \
    session here, with an optional `prompt`. One open in the user's terminal can be checked \
    and steered through beep-node's hooks: `live` says what it does now (working or waiting, \
    the tool running and for how long, last error, todos, files changed, last message), \
    `steer` sends it `prompt`, which Claude reads after the current tool call or instead of \
    ending its turn, and `interrupt` stops it at its next tool call. One idle at its prompt \
    cannot be steered. In a spoken reply, say `live`'s first line and offer to dig in (test \
    output, the diff) rather than reading the details aloud. Start a session on a machine that can do the work: iOS and Mac apps build only \
    on macOS with xcode. Agents run with their own permission prompts off, so they act without \
    asking; a question that still comes is one the agent puts to the user. Answer it yourself \
    if the answer is clearly within what the user asked. When it is risky, destructive or \
    unclear, such as a \
    force push or deleting data, do not answer it either way, not even to deny: tell the user \
    what it wants to do and what you recommend, and they answer on its card or in the chat. \
    `agent` picks the agent when the machine has several.";

pub(super) struct CodeAgentTool {
    pub(super) machines: Machines,
    /// The chat the turn runs in, which followed sessions report to.
    pub(super) chat: Uuid,
}

#[derive(Debug, Deserialize, PartialEq, Clone, Copy)]
#[serde(rename_all = "snake_case")]
enum Action {
    Start,
    Status,
    Sessions,
    Prompt,
    Answer,
    Cancel,
    Close,
    Transcript,
    Follow,
    Unfollow,
    Takeover,
    Live,
    Steer,
    Interrupt,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct Args {
    action: Action,
    #[serde(default)]
    agent: Option<String>,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    prompt: Option<String>,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    wait_secs: Option<u64>,
    #[serde(default)]
    last: Option<u32>,
    #[serde(default)]
    all: bool,
    #[serde(default, rename = "in")]
    machine: Option<String>,
}

/// What a call does once its machine and agent are known.
#[derive(Debug, PartialEq)]
enum Plan {
    /// Every session on the machine, of any agent.
    AllSessions,
    /// An order for one agent.
    Agent(Order),
}

impl Args {
    fn wait(&self) -> Duration {
        Duration::from_secs(
            self.wait_secs
                .unwrap_or(DEFAULT_WAIT_SECS)
                .min(MAX_WAIT_SECS),
        )
    }

    /// The session id given, trimmed; a blank one is none.
    fn given_id(&self) -> Option<String> {
        self.id
            .as_deref()
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .map(str::to_owned)
    }

    fn id(&self) -> Result<String, String> {
        self.given_id()
            .ok_or_else(|| format!("{:?} needs the session's `id`.", self.action))
    }

    fn prompt(&self) -> Option<String> {
        self.prompt.clone().filter(|text| !text.trim().is_empty())
    }

    fn needs_prompt(&self) -> Result<String, String> {
        self.prompt()
            .ok_or_else(|| format!("{:?} needs `prompt`.", self.action))
    }

    /// Whether the call is about every agent when it names none.
    fn any_agent(&self) -> bool {
        self.agent.is_none()
            && (self.action == Action::Sessions && !self.all
                || self.action == Action::Status && self.given_id().is_none())
    }

    /// What the call does; the error is for the model.
    fn plan(&self) -> Result<Plan, String> {
        if self.any_agent() {
            return Ok(Plan::AllSessions);
        }
        Ok(Plan::Agent(match self.action {
            Action::Start => Order::Start {
                cwd: self.cwd.clone(),
                prompt: self.needs_prompt()?,
                wait: self.wait(),
            },
            Action::Status => Order::Status {
                id: self.given_id(),
                wait: self.wait_secs.map(|_| self.wait()),
            },
            Action::Sessions => Order::Sessions { all: self.all },
            Action::Prompt => Order::Prompt {
                id: self.id()?,
                text: self.needs_prompt()?,
                wait: self.wait(),
            },
            Action::Answer => Order::Answer {
                id: self.id()?,
                text: self.needs_prompt()?,
                wait: self.wait(),
            },
            Action::Cancel => Order::Cancel { id: self.id()? },
            Action::Close => Order::Close { id: self.id()? },
            Action::Transcript => Order::Transcript {
                id: self.id()?,
                last: self.last.unwrap_or(DEFAULT_LAST),
            },
            Action::Follow => Order::Follow { id: self.id()? },
            Action::Unfollow => Order::Unfollow { id: self.id()? },
            Action::Takeover => Order::Takeover {
                id: self.id()?,
                prompt: self.prompt(),
                wait: self.wait(),
            },
            Action::Live => Order::Live { id: self.id()? },
            Action::Steer => Order::Steer {
                id: self.id()?,
                steer: Steer::Message {
                    text: self.needs_prompt()?,
                },
            },
            Action::Interrupt => Order::Steer {
                id: self.id()?,
                steer: Steer::Stop {},
            },
        }))
    }
}

/// The agent a call uses: the one it names, or the only one `offered`.
fn agent_for<'a>(named: Option<&'a str>, offered: &[&'a str]) -> Result<&'a str, String> {
    match (named, offered) {
        (Some(name), _) => Ok(name),
        (None, [only]) => Ok(only),
        (None, []) => Err("This machine offers no coding agent.".into()),
        (None, many) => Err(format!("Name the `agent`: {}.", many.join(", "))),
    }
}

#[async_trait]
impl Tool for CodeAgentTool {
    fn name(&self) -> &str {
        "code_agent"
    }
    fn description(&self) -> &str {
        DESCRIPTION
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["start", "status", "sessions", "prompt", "answer", "cancel", "close", "transcript", "follow", "unfollow", "takeover", "live", "steer", "interrupt"]
                },
                "agent": { "type": "string", "enum": ["claude", "codex", "gemini", "pi", "flsh", "opencode"] },
                "id": { "type": "string", "description": "The session id" },
                "prompt": { "type": "string", "description": "The prompt for start, prompt and takeover, the message for steer, or the answer for answer" },
                "cwd": { "type": "string", "description": "The folder start runs the agent in" },
                "wait_secs": { "type": "integer", "minimum": 0, "maximum": MAX_WAIT_SECS },
                "last": { "type": "integer", "minimum": 1, "description": "How many transcript entries" },
                "all": { "type": "boolean", "description": "sessions: also list Claude Code sessions started outside Beep" },
                "in": in_parameter("Omit for the user's own environment, or the only machine with coding agents.")
            },
            "required": ["action"],
            "additionalProperties": false
        })
    }
    fn timeout_secs(&self) -> Option<u64> {
        Some(MAX_WAIT_SECS + 60)
    }
    fn humanize(&self, args: &Value) -> String {
        let agent = args
            .get("agent")
            .and_then(Value::as_str)
            .map_or("a coding agent", crate::nodes::agent_name);
        match args.get("action").and_then(Value::as_str) {
            Some("start") => format!("Starting {agent}"),
            Some("follow") => format!("Following {agent}"),
            Some("unfollow") => format!("Unfollowing {agent}"),
            Some("takeover") => format!("Taking over {agent}"),
            Some("prompt" | "steer") => format!("Messaging {agent}"),
            Some("interrupt") => format!("Stopping {agent}"),
            Some("answer") => format!("Answering {agent}"),
            Some("cancel" | "close") => format!("Stopping {agent}"),
            _ => format!("Checking {agent}"),
        }
    }
    async fn execute(&self, ctx: ToolContext<'_>) -> anyhow::Result<ToolResult> {
        let call_id = ctx.tool_call_id;
        let args: Args = match serde_json::from_value(ctx.args.clone()) {
            Ok(args) => args,
            Err(e) => return Ok(ToolResult::failure(call_id, e.to_string())),
        };
        let plan = match args.plan() {
            Ok(plan) => plan,
            Err(e) => return Ok(ToolResult::failure(call_id, e)),
        };
        let need = Need::Agent(args.agent.as_deref());
        let picked = match self.machines.pick(args.machine.as_deref(), need).await {
            Ok(picked) => picked,
            Err(e) => return Ok(ToolResult::failure(call_id, e)),
        };
        let node = picked.shown();
        let order = match plan {
            Plan::AllSessions => {
                return Ok(finish(
                    call_id,
                    agents::sessions(&picked.online, &node).await,
                ));
            }
            Plan::Agent(order) => order,
        };
        let offered: Vec<&str> = picked
            .online
            .offers
            .iter()
            .filter_map(|offer| agent::agent_of(offer))
            .collect();
        let agent = match agent_for(args.agent.as_deref(), &offered) {
            Ok(agent) => agent,
            Err(e) => return Ok(ToolResult::failure(call_id, e)),
        };
        let asker = Asker {
            app: &self.machines.app,
            user: &self.machines.user,
            chat: self.chat,
            endpoint: &picked.endpoint,
        };
        let result = agents::run(&picked.online, &node, agent, order, &asker).await;
        Ok(finish(call_id, result))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan(value: Value) -> Result<Plan, String> {
        serde_json::from_value::<Args>(value)
            .map_err(|e| e.to_string())?
            .plan()
    }

    #[test]
    fn a_status_id_is_trimmed_and_a_blank_one_is_none() {
        assert_eq!(
            plan(json!({ "action": "status", "id": " " })),
            Ok(Plan::AllSessions)
        );
        assert_eq!(
            plan(json!({ "action": "status", "agent": "codex", "id": "a3f9c1 " })),
            Ok(Plan::Agent(Order::Status {
                id: Some("a3f9c1".into()),
                wait: None
            }))
        );
    }
}
