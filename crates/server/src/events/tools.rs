//! The agent's event tools: `event_topics` lists and changes which account
//! events the user gets, and `recent_events` says what came in and what the
//! triage agent made of it, in any chat; `hold_events` keeps events of a
//! woken turn for a later digest.

use super::{Topic, Urgency, Verdict, triggers};
use crate::{
    store::{EventState, RecentEvent},
    user::User,
};
use async_trait::async_trait;
use chrono::{Duration as Days, Utc};
use flashmind::types::tool::{Tool, ToolContext, ToolResult};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{fmt::Write, str::FromStr, sync::Arc, time::Duration};

/// How long a change waits for Composio to set its triggers up.
const TRIGGER_SYNC_TIMEOUT: Duration = Duration::from_secs(30);

pub(crate) struct EventTopicsTool {
    pub user: Arc<User>,
}

#[derive(Deserialize)]
struct TopicArgs {
    #[serde(default)]
    changes: Vec<TopicChange>,
}

#[derive(Deserialize)]
struct TopicChange {
    topic: String,
    enabled: bool,
}

#[async_trait]
impl Tool for EventTopicsTool {
    fn name(&self) -> &str {
        "event_topics"
    }
    fn description(&self) -> &str {
        "List or change which events from the user's connected accounts wake you, such as \
         new mail, calendar changes, GitHub review requests or failed CI. Without changes it \
         lists every topic and whether it is on. Use it when the user asks what you watch \
         for them, or to stop or start telling them about something."
    }
    fn parameters(&self) -> Value {
        let topics: Vec<&str> = Topic::ALL.iter().map(|topic| topic.as_str()).collect();
        json!({
            "type": "object",
            "properties": {
                "changes": {
                    "type": "array",
                    "description": "Topics to turn on or off. Omit to only list them.",
                    "items": {
                        "type": "object",
                        "properties": {
                            "topic": {"type": "string", "enum": topics},
                            "enabled": {"type": "boolean"}
                        },
                        "required": ["topic", "enabled"],
                        "additionalProperties": false
                    }
                }
            },
            "additionalProperties": false
        })
    }
    async fn execute(&self, ctx: ToolContext<'_>) -> anyhow::Result<ToolResult> {
        let call = ctx.tool_call_id;
        let args: TopicArgs = match serde_json::from_value(ctx.args.clone()) {
            Ok(args) => args,
            Err(error) => return Ok(ToolResult::failure(call, format!("Bad arguments: {error}"))),
        };
        for change in &args.changes {
            let Ok(topic) = Topic::from_str(&change.topic) else {
                return Ok(ToolResult::failure(
                    call,
                    format!("Unknown topic {}", change.topic),
                ));
            };
            self.user.set_event_topic(topic, change.enabled).await?;
        }
        let mut text = String::new();
        if !args.changes.is_empty() {
            let synced =
                tokio::time::timeout(TRIGGER_SYNC_TIMEOUT, triggers::sync_user(&self.user)).await;
            match synced {
                Ok(Ok(())) => {}
                Ok(Err(error)) => {
                    log::warn!("events: unable to update triggers: {error:#}");
                    text.push_str("Saved, but the app triggers could not be updated yet; ");
                    text.push_str("the next check in a few minutes tries again.\n");
                }
                Err(_) => text.push_str("Saved; the app triggers are still being updated.\n"),
            }
        }
        for (topic, on) in self.user.event_topics().await?.iter() {
            let state = if on { "on" } else { "off" };
            let _ = writeln!(text, "{}: {state} ({})", topic.as_str(), topic.describe());
        }
        Ok(ToolResult::success(call, text.trim_end()))
    }
}

pub(crate) struct HoldEventsTool {
    pub user: Arc<User>,
}

#[derive(Deserialize)]
struct HoldArgs {
    ids: Vec<i64>,
}

