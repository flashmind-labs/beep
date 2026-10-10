use crate::user::User;
use async_trait::async_trait;
use flashmind::types::tool::{Tool, ToolContext, ToolResult};
use serde_json::{Value, json};
use sqlx::Error as SqlError;
use std::sync::Arc;

/// Which of the habit tools.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HabitToolKind {
    Create,
    Log,
    Status,
    Delete,
}

impl HabitToolKind {
    pub(crate) const ALL: [Self; 4] = [Self::Create, Self::Log, Self::Status, Self::Delete];

    fn name(self) -> &'static str {
        match self {
            Self::Create => "habit_create",
            Self::Log => "habit_log",
            Self::Status => "habit_status",
            Self::Delete => "habit_delete",
        }
    }
}

pub(crate) struct HabitTool {
    pub user: Arc<User>,
    pub kind: HabitToolKind,
}

/// Whether `error` is the database refusing a second habit of the same name.
fn is_duplicate(error: &anyhow::Error) -> bool {
    matches!(
        error.downcast_ref::<SqlError>(),
        Some(SqlError::Database(error)) if error.is_unique_violation()
    )
}

#[async_trait]
impl Tool for HabitTool {
    fn name(&self) -> &str {
        self.kind.name()
    }
    fn description(&self) -> &str {
        match self.kind {
            HabitToolKind::Create => {
                "Create a new habit to track. Frequency can be daily, weekly, or weekdays."
            }
            HabitToolKind::Log => "Log a habit completion. Looks up the habit by name.",
            HabitToolKind::Status => {
                "Get status and recent logs for a specific habit, including streak and completion rate."
            }
            HabitToolKind::Delete => "Archive (soft-delete) a habit by name.",
        }
    }
    fn parameters(&self) -> Value {
        match self.kind {
            HabitToolKind::Create => json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Habit name, e.g. 'Exercise'" },
                    "frequency": { "type": "string", "enum": ["daily", "weekly", "weekdays"], "description": "How often. Default: daily" }
                },
                "required": ["name"],
                "additionalProperties": false
            }),
            HabitToolKind::Log => json!({
                "type": "object",
                "properties": {
                    "habit": { "type": "string", "description": "Habit name" },
                    "value": { "type": "number", "description": "Amount or count. Default 1" },
                    "note": { "type": "string", "description": "Optional note" }
                },
                "required": ["habit"],
                "additionalProperties": false
            }),
            HabitToolKind::Status => json!({
                "type": "object",
                "properties": {
                    "habit": { "type": "string", "description": "Habit name. Omit to list all habits." },
                    "days": { "type": "integer", "description": "How many days back to look. Default 7" }
                },
                "additionalProperties": false
            }),
            HabitToolKind::Delete => json!({
                "type": "object",
                "properties": {
                    "habit": { "type": "string", "description": "Habit name to archive" }
                },
                "required": ["habit"],
                "additionalProperties": false
            }),
        }
    }
    async fn execute(&self, ctx: ToolContext<'_>) -> anyhow::Result<ToolResult> {
        let output = match self.kind {
            HabitToolKind::Create => {
                let name = ctx.args.get("name").and_then(Value::as_str).unwrap_or("");
                let frequency = ctx
                    .args
                    .get("frequency")
                    .and_then(Value::as_str)
                    .unwrap_or("daily");
                if name.is_empty() {
                    return Ok(ToolResult::failure(ctx.tool_call_id, "name is required"));
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
            HabitToolKind::Log => {
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
            HabitToolKind::Status => {
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
            HabitToolKind::Delete => {
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
mod tests {
    use super::*;
    use crate::{store::TestDb, user::UserKey};

    #[tokio::test]
    async fn a_second_habit_of_the_same_name_is_a_duplicate() {
        let db = TestDb::new().await.expect("test database");
        let store = db.store().await.expect("store");
        let alice = UserKey::for_test("alice");
        store
            .create_habit(&alice, "Run", "daily")
            .await
            .expect("first habit");
        let again = store
            .create_habit(&alice, "Run", "weekly")
            .await
            .expect_err("same name");
        assert!(is_duplicate(&again), "{again:#}");
        let invalid = store
            .create_habit(&alice, "Swim", "hourly")
            .await
            .expect_err("bad frequency");
        assert!(!is_duplicate(&invalid));
    }
}
