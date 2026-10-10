//! Reminders a group sets for itself. Each one is a row in `crons` tied to
//! the group, kept in the timezone of the member who asked, and when it is
//! due Beep posts its message into the group and wakes every member. They
//! stay out of the members' own routines.

use super::group_tools::GroupRun;
use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};
use chrono_tz::Tz;
use flashmind::{
    cron::{CronJob, CronSchedule, JobSchedule},
    types::{
        ToolRegistry,
        tool::{Tool, ToolContext, ToolResult},
    },
};
use serde_json::{Value, json};
use std::sync::Arc;
use uuid::Uuid;

/// The most reminders one group keeps.
const MAX_REMINDERS: usize = 20;
/// The longest reminder message.
const MAX_MESSAGE: usize = 1000;

struct SetTool {
    run: Arc<GroupRun>,
}

struct ListTool {
    run: Arc<GroupRun>,
}

struct DeleteTool {
    run: Arc<GroupRun>,
}

pub(super) fn register(registry: &mut ToolRegistry, run: Arc<GroupRun>) {
    registry.register(Arc::new(SetTool { run: run.clone() }));
    registry.register(Arc::new(ListTool { run: run.clone() }));
    registry.register(Arc::new(DeleteTool { run }));
}

fn text<'a>(args: &'a Value, name: &str) -> Option<&'a str> {
    args.get(name)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|v| !v.is_empty())
}

fn failure(ctx: &ToolContext<'_>, message: impl Into<String>) -> Result<ToolResult> {
    Ok(ToolResult::failure(ctx.tool_call_id, message.into()))
}

fn success(ctx: &ToolContext<'_>, output: Value) -> Result<ToolResult> {
    Ok(ToolResult::success(ctx.tool_call_id, output.to_string()))
}

impl GroupRun {
    /// The timezone of the member who asked, UTC when they never set one.
    async fn timezone(&self) -> Tz {
        self.actor
            .user
            .get_timezone()
            .await
            .ok()
            .and_then(|tz| tz.parse().ok())
            .unwrap_or(Tz::UTC)
    }
}

/// A one-off time: local in `tz`, or with its own offset.
fn local_time(at: &str, tz: Tz) -> Option<DateTime<Utc>> {
    if let Ok(at) = DateTime::parse_from_rfc3339(at) {
        return Some(at.with_timezone(&Utc));
    }
    let naive = ["%Y-%m-%dT%H:%M:%S", "%Y-%m-%dT%H:%M", "%Y-%m-%d %H:%M"]
        .iter()
        .find_map(|format| NaiveDateTime::parse_from_str(at, format).ok())?;
    tz.from_local_datetime(&naive)
        .earliest()
        .map(|at| at.with_timezone(&Utc))
}

/// When `job` fires, in `tz`, as the model reads it back.
fn when(job: &CronJob, tz: Tz) -> String {
    match &job.schedule {
        JobSchedule::Once(at) => {
            format!("once at {}", at.with_timezone(&tz).format("%Y-%m-%d %H:%M"))
        }
        JobSchedule::Cron(expr) => format!("repeats on cron \"{expr}\""),
        JobSchedule::OnWake { .. } => "on wake".to_owned(),
    }
}

