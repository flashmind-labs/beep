use async_trait::async_trait;
use flashmind::types::tool::{Tool, ToolContext, ToolResult};
use serde_json::{Value, json};

pub(crate) struct SetAlarmTool;

#[async_trait]
impl Tool for SetAlarmTool {
    fn name(&self) -> &str {
        "set_alarm"
    }
    fn description(&self) -> &str {
        "Set an alarm in the phone's Clock app at a time of day in the user's local time. An alarm without days rings at the next time that clock time comes round, so 8:00 asked for at night rings tomorrow morning. Use this for alarms and wake-ups, not open_app or a reminder. The phone never reports back, so say you are setting it, never that it is set."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "hour": { "type": "integer", "minimum": 0, "maximum": 23, "description": "24-hour clock" },
                "minute": { "type": "integer", "minimum": 0, "maximum": 59 },
                "label": { "type": "string", "description": "Optional short name shown when it rings" },
                "days": {
                    "type": "array",
                    "description": "Only for a repeating alarm: the weekdays it rings on",
                    "items": { "type": "string", "enum": DAYS }
                }
            },
            "required": ["hour", "minute"],
            "additionalProperties": false
        })
    }
    async fn execute(&self, ctx: ToolContext<'_>) -> anyhow::Result<ToolResult> {
        let hour = ctx.args.get("hour").and_then(Value::as_u64);
        let minute = ctx.args.get("minute").and_then(Value::as_u64);
        let (Some(hour @ 0..=23), Some(minute @ 0..=59)) = (hour, minute) else {
            return Ok(ToolResult::failure(
                ctx.tool_call_id,
                "hour must be 0 to 23 and minute 0 to 59.".to_owned(),
            ));
        };
        let label = ctx.args.get("label").and_then(Value::as_str).unwrap_or("");
        let days: Vec<&str> = ctx
            .args
            .get("days")
            .and_then(Value::as_array)
            .map(|list| {
                list.iter()
                    .filter_map(Value::as_str)
                    .filter(|day| DAYS.contains(day))
                    .collect()
            })
            .unwrap_or_default();

        let output = json!({
            "action": "set_alarm",
            "hour": hour,
            "minute": minute,
            "label": label,
            "days": days,
            "status": "sent to the phone, not confirmed",
        });
        Ok(ToolResult::success(ctx.tool_call_id, output.to_string()))
    }
}

const DAYS: [&str; 7] = [
    "monday",
    "tuesday",
    "wednesday",
    "thursday",
    "friday",
    "saturday",
    "sunday",
];