#[async_trait]
impl Tool for HoldEventsTool {
    fn name(&self) -> &str {
        "hold_events"
    }
    fn description(&self) -> &str {
        "Keep events from this turn's list for a later digest, by their [#id]. Use it for \
         events worth telling the user about, but not now."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "ids": {"type": "array", "items": {"type": "integer"}}
            },
            "required": ["ids"],
            "additionalProperties": false
        })
    }
    async fn execute(&self, ctx: ToolContext<'_>) -> anyhow::Result<ToolResult> {
        let call = ctx.tool_call_id;
        let args: HoldArgs = match serde_json::from_value(ctx.args.clone()) {
            Ok(args) => args,
            Err(error) => return Ok(ToolResult::failure(call, format!("Bad arguments: {error}"))),
        };
        // The run can start before its events are marked woken, so pending
        // ones are held too; the wake then leaves them held.
        let mut held = 0;
        for from in [EventState::Woken, EventState::Pending] {
            held += self
                .user
                .move_events(&args.ids, from, EventState::Held, None)
                .await?;
        }
        Ok(ToolResult::success(
            call,
            format!("Held {held} events for the digest."),
        ))
    }
}

/// The longest span `recent_events` looks back over, in hours.
const MAX_HOURS: i64 = 14 * 24;
/// Most events `recent_events` lists.
const MAX_RECENT: i64 = 60;

pub(crate) struct RecentEventsTool {
    pub user: Arc<User>,
}

#[derive(Deserialize)]
struct RecentArgs {
    #[serde(default = "default_hours")]
    hours: i64,
}

fn default_hours() -> i64 {
    24
}

#[async_trait]
impl Tool for RecentEventsTool {
    fn name(&self) -> &str {
        "recent_events"
    }
    fn description(&self) -> &str {
        "List what came in from the user's connected accounts (mail, calendar, GitHub), newest \
         first, with what the event triage agent decided about each: passed on now or for \
         later, with its summary, ignored and why, or not triaged yet. Use it when the user \
         asks what came in today or why they were or were not told about something."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "hours": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": MAX_HOURS,
                    "description": "How far back to look. Defaults to 24."
                }
            },
            "additionalProperties": false
        })
    }
    async fn execute(&self, ctx: ToolContext<'_>) -> anyhow::Result<ToolResult> {
        let call = ctx.tool_call_id;
        let args: RecentArgs = match serde_json::from_value(ctx.args.clone()) {
            Ok(args) => args,
            Err(error) => return Ok(ToolResult::failure(call, format!("Bad arguments: {error}"))),
        };
        let hours = args.hours.clamp(1, MAX_HOURS);
        let since = Utc::now() - Days::hours(hours);
        let events = self.user.recent_events(since, MAX_RECENT).await?;
        if events.is_empty() {
            return Ok(ToolResult::success(
                call,
                format!("Nothing came in over the last {hours} hours."),
            ));
        }
        let mut text = String::new();
        for event in &events {
            let _ = writeln!(text, "{}", recent_line(event));
        }
        Ok(ToolResult::success(call, text.trim_end()))
    }
}

/// One event and what became of it.
fn recent_line(recent: &RecentEvent) -> String {
    let event = &recent.event;
    let mut line = format!(
        "[#{}] {}, {}, {}: {}",
        event.id,
        event.occurred.format("%Y-%m-%d %H:%M UTC"),
        event.app.name(),
        event.kind.as_str().replace('_', " "),
        event.summary
    );
    let outcome = match &recent.triage {
        None => match event.state {
            EventState::Pending | EventState::Triaging => "not triaged yet".to_owned(),
            EventState::Held | EventState::Digested => "held for a digest".to_owned(),
            EventState::Woken | EventState::Triaged => "passed on".to_owned(),
        },
        Some(triage) => match triage.verdict {
            Verdict::Inform => {
                let when = match triage.urgency {
                    Some(Urgency::Now) => "now",
                    _ => "for later",
                };
                let summary = triage.summary.as_deref().unwrap_or_default();
                if triage.refs.is_empty() || triage.refs.contains(&event.id) {
                    format!("passed on {when}: {summary}")
                } else {
                    format!("left out of a summary passed on {when}")
                }
            }
            Verdict::Ignore => format!("ignored: {}", triage.reason),
            Verdict::Failed if triage.refs.contains(&event.id) => {
                format!("{}; passed on as it looked urgent", triage.reason)
            }
            Verdict::Failed => format!("{}; dropped", triage.reason),
        },
    };
    let _ = write!(line, " -> {outcome}");
    line
}