#[async_trait]
impl Tool for SetTool {
    fn name(&self) -> &str {
        "reminder_set"
    }
    fn description(&self) -> &str {
        "Set a reminder for the whole group. When it is due you post `message` into this chat and every member is notified. Give `at` for a one-off or `repeat` for a repeating one, never both. Times are the asking member's local time."
    }
    fn parameters(&self) -> Value {
        json!({"type":"object","properties":{
            "message":{"type":"string","description":"What you post to the group when it is due, written to them in one or two short sentences, like \"Check-out is at 11, leave the keys at the desk.\""},
            "label":{"type":"string","description":"A few words naming it, like \"Hotel check-out\"."},
            "at":{"type":"string","description":"For a one-off: the local date and time, like 2026-10-05T09:30."},
            "repeat":{"type":"string","description":"For a repeating one: a 5-field cron expression in local time, like \"0 9 * * 1-5\"."}
        },"required":["message","label"],"additionalProperties":false})
    }
    async fn execute(&self, ctx: ToolContext<'_>) -> Result<ToolResult> {
        let Some(message) = text(&ctx.args, "message") else {
            return failure(&ctx, "Write the message to post.");
        };
        if message.chars().count() > MAX_MESSAGE {
            return failure(&ctx, "Keep the message shorter.");
        }
        let label = text(&ctx.args, "label").unwrap_or("Reminder");
        let tz = self.run.timezone().await;
        let (schedule, once) = match (text(&ctx.args, "at"), text(&ctx.args, "repeat")) {
            (Some(at), None) => match local_time(at, tz) {
                Some(at) if at > Utc::now() => (JobSchedule::Once(at), true),
                Some(_) => return failure(&ctx, "That time has already passed."),
                None => return failure(&ctx, "Give `at` as a local time like 2026-10-05T09:30."),
            },
            (None, Some(expr)) => match CronSchedule::parse(expr) {
                Ok(_) => (JobSchedule::Cron(expr.to_owned()), false),
                Err(error) => return failure(&ctx, format!("Not a cron expression: {error}")),
            },
            _ => return failure(&ctx, "Give exactly one of `at` or `repeat`."),
        };
        let store = &self.run.app.store;
        if store.group_reminders(self.run.group_id).await?.len() >= MAX_REMINDERS {
            return failure(&ctx, "The group has too many reminders. Delete one first.");
        }
        if self.run.leaks(message).await {
            return failure(
                &ctx,
                "That message reveals something private about a member. Write it without that.",
            );
        }
        let job = CronJob {
            id: Uuid::new_v4(),
            schedule,
            task: message.to_owned(),
            enabled: true,
            once,
            metadata: json!({
                "user_id": self.run.actor.user.id(),
                "label": label,
                "group_id": self.run.group_id,
            }),
            created_at: Utc::now(),
            last_run: None,
        };
        store.upsert_cron_job(&job).await?;
        store.notify_crons_changed();
        success(
            &ctx,
            json!({"set": true, "id": job.id, "when": when(&job, tz)}),
        )
    }
}

#[async_trait]
impl Tool for ListTool {
    fn name(&self) -> &str {
        "reminder_list"
    }
    fn description(&self) -> &str {
        "List the group's reminders, with their ids for reminder_delete."
    }
    fn parameters(&self) -> Value {
        json!({"type":"object","properties":{},"additionalProperties":false})
    }
    async fn execute(&self, ctx: ToolContext<'_>) -> Result<ToolResult> {
        let tz = self.run.timezone().await;
        let jobs = self
            .run
            .app
            .store
            .group_reminders(self.run.group_id)
            .await?;
        let list: Vec<Value> = jobs
            .iter()
            .filter(|job| job.enabled)
            .map(|job| {
                json!({
                    "id": job.id,
                    "label": job.metadata.get("label"),
                    "message": job.task,
                    "when": when(job, tz),
                })
            })
            .collect();
        success(&ctx, Value::Array(list))
    }
}

#[async_trait]
impl Tool for DeleteTool {
    fn name(&self) -> &str {
        "reminder_delete"
    }
    fn description(&self) -> &str {
        "Delete one of the group's reminders by the id reminder_list gave."
    }
    fn parameters(&self) -> Value {
        json!({"type":"object","properties":{
            "id":{"type":"string"}
        },"required":["id"],"additionalProperties":false})
    }
    async fn execute(&self, ctx: ToolContext<'_>) -> Result<ToolResult> {
        let Some(id) = text(&ctx.args, "id").and_then(|id| id.parse::<Uuid>().ok()) else {
            return failure(&ctx, "Give the id from reminder_list.");
        };
        let deleted = self
            .run
            .app
            .store
            .delete_group_reminder(self.run.group_id, id)
            .await?;
        if !deleted {
            return failure(&ctx, "No reminder in this group has that id.");
        }
        success(&ctx, json!({"deleted": true}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_local_time_is_read_in_the_members_timezone() {
        let tz: Tz = "Asia/Tokyo".parse().expect("known timezone");
        let at = local_time("2026-10-05T09:30", tz).expect("parses");
        assert_eq!(at.to_rfc3339(), "2026-10-05T00:30:00+00:00");
        let at = local_time("2026-10-05T09:30:00+02:00", tz).expect("parses");
        assert_eq!(at.to_rfc3339(), "2026-10-05T07:30:00+00:00");
        assert!(local_time("tomorrow at 9", tz).is_none());
    }
}
