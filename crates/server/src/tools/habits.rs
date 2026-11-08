use crate::{account::user::User, store::DuplicateHabit};
use async_trait::async_trait;
use flashmind::types::tool::{Tool, ToolContext, ToolResult};
use serde_json::{Value, json};
use std::sync::Arc;

/// What a `habit` call does, by its `action`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::EnumString)]
#[strum(serialize_all = "snake_case")]
enum Action {
    Create,
    Log,
    Status,
    Delete,
}

/// `habit`: the habits the user tracks.
pub(crate) struct HabitTool {
    pub user: Arc<User>,
}

/// Whether `error` is the store refusing a second habit of the same name.
fn is_duplicate(error: &anyhow::Error) -> bool {
    error.is::<DuplicateHabit>()
}

fn action(args: &Value) -> Option<Action> {
    args.get("action")?.as_str()?.parse().ok()
}

#[async_trait]
impl Tool for HabitTool {
    fn name(&self) -> &str {
        "habit"
    }
    fn description(&self) -> &str {
        "Habits the user tracks. `create` starts tracking one (frequency daily, weekly or weekdays), \
         `log` records a completion, `status` gives one habit's streak, completion rate and recent \
         logs, or lists every habit without a name, and `delete` archives one. Habits are found by \
         name."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": { "type": "string", "enum": ["create", "log", "status", "delete"] },
                "habit": { "type": "string", "description": "The habit's name, e.g. 'Exercise'." },
                "frequency": { "type": "string", "enum": ["daily", "weekly", "weekdays"], "description": "create: how often. Default daily." },
                "value": { "type": "number", "description": "log: amount or count. Default 1." },
                "note": { "type": "string", "description": "log: an optional note." },
                "days": { "type": "integer", "description": "status: how many days back to look. Default 7." }
            },
            "required": ["action"],
            "additionalProperties": false
        })
    }
    fn humanize(&self, args: &Value) -> String {
        match action(args) {
            Some(Action::Create) => "Creating a habit",
            Some(Action::Log) => "Logging a habit",
            Some(Action::Delete) => "Deleting a habit",
            Some(Action::Status) | None => "Checking habits",
        }
        .to_owned()
    }
    async fn execute(&self, ctx: ToolContext<'_>) -> anyhow::Result<ToolResult> {
        let Some(action) = action(&ctx.args) else {
            return Ok(ToolResult::failure(
                ctx.tool_call_id,
                "Give `action`: create, log, status or delete.",
            ));
        };
        let output = match action {
            Action::Create => {
                let name = ctx.args.get("habit").and_then(Value::as_str).unwrap_or("");
                let frequency = ctx
                    .args
                    .get("frequency")
                    .and_then(Value::as_str)
                    .unwrap_or("daily");
                if name.is_empty() {
                    return Ok(ToolResult::failure(ctx.tool_call_id, "habit is required"));
                }
                match self.user.create_habit(name, frequency).await {
                    Ok(_id) => json!({"created": true, "name": name, "frequency": frequency}),
                    Err(e) if is_duplicate(&e) => {
                        return Ok(ToolResult::failure(
                            ctx.tool_call_id,
                            format!("a habit named '{}' already exists", name),
                        ));
                    }
                    Err(e) => return Err(e),
                }
            }
            Action::Log => {
                let habit = ctx.args.get("habit").and_then(Value::as_str).unwrap_or("");
                let value = ctx.args.get("value").and_then(Value::as_f64).unwrap_or(1.0);
                let note = ctx.args.get("note").and_then(Value::as_str);
                if habit.is_empty() {
                    return Ok(ToolResult::failure(
                        ctx.tool_call_id,
                        "habit name is required",
                    ));
                }
                match self.user.log_habit(habit, value, note).await {
                    Ok(()) => json!({"logged": true, "habit": habit, "value": value}),
                    Err(e) => {
                        return Ok(ToolResult::failure(ctx.tool_call_id, e.to_string()));
                    }
                }
            }
            Action::Status => {
                let habit = ctx.args.get("habit").and_then(Value::as_str);
                let days = ctx.args.get("days").and_then(Value::as_i64).unwrap_or(7);
                match habit {
                    Some(name) => self.user.habit_status(name, days).await?,
                    None => {
                        let habits = self.user.list_habits().await?;
                        json!({"habits": habits})
                    }
                }
            }
            Action::Delete => {
                let habit = ctx.args.get("habit").and_then(Value::as_str).unwrap_or("");
                if habit.is_empty() {
                    return Ok(ToolResult::failure(
                        ctx.tool_call_id,
                        "habit name is required",
                    ));
                }
                let archived = self.user.archive_habit(habit).await?;
                json!({"archived": archived, "habit": habit})
            }
        };
        Ok(ToolResult::success(ctx.tool_call_id, output.to_string()))
    }
}

#[cfg(test)]
#[path = "../tests/tools_habits.rs"]
mod tests;
