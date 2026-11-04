//! `request_grant`: the agent asks the user for a standing grant.

use super::{
    Constraints, DEFAULT_HOURS, GRANTABLE, Grantable, MAX_HOURS, MAX_PER_RUN, REQUEST_GRANT,
    grantable,
};
use crate::{
    account::user::User,
    apps::permissions::{self, ApprovalPayload, ToolSlug},
    protocol::{Approval, ApprovalField},
    routines::digest,
    store::NewGrant,
};
use anyhow::Result;
use async_trait::async_trait;
use chrono::{Duration, Utc};
use flashmind::types::{
    ToolCall,
    tool::{Tool, ToolContext, ToolResult},
};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::sync::Arc;
use uuid::Uuid;

/// What the agent asks for.
#[derive(Deserialize)]
pub(super) struct Request {
    pub(super) routine: String,
    pub(super) tool: String,
    pub(super) recipients: Vec<String>,
    #[serde(default)]
    pub(super) exact: Map<String, Value>,
    #[serde(default)]
    pub(super) per_run: Option<u32>,
    #[serde(default)]
    pub(super) hours: Option<u32>,
}

/// A request that can be granted, with the card that asks.
pub(super) struct Asked {
    pub(super) routine: Uuid,
    pub(super) tool: ToolSlug,
    pub(super) constraints: Constraints,
    pub(super) per_run: u32,
    pub(super) hours: u32,
    pub(super) card: Approval,
}

/// Asks the user for a grant on an approval card. Never given to a routine's own runs, so text it reads cannot ask.
pub(crate) struct RequestGrantTool {
    pub(crate) user: Arc<User>,
    /// The call the user just allowed, which saves its grant.
    pub(crate) approved: Option<String>,
}

#[async_trait]
impl Tool for RequestGrantTool {
    fn name(&self) -> &str {
        REQUEST_GRANT
    }

    fn description(&self) -> &str {
        "Ask the user to allow one routine to act without asking each time, such as \
         replying to Ana in one thread whenever she writes. Call it right after setting up a \
         routine that will send, with the routine's id. The user sees a card with exactly what \
         is allowed and answers once. Each run of the routine may then make `per_run` calls \
         of `tool` without asking, only to `recipients` and only with the `exact` values; any \
         other call asks as usual. It ends when the routine ends, after `hours`, or when the \
         user revokes it. Tools: GMAIL_REPLY_TO_THREAD (exact must pin thread_id), \
         GMAIL_SEND_EMAIL, OUTLOOK_SEND_EMAIL, FASTMAIL_SEND_EMAIL (may pin mode, emailId, \
         account)."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "routine": {
                    "type": "string",
                    "description": "The id of the routine the grant is for, as cron_create or cron_list gave it."
                },
                "tool": {
                    "type": "string",
                    "enum": GRANTABLE.iter().map(|spec| spec.tool).collect::<Vec<_>>(),
                    "description": "The one tool the routine may call."
                },
                "recipients": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Every email address the routine may send to, such as [\"ana@example.com\"]."
                },
                "exact": {
                    "type": "object",
                    "additionalProperties": {"type": "string"},
                    "description": "Arguments the call must give with exactly these values, such as {\"thread_id\": \"18c2...\"}."
                },
                "per_run": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": MAX_PER_RUN,
                    "description": "Calls each run may make. Default 1."
                },
                "hours": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": MAX_HOURS,
                    "description": "Hours until it expires. Default 8, at most a week."
                }
            },
            "required": ["routine", "tool", "recipients"],
            "additionalProperties": false
        })
    }

    async fn execute(&self, ctx: ToolContext<'_>) -> Result<ToolResult> {
        let id = ctx.tool_call_id;
        let request: Request = match ctx.parse_args(REQUEST_GRANT) {
            Ok(request) => request,
            Err(error) => return Ok(ToolResult::failure(id, error.to_string())),
        };
        let asked = match prepare(&self.user, &request).await? {
            Ok(asked) => asked,
            Err(reason) => return Ok(ToolResult::failure(id, format!("Not asked: {reason}"))),
        };
        if self.approved.as_deref() != Some(id) {
            return Ok(ToolResult::interrupt(
                id,
                Arc::new(ApprovalPayload {
                    approval: asked.card,
                }),
            ));
        }
        self.user
            .create_grant(NewGrant {
                routine: asked.routine,
                tool: asked.tool.as_str(),
                constraints: serde_json::to_value(&asked.constraints)?,
                per_run: asked.per_run,
                shown: serde_json::to_value(&asked.card.fields)?,
                call_id: id,
                expires_at: Utc::now() + Duration::hours(i64::from(asked.hours)),
            })
            .await?;
        Ok(ToolResult::success(
            id,
            "The user allowed it. Runs of the routine may now make these calls without asking; \
             anything else still asks. Tell them in a few words.",
        ))
    }

    fn humanize(&self, _args: &Value) -> String {
        "Asking to allow a routine".to_owned()
    }
}

