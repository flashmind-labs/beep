//! `project_reply`: post a reply on a pull request or issue of the project,
//! always after the user approves its card.

use super::{ProjectTools, card, field, leaked};
use crate::projects::{
    gate::{Asks, Gate},
    issues::IssueError,
    threads::{self, ReadThread},
};
use anyhow::Result;
use async_trait::async_trait;
use flashmind::types::tool::{Tool, ToolContext, ToolResult};
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;

pub(crate) const REPLY: &str = "project_reply";
/// Never saved: a reply always asks.
pub(crate) const REPLY_SLUG: &str = "PROJECT_REPLY";
/// The longest reply.
const MAX_REPLY: usize = 16 * 1024;

pub(super) struct ReplyTool {
    pub(super) tools: Arc<ProjectTools>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReplyArgs {
    repo: String,
    number: u64,
    body: String,
}

#[async_trait]
impl Tool for ReplyTool {
    fn name(&self) -> &str {
        REPLY
    }

    fn description(&self) -> &str {
        "Post a reply as the user on a pull request or issue of this project's repository. The \
         user sees the reply on a card and it posts only once they approve. Keep it short and in \
         the user's voice. It must not name machines, user names, home folders, IP addresses or \
         ports. A thread someone else opened gets one reply from Beep at most."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "repo": {"type": "string", "description": "The project's repository, as org/repo."},
                "number": {"type": "integer", "minimum": 1, "description": "The pull request or issue number."},
                "body": {"type": "string", "description": "The reply, in Markdown."}
            },
            "required": ["repo", "number", "body"],
            "additionalProperties": false
        })
    }

    async fn execute(&self, ctx: ToolContext<'_>) -> Result<ToolResult> {
        let id = ctx.tool_call_id;
        let args: ReplyArgs = match ctx.parse_args(REPLY) {
            Ok(args) => args,
            Err(error) => return Ok(ToolResult::failure(id, error.to_string())),
        };
        Ok(match self.reply(id, &args).await? {
            Replied::Done(text) => ToolResult::success(id, text),
            Replied::Refused(text) => ToolResult::failure(id, text),
            Replied::Gated(result) => result,
        })
    }

    fn humanize(&self, args: &Value) -> String {
        match args.get("number").and_then(Value::as_u64) {
            Some(number) => format!("Replying on #{number}"),
            None => "Replying".to_owned(),
        }
    }
}

enum Replied {
    Done(String),
    Refused(String),
    Gated(ToolResult),
}

fn refused(error: IssueError) -> Result<Replied> {
    match error {
        IssueError::Store(error) => Err(error),
        other => Ok(Replied::Refused(other.to_string())),
    }
}

impl ReplyTool {
    async fn reply(&self, id: &str, args: &ReplyArgs) -> Result<Replied> {
        let tools = &self.tools;
        let project = tools.project().await?;
        let repo = match threads::project_repo(&project, &args.repo) {
            Ok(repo) => repo,
            Err(error) => return refused(error),
        };
        let body = args.body.trim();
        if body.is_empty() || body.len() > MAX_REPLY {
            return Ok(Replied::Refused(format!(
                "Give the reply as body, 1 to {MAX_REPLY} bytes."
            )));
        }
        let hits = tools.public_text().await?.hits(body);
        if !hits.is_empty() {
            return Ok(Replied::Refused(leaked(&hits)));
        }
        let read =
            match threads::read(tools.fleet.as_ref(), &tools.user, &project, args.number).await {
                Ok(read) => read,
                Err(error) => return refused(error),
            };
        // A task tracking someone else's issue keeps its one comment too.
        let tracked = tools
            .user
            .project_tasks(project.id)
            .await?
            .into_iter()
            .find(|task| task.issue.as_ref().map(|issue| issue.number) == Some(args.number));
        let once = read.foreign() || tracked.as_ref().is_some_and(|task| task.issue_foreign);
        if once && tracked.as_ref().is_some_and(|task| task.issue_commented) {
            return Ok(Replied::Refused(once_said(&read)));
        }
        let label = read.thread.kind.label(args.number);
        let reply_card = || {
            let mut fields = vec![
                field("Thread", format!("{label}: {}", read.thread.title.trim())),
                field("Link", read.thread.url.clone()),
            ];
            let last = read.last_other();
            if !last.is_empty() {
                fields.push(field("Last from them", last));
            }
            fields.push(field("Reply", body));
            card("Post reply".to_owned(), fields)
        };
        match tools.gate.check(id, REPLY_SLUG, Asks::Always, reply_card) {
            Gate::Run => {}
            Gate::Refuse(reason) => return Ok(Replied::Refused(reason)),
            Gate::Park(result) => return Ok(Replied::Gated(result)),
        }
        // Noted first, so two calls cannot both post on someone else's thread.
        if once {
            let fresh = tools
                .user
                .note_thread_reply(project.id, &repo, args.number)
                .await?;
            let task_fresh = match &tracked {
                Some(task) => tools.user.note_issue_comment(task.id, true).await?,
                None => true,
            };
            if !fresh || !task_fresh {
                return Ok(Replied::Refused(once_said(&read)));
            }
        } else if let Some(task) = &tracked {
            tools.user.note_issue_comment(task.id, false).await?;
        }
        Ok(match read.comment(body).await {
            Ok(()) => Replied::Done(format!("Posted the reply on {label}.")),
            Err(error) => Replied::Refused(format!("The reply failed: {error:#}")),
        })
    }
}

fn once_said(read: &ReadThread) -> String {
    format!(
        "Beep already replied on {}, which someone else opened; it gets one reply at most.",
        read.thread.kind.label(read.thread.number)
    )
}
