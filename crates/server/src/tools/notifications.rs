use crate::store::Store;
use async_trait::async_trait;
use flashmind::types::tool::{Tool, ToolContext, ToolResult};
use serde_json::{Value, json};

const APP_FRIENDLY_NAMES: &[(&str, &str)] = &[
    ("com.whatsapp", "WhatsApp"),
    ("com.google.android.gm", "Gmail"),
    ("com.google.android.apps.messaging", "Messages"),
    ("com.google.android.apps.maps", "Maps"),
    ("com.google.android.youtube", "YouTube"),
    ("com.google.android.calendar", "Calendar"),
    ("com.google.android.dialer", "Phone"),
    ("com.spotify.music", "Spotify"),
    ("com.instagram.android", "Instagram"),
    ("com.facebook.orca", "Messenger"),
    ("com.facebook.katana", "Facebook"),
    ("com.twitter.android", "X"),
    ("com.Slack", "Slack"),
    ("org.telegram.messenger", "Telegram"),
    ("com.discord", "Discord"),
    ("com.google.android.apps.docs", "Google Docs"),
    ("com.android.chrome", "Chrome"),
];

fn friendly_app_name(package: &str) -> String {
    APP_FRIENDLY_NAMES
        .iter()
        .find(|(pkg, _)| *pkg == package)
        .map(|(_, name)| (*name).to_owned())
        .unwrap_or_else(|| package.to_owned())
}

pub(crate) struct GetNotificationsTool {
    pub store: Store,
    pub user: String,
}

#[async_trait]
impl Tool for GetNotificationsTool {
    fn name(&self) -> &str {
        "get_notifications"
    }
    fn description(&self) -> &str {
        "Query recent notifications from the user's phone. Returns notifications from all apps unless filtered."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "app": {
                    "type": "string",
                    "description": "Filter by app name or package (e.g. 'WhatsApp', 'Gmail', 'com.whatsapp')"
                },
                "minutes": {
                    "type": "integer",
                    "description": "How far back to look in minutes. Default 60."
                },
                "keyword": {
                    "type": "string",
                    "description": "Search in notification title and text"
                }
            },
            "additionalProperties": false
        })
    }
    async fn execute(&self, ctx: ToolContext<'_>) -> anyhow::Result<ToolResult> {
        let app_filter = ctx.args.get("app").and_then(Value::as_str);
        let minutes = ctx.args.get("minutes").and_then(Value::as_i64);
        let keyword = ctx.args.get("keyword").and_then(Value::as_str);

        let resolved_app: Option<String> = app_filter.map(|f| {
            APP_FRIENDLY_NAMES
                .iter()
                .find(|(_, name)| name.to_lowercase() == f.to_lowercase())
                .map(|(pkg, _)| (*pkg).to_owned())
                .unwrap_or_else(|| f.to_owned())
        });

        let mut notifications = self
            .store
            .query_notifications(
                &self.user,
                resolved_app.as_deref(),
                minutes,
                keyword,
                Some(100),
            )
            .await?;

        for n in &mut notifications {
            if let Some(pkg) = n.get("app").and_then(Value::as_str) {
                let friendly = friendly_app_name(pkg);
                if friendly != pkg {
                    n["app_name"] = json!(friendly);
                }
            }
        }

        let output = json!({
            "notifications": notifications,
            "count": notifications.len(),
        });
        Ok(ToolResult::success(ctx.tool_call_id, output.to_string()))
    }
}