/// Checks a request and builds its card, or says why it cannot be granted.
pub(super) async fn prepare(user: &User, request: &Request) -> Result<Result<Asked, String>> {
    let tool = ToolSlug::new(&request.tool);
    let Some(spec) = grantable(&tool) else {
        return Ok(Err(format!(
            "{} cannot be granted. Leave it to ask the user each time.",
            request.tool
        )));
    };
    let Some(routine) = Uuid::parse_str(request.routine.trim()).ok() else {
        return Ok(Err("routine is not a routine id.".to_owned()));
    };
    let Some(job) = user.cron_job(routine).await? else {
        return Ok(Err("The user has no routine with that id.".to_owned()));
    };
    if digest::is_digest(&job) {
        return Ok(Err("The digest cannot be given a grant.".to_owned()));
    }
    let per_run = request.per_run.unwrap_or(1);
    if !(1..=MAX_PER_RUN).contains(&per_run) {
        return Ok(Err(format!("per_run must be 1 to {MAX_PER_RUN}.")));
    }
    let hours = request.hours.unwrap_or(DEFAULT_HOURS);
    if !(1..=MAX_HOURS).contains(&hours) {
        return Ok(Err(format!("hours must be 1 to {MAX_HOURS}.")));
    }
    let constraints = match Constraints::new(spec, &request.recipients, &request.exact) {
        Ok(constraints) => constraints,
        Err(reason) => return Ok(Err(reason)),
    };
    let label = job
        .metadata
        .get("label")
        .and_then(Value::as_str)
        .filter(|label| !label.trim().is_empty())
        .unwrap_or(&job.task);
    let card = card(
        user,
        spec,
        &constraints,
        Card {
            routine: label,
            per_run,
            hours,
        },
    )
    .await;
    Ok(Ok(Asked {
        routine,
        tool,
        constraints,
        per_run,
        hours,
        card,
    }))
}

/// What a grant's card says besides its constraints.
pub(super) struct Card<'a> {
    pub(super) routine: &'a str,
    pub(super) per_run: u32,
    pub(super) hours: u32,
}

/// The card asking for a grant, with pinned ids read back so the user sees the thread's subject.
pub(super) async fn card(
    user: &User,
    spec: &Grantable,
    constraints: &Constraints,
    about: Card<'_>,
) -> Approval {
    let tool = ToolSlug::new(spec.tool);
    let (app, action) = match permissions::lookup(tool.as_str()) {
        Some((app, _)) => (app.name(), permissions::action_name(&app, tool.as_str())),
        None => (spec.tool.to_owned(), spec.tool.to_lowercase()),
    };
    let routine: String = about
        .routine
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let calls = if about.per_run == 1 {
        format!("1 {}", spec.noun)
    } else {
        format!("Up to {} {}s", about.per_run, spec.noun)
    };
    let mut fields = vec![
        ApprovalField {
            label: "Routine".to_owned(),
            value: permissions::shorten(&routine, 200),
        },
        ApprovalField {
            label: "Allows".to_owned(),
            value: format!("{calls} each time it runs, without asking"),
        },
        ApprovalField {
            label: "To".to_owned(),
            value: format!(
                "{} only",
                constraints
                    .recipients
                    .iter()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        },
    ];
    for (key, value) in &constraints.pinned {
        let label = permissions::field_label(key);
        fields.push(if permissions::is_id(key) {
            ApprovalField {
                label: "Only in".to_owned(),
                value: format!(
                    "this {}",
                    label
                        .trim_end_matches(" id")
                        .trim_end_matches("Id")
                        .to_lowercase()
                ),
            }
        } else {
            ApprovalField {
                label,
                value: value.clone(),
            }
        });
    }
    if !constraints.pinned.is_empty() {
        let named = ToolCall {
            id: String::new(),
            name: spec.tool.to_owned(),
            arguments: Value::Object(
                constraints
                    .pinned
                    .iter()
                    .map(|(key, value)| (key.clone(), Value::String(value.clone())))
                    .collect(),
            ),
        };
        fields.extend(permissions::read_back(user, &named).await);
    }
    fields.push(ApprovalField {
        label: "Ends".to_owned(),
        value: format!("When the routine ends, or in {}", span(about.hours)),
    });
    Approval {
        app,
        action: format!("{action} for a routine"),
        fields,
    }
}

/// A grant's lifetime as the card says it, such as "2 days and 6 hours".
pub(super) fn span(hours: u32) -> String {
    let unit = |n: u32, one: &str| {
        if n == 1 {
            format!("1 {one}")
        } else {
            format!("{n} {one}s")
        }
    };
    match (hours / 24, hours % 24) {
        (0, h) => unit(h, "hour"),
        (d, 0) => unit(d, "day"),
        (d, h) => format!("{} and {}", unit(d, "day"), unit(h, "hour")),
    }
}
