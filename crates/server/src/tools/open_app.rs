use async_trait::async_trait;
use flashmind::types::tool::{Tool, ToolContext, ToolResult};
use serde_json::{Value, json};

const APPS: &str =
    "spotify, youtube, whatsapp, maps, phone, clock, browser, chrome, gmail, calendar";

pub(crate) struct OpenAppTool;

#[async_trait]
impl Tool for OpenAppTool {
    fn name(&self) -> &str {
        "open_app"
    }
    fn description(&self) -> &str {
        "Ask the user's Android phone to open an app. The phone gets the request but never reports back whether the app opened, so say you are opening it, never that it opened."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "app": {
                    "type": "string",
                    "enum": APPS.split(", ").collect::<Vec<_>>()
                },
                "action": {
                    "type": "string",
                    "description": "What to do, e.g. 'play song X', 'call X'"
                },
                "uri": {
                    "type": "string",
                    "description": "Optional deep link, only one you know is valid for the app. Never guess a path. For Maps: geo:0,0?q=Dubai+Mall for a place, https://maps.google.com/location-sharing for people sharing their location."
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

        let Some(target) = target(app) else {
            return Ok(ToolResult::failure(
                ctx.tool_call_id,
                format!("Unknown app {app:?}. Supported: {APPS}."),
            ));
        };

        let uri = uri_override.unwrap_or(target.uri);

        let output = json!({
            "action": "open_app",
            "app": app,
            "intent_action": target.intent_action,
            "uri": uri,
            "fallback_url": target.fallback_url,
            "package": target.package,
            "requested_action": action,
            "status": "sent to the phone, not confirmed",
        });
        Ok(ToolResult::success(ctx.tool_call_id, output.to_string()))
    }
}

/// How the phone opens an app.
struct Target {
    intent_action: &'static str,
    uri: &'static str,
    fallback_url: &'static str,
    package: &'static str,
}

fn target(app: &str) -> Option<Target> {
    Some(match app.to_lowercase().as_str() {
        "spotify" => Target {
            intent_action: "android.intent.action.VIEW",
            uri: "spotify:",
            fallback_url: "https://open.spotify.com/",
            package: "com.spotify.music",
        },
        "youtube" => Target {
            intent_action: "android.intent.action.VIEW",
            uri: "vnd.youtube:",
            fallback_url: "https://www.youtube.com/",
            package: "com.google.android.youtube",
        },
        "whatsapp" => Target {
            intent_action: "android.intent.action.VIEW",
            uri: "whatsapp://send",
            fallback_url: "https://wa.me/",
            package: "com.whatsapp",
        },
        "maps" => Target {
            intent_action: "android.intent.action.VIEW",
            uri: "geo:0,0",
            fallback_url: "https://maps.google.com/",
            package: "com.google.android.apps.maps",
        },
        "phone" => Target {
            intent_action: "android.intent.action.DIAL",
            uri: "tel:",
            fallback_url: "tel:",
            package: "com.android.dialer",
        },
        "clock" => Target {
            intent_action: "android.intent.action.SET_ALARM",
            uri: "clock:",
            fallback_url: "",
            package: "com.google.android.deskclock",
        },
        "browser" | "chrome" => Target {
            intent_action: "android.intent.action.VIEW",
            uri: "https://www.google.com/",
            fallback_url: "https://www.google.com/",
            package: "com.android.chrome",
        },
        "gmail" => Target {
            intent_action: "android.intent.action.VIEW",
            uri: "mailto:",
            fallback_url: "https://mail.google.com/",
            package: "com.google.android.gm",
        },
        "calendar" => Target {
            intent_action: "android.intent.action.VIEW",
            uri: "content://com.android.calendar/time/",
            fallback_url: "https://calendar.google.com/",
            package: "com.google.android.calendar",
        },
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_listed_app_opens() {
        for app in APPS.split(", ") {
            assert!(target(app).is_some(), "{app} has no target");
        }
    }
}
