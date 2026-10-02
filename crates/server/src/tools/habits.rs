use crate::store::Store;
use async_trait::async_trait;
use flashmind::types::tool::{Tool, ToolContext, ToolResult};
use serde_json::{Value, json};

pub(crate) struct HabitTool {
    pub store: Store,
    pub user: String,
    pub variant: &'static str,
}

#[async_trait]
impl Tool for HabitTool {
    fn name(&self) -> &str {
        self.variant
    }
    fn description(&self) -> &str {
        match self.variant {
            "habit_create" => {
                "Create a new habit to track. Frequency can be daily, weekly, or weekdays."
            }
            "habit_log" => "Log a habit completion. Looks up the habit by name.",
            "habit_status" => {
                "Get status and recent logs for a specific habit, including streak and completion rate."
            }
            "habit_delete" => "Archive (soft-delete) a habit by name.",
            _ => "",
        }
    }
    fn parameters(&self) -> Value {
        match self.variant {
            "habit_create" => json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Habit name, e.g. 'Exercise'" },
                    "frequency": { "type": "string", "enum": ["daily", "weekly", "weekdays"], "description": "How often. Default: daily" }
                },
                "required": ["name"],
                "additionalProperties": false
            }),
            "habit_log" => json!({
                "type": "object",
                "properties": {
                    "habit": { "type": "string", "description": "Habit name" },
                    "value": { "type": "number", "description": "Amount or count. Default 1" },
                    "note": { "type": "string", "description": "Optional note" }
                },
                "required": ["habit"],
                "additionalProperties": false
            }),
            "habit_status" => json!({
                "type": "object",
                "properties": {
                    "habit": { "type": "string", "description": "Habit name. Omit to list all habits." },
                    "days": { "type": "integer", "description": "How many days back to look. Default 7" }
                },
                "additionalProperties": false
            }),
            "habit_delete" => json!({
                "type": "object",
                "properties": {
                    "habit": { "type": "string", "description": "Habit name to archive" }
                },
                "required": ["habit"],
                "additionalProperties": false
            }),
            _ => json!({}),
        }
    }
    async fn execute(&self, ctx: ToolContext<'_>) -> anyhow::Result<ToolResult> {
        let output = match self.variant {
            "habit_create" => {
                let name = ctx.args.get("name").and_then(Value::as_str).unwrap_or("");
                let frequency = ctx
                    .args
                    .get("frequency")
                    .and_then(Value::as_str)
                    .unwrap_or("daily");
                if name.is_empty() {
                    return Ok(ToolResult::failure(ctx.tool_call_id, "name is required"));
                }
                match self.store.create_habit(&self.user, name, frequency).await {
                    Ok(_id) => json!({"created": true, "name": name, "frequency": frequency}),
                    Err(e) => {
                        let msg = e.to_string();
                        if msg.contains("UNIQUE") {
                            return Ok(ToolResult::failure(
                                ctx.tool_call_id,
                                format!("a habit named '{}' already exists", name),
                            ));
                        }
                        return Err(e);
                    }
                }
            }
            "habit_log" => {
                let habit = ctx.args.get("habit").and_then(Value::as_str).unwrap_or("");
                let value = ctx.args.get("value").and_then(Value::as_f64).unwrap_or(1.0);
                let note = ctx.args.get("note").and_then(Value::as_str);
                if habit.is_empty() {
                    return Ok(ToolResult::failure(
                        ctx.tool_call_id,
                        "habit name is required",
                    ));
                }
                match self.store.log_habit(&self.user, habit, value, note).await {
                    Ok(()) => json!({"logged": true, "habit": habit, "value": value}),
                    Err(e) => {
                        return Ok(ToolResult::failure(ctx.tool_call_id, e.to_string()));
                    }
                }
            }
            "habit_status" => {
                let habit = ctx.args.get("habit").and_then(Value::as_str);
                let days = ctx.args.get("days").and_then(Value::as_i64).unwrap_or(7);
                match habit {
                    Some(name) => self.store.habit_status(&self.user, name, days).await?,
                    None => {
                        let habits = self.store.list_habits(&self.user).await?;
                        json!({"habits": habits})
                    }
                }
            }
            "habit_delete" => {
                let habit = ctx.args.get("habit").and_then(Value::as_str).unwrap_or("");
                if habit.is_empty() {
                    return Ok(ToolResult::failure(
                        ctx.tool_call_id,
                        "habit name is required",
                    ));
                }
                let archived = self.store.archive_habit(&self.user, habit).await?;
                json!({"archived": archived, "habit": habit})
            }
            _ => json!({}),
        };
        Ok(ToolResult::success(ctx.tool_call_id, output.to_string()))
    }
}
