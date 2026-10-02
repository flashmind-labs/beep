use async_trait::async_trait;
use flashmind::types::tool::{Tool, ToolContext, ToolResult};
use serde_json::{Value, json};

pub(crate) struct OpenAppTool;

#[async_trait]
impl Tool for OpenAppTool {
    fn name(&self) -> &str {
        "open_app"
    }
    fn description(&self) -> &str {
        "Open an app on the user's Android phone with an optional action. Returns an intent for the client to execute."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "app": {
                    "type": "string",
                    "description": "App name: spotify, youtube, whatsapp, phone, clock, browser, chrome, gmail, calendar"
                },
                "action": {
                    "type": "string",
                    "description": "What to do, e.g. 'play song X', 'call X'"
                },
                "uri": {
                    "type": "string",
                    "description": "Optional deep link URI"
                }
            },
            "required": ["app", "action"],
            "additionalProperties": false
        })
    }
    async fn execute(&self, ctx: ToolContext<'_>) -> anyhow::Result<ToolResult> {
        let app = ctx.args.get("app").and_then(Value::as_str).unwrap_or("");
        let action = ctx.args.get("action").and_then(Value::as_str).unwrap_or("");
        let uri_override = ctx.args.get("uri").and_then(Value::as_str);

        let (intent_action, default_uri, fallback_url, package) = match app.to_lowercase().as_str()
        {
            "spotify" => (
                "android.intent.action.VIEW",
                "spotify:",
                "https://open.spotify.com/",
                "com.spotify.music",
            ),
            "youtube" => (
                "android.intent.action.VIEW",
                "vnd.youtube:",
                "https://www.youtube.com/",
                "com.google.android.youtube",
            ),
            "whatsapp" => (
                "android.intent.action.VIEW",
                "whatsapp://send",
                "https://wa.me/",
                "com.whatsapp",
            ),
            "phone" => (
                "android.intent.action.DIAL",
                "tel:",
                "tel:",
                "com.android.dialer",
            ),
            "clock" => (
                "android.intent.action.SET_ALARM",
                "clock:",
                "",
                "com.google.android.deskclock",
            ),
            "browser" | "chrome" => (
                "android.intent.action.VIEW",
                "https://www.google.com/",
                "https://www.google.com/",
                "com.android.chrome",
            ),
            "gmail" => (
                "android.intent.action.VIEW",
                "mailto:",
                "https://mail.google.com/",
                "com.google.android.gm",
            ),
            "calendar" => (
                "android.intent.action.VIEW",
                "content://com.android.calendar/time/",
                "https://calendar.google.com/",
                "com.google.android.calendar",
            ),
            _ => ("android.intent.action.VIEW", "", "", ""),
        };

        let uri = uri_override.unwrap_or(default_uri);

        let output = json!({
            "action": "open_app",
            "app": app,
            "intent_action": intent_action,
            "uri": uri,
            "fallback_url": fallback_url,
            "package": package,
            "requested_action": action,
        });
        Ok(ToolResult::success(ctx.tool_call_id, output.to_string()))
    }
}
