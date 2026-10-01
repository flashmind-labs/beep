use crate::store::Store;
use async_trait::async_trait;
use flashmind::types::{
    ToolRegistry,
    tool::{Tool, ToolContext, ToolResult},
};
use serde_json::{Value, json};
use std::sync::Arc;

struct MemoryTool {
    store: Store,
    user: String,
    name: &'static str,
}

#[async_trait]
impl Tool for MemoryTool {
    fn name(&self) -> &str {
        self.name
    }
    fn description(&self) -> &str {
        match self.name {
            "memory_store" => {
                "Remember or update a durable user fact by stable key. Never store secrets."
            }
            "memory_recall" => {
                "Find the user's memories by literal substring. Empty query lists up to 50 memories."
            }
            "memory_forget" => "Forget a user memory by its exact key.",
            _ => "Get the current UTC date and time.",
        }
    }
    fn parameters(&self) -> Value {
        match self.name {
            "memory_store" => {
                json!({"type":"object","properties":{"key":{"type":"string"},"content":{"type":"string"}},"required":["key","content"],"additionalProperties":false})
            }
            "memory_recall" => {
                json!({"type":"object","properties":{"query":{"type":"string"}},"required":["query"],"additionalProperties":false})
            }
            "memory_forget" => {
                json!({"type":"object","properties":{"key":{"type":"string"}},"required":["key"],"additionalProperties":false})
            }
            _ => json!({"type":"object","properties":{},"additionalProperties":false}),
        }
    }
    async fn execute(&self, ctx: ToolContext<'_>) -> anyhow::Result<ToolResult> {
        let arg = |name| {
            ctx.args
                .get(name)
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow::anyhow!("missing string argument: {name}"))
        };
        let output = match self.name {
            "memory_store" => {
                self.store
                    .remember(&self.user, arg("key")?, arg("content")?)
                    .await?;
                json!({"stored":true})
            }
            "memory_recall" => self.store.recall(&self.user, arg("query")?).await?,
            "memory_forget" => {
                json!({"forgotten":self.store.forget(&self.user,arg("key")?).await?})
            }
            _ => json!({"utc":chrono::Utc::now().to_rfc3339()}),
        };
        Ok(ToolResult::success(ctx.tool_call_id, output.to_string()))
    }
}

struct OpenAppTool;

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
                    "description": "App name: spotify, youtube, maps, whatsapp, phone, clock, browser, chrome, gmail, calendar"
                },
                "action": {
                    "type": "string",
                    "description": "What to do, e.g. 'play song X', 'navigate to X', 'call X'"
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

        let (intent_action, default_uri, fallback_url, package) = match app.to_lowercase().as_str() {
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
            "maps" => (
                "android.intent.action.VIEW",
                "geo:0,0?q=",
                "https://maps.google.com/",
                "com.google.android.apps.maps",
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
            _ => (
                "android.intent.action.VIEW",
                "",
                "",
                "",
            ),
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

struct GetLocationTool {
    store: Store,
    user: String,
}

#[async_trait]
impl Tool for GetLocationTool {
    fn name(&self) -> &str {
        "get_location"
    }
    fn description(&self) -> &str {
        "Get the user's last known location (latitude, longitude). Requires the user to have enabled location sharing on their device."
    }
    fn parameters(&self) -> Value {
        json!({"type":"object","properties":{},"additionalProperties":false})
    }
    async fn execute(&self, ctx: ToolContext<'_>) -> anyhow::Result<ToolResult> {
        let output = match self.store.get_location(&self.user).await? {
            Some(loc) => loc,
            None => json!({
                "message": "Location data will be available when the user enables it on their device"
            }),
        };
        Ok(ToolResult::success(ctx.tool_call_id, output.to_string()))
    }
}

struct OpenBrowserTool;

#[async_trait]
impl Tool for OpenBrowserTool {
    fn name(&self) -> &str {
        "open_browser"
    }
    fn description(&self) -> &str {
        "Open a URL in the in-app WebView on the user's Android device. Optionally extract page content."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "The URL to open"
                },
                "extract_content": {
                    "type": "boolean",
                    "description": "Whether to read the page content back"
                }
            },
            "required": ["url"],
            "additionalProperties": false
        })
    }
    async fn execute(&self, ctx: ToolContext<'_>) -> anyhow::Result<ToolResult> {
        let url = ctx.args.get("url").and_then(Value::as_str).unwrap_or("");
        let extract_content = ctx.args.get("extract_content").and_then(Value::as_bool).unwrap_or(false);

        let output = json!({
            "action": "open_browser",
            "url": url,
            "extract_content": extract_content,
        });
        Ok(ToolResult::success(ctx.tool_call_id, output.to_string()))
    }
}

pub fn registry(store: Store, user: &str) -> ToolRegistry {
    let mut registry = ToolRegistry::new();
    for name in ["memory_store", "memory_recall", "memory_forget", "get_time"] {
        registry.register(Arc::new(MemoryTool {
            store: store.clone(),
            user: user.into(),
            name,
        }));
    }
    registry.register(Arc::new(OpenAppTool));
    registry.register(Arc::new(GetLocationTool {
        store: store.clone(),
        user: user.into(),
    }));
    registry.register(Arc::new(OpenBrowserTool));
    registry
}
