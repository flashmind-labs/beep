use crate::app::App;
use crate::store::Store;
use async_trait::async_trait;
use flashmind::types::{
    ToolRegistry,
    tool::{Tool, ToolContext, ToolResult},
};
use serde_json::{Value, json};
use std::{str::FromStr, sync::Arc};

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
                "Remember or update a durable user fact by stable key. Never store secrets. Optionally assign a category."
            }
            "memory_recall" => {
                "Find the user's memories by literal substring. Empty query lists up to 50 memories. Optionally filter by category."
            }
            "memory_forget" => "Forget a user memory by its exact key.",
            "memory_update" => "Update an existing memory's content or category by its exact key.",
            _ => "Get the current UTC date and time.",
        }
    }
    fn parameters(&self) -> Value {
        match self.name {
            "memory_store" => {
                json!({"type":"object","properties":{
                    "key":{"type":"string"},
                    "content":{"type":"string"},
                    "category":{"type":"string","enum":["personal","preference","work","location","routine","general"],"description":"Memory category. Defaults to 'general'."}
                },"required":["key","content"],"additionalProperties":false})
            }
            "memory_recall" => {
                json!({"type":"object","properties":{
                    "query":{"type":"string"},
                    "category":{"type":"string","enum":["personal","preference","work","location","routine","general"],"description":"Filter by category."}
                },"required":["query"],"additionalProperties":false})
            }
            "memory_forget" => {
                json!({"type":"object","properties":{"key":{"type":"string"}},"required":["key"],"additionalProperties":false})
            }
            "memory_update" => {
                json!({"type":"object","properties":{
                    "key":{"type":"string","description":"The memory key to update."},
                    "content":{"type":"string","description":"The new content."},
                    "category":{"type":"string","enum":["personal","preference","work","location","routine","general"],"description":"Change the category."}
                },"required":["key","content"],"additionalProperties":false})
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
        let opt_arg = |name: &str| ctx.args.get(name).and_then(Value::as_str);
        let output = match self.name {
            "memory_store" => {
                self.store
                    .remember(
                        &self.user,
                        arg("key")?,
                        arg("content")?,
                        opt_arg("category"),
                    )
                    .await?;
                json!({"stored":true})
            }
            "memory_recall" => {
                self.store
                    .recall(&self.user, arg("query")?, opt_arg("category"))
                    .await?
            }
            "memory_forget" => {
                json!({"forgotten":self.store.forget(&self.user,arg("key")?).await?})
            }
            "memory_update" => {
                let updated = self
                    .store
                    .update_memory(
                        &self.user,
                        arg("key")?,
                        arg("content")?,
                        opt_arg("category"),
                    )
                    .await?;
                json!({"updated": updated})
            }
            _ => json!({"utc":chrono::Utc::now().to_rfc3339()}),
        };
        Ok(ToolResult::success(ctx.tool_call_id, output.to_string()))
    }
}

struct CronTool {
    store: Store,
    user: String,
    variant: &'static str,
}

#[async_trait]
impl Tool for CronTool {
    fn name(&self) -> &str {
        self.variant
    }
    fn description(&self) -> &str {
        match self.variant {
            "cron_create" => {
                "Create a scheduled automation. Uses standard 5-field POSIX cron (minute hour day-of-month month day-of-week) in the user's local timezone. Set once=true for one-time reminders that auto-disable after firing."
            }
            "cron_list" => {
                "List the user's existing scheduled automations with their IDs, labels, schedules, and enabled status."
            }
            "cron_delete" => "Delete a scheduled automation by its ID.",
            _ => "",
        }
    }
    fn parameters(&self) -> Value {
        match self.variant {
            "cron_create" => json!({
                "type": "object",
                "properties": {
                    "label": { "type": "string", "description": "Short name for the automation, e.g. 'Morning briefing'" },
                    "schedule": { "type": "string", "description": "5-field POSIX cron expression in user's local timezone, e.g. '0 9 * * *' for 9 AM daily, '30 14 * * *' for 2:30 PM daily, '0 9 5 10 *' for Oct 5 at 9 AM" },
                    "prompt": { "type": "string", "description": "The message Beep will process at each trigger" },
                    "once": { "type": "boolean", "description": "If true, this is a one-time reminder that auto-disables after firing. Default false." }
                },
                "required": ["label", "schedule", "prompt"],
                "additionalProperties": false
            }),
            "cron_list" => json!({"type":"object","properties":{},"additionalProperties":false}),
            "cron_delete" => json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "The cron automation ID (from cron_list)" }
                },
                "required": ["id"],
                "additionalProperties": false
            }),
            _ => json!({}),
        }
    }
    async fn execute(&self, ctx: ToolContext<'_>) -> anyhow::Result<ToolResult> {
        let output = match self.variant {
            "cron_create" => {
                let label = ctx.args.get("label").and_then(Value::as_str).unwrap_or("");
                let schedule = ctx
                    .args
                    .get("schedule")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let prompt = ctx.args.get("prompt").and_then(Value::as_str).unwrap_or("");
                let once = ctx
                    .args
                    .get("once")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                if label.is_empty() || schedule.is_empty() || prompt.is_empty() {
                    return Ok(ToolResult::failure(
                        ctx.tool_call_id,
                        "label, schedule, and prompt are all required",
                    ));
                }
                // Validate the cron expression by converting to 7-field and parsing
                let seven = format!("0 {} *", schedule);
                if cron::Schedule::from_str(&seven).is_err() {
                    return Ok(ToolResult::failure(
                        ctx.tool_call_id,
                        "Invalid cron expression. Use 5-field POSIX format: minute hour day month weekday. Example: '0 9 * * *' for daily at 9 AM.",
                    ));
                }
                self.store
                    .create_cron_ex(&self.user, label, schedule, prompt, once)
                    .await?;
                json!({"created": true, "label": label, "schedule": schedule, "once": once})
            }
            "cron_list" => {
                let crons = self.store.list_crons(&self.user).await?;
                json!({"crons": crons})
            }
            "cron_delete" => {
                let id = ctx.args.get("id").and_then(Value::as_str).unwrap_or("");
                let deleted = self.store.delete_cron(&self.user, id).await?;
                json!({"deleted": deleted})
            }
            _ => json!({}),
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

struct OpenMapsTool;

#[async_trait]
impl Tool for OpenMapsTool {
    fn name(&self) -> &str {
        "open_maps"
    }
    fn description(&self) -> &str {
        "Open Google Maps on the user's phone to search places, show a specific place, or get directions. Does not call any API; constructs an intent URI for the device."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["search", "directions", "place"],
                    "description": "search: search for places; place: show a named place; directions: get navigation directions"
                },
                "query": {
                    "type": "string",
                    "description": "Search query or place name, e.g. 'restaurants near me' or 'Lau Pa Sat Singapore'"
                },
                "latitude": {
                    "type": "number",
                    "description": "Latitude for place or directions destination"
                },
                "longitude": {
                    "type": "number",
                    "description": "Longitude for directions destination"
                },
                "travel_mode": {
                    "type": "string",
                    "enum": ["driving", "walking", "transit", "bicycling"],
                    "description": "Travel mode for directions"
                }
            },
            "required": ["action"],
            "additionalProperties": false
        })
    }
    async fn execute(&self, ctx: ToolContext<'_>) -> anyhow::Result<ToolResult> {
        let action = ctx.args.get("action").and_then(Value::as_str).unwrap_or("");
        let query = ctx.args.get("query").and_then(Value::as_str).unwrap_or("");
        let latitude = ctx.args.get("latitude").and_then(Value::as_f64);
        let longitude = ctx.args.get("longitude").and_then(Value::as_f64);
        let travel_mode = ctx
            .args
            .get("travel_mode")
            .and_then(Value::as_str)
            .unwrap_or("driving");

        let encoded_query = query.replace(' ', "+");

        let uri = match action {
            "search" => {
                if query.is_empty() {
                    return Ok(ToolResult::failure(
                        ctx.tool_call_id,
                        "query is required for search action",
                    ));
                }
                format!("geo:0,0?q={encoded_query}")
            }
            "place" => {
                if let (Some(lat), Some(lng)) = (latitude, longitude) {
                    format!("geo:{lat},{lng}?q={encoded_query}")
                } else if !query.is_empty() {
                    format!("geo:0,0?q={encoded_query}")
                } else {
                    return Ok(ToolResult::failure(
                        ctx.tool_call_id,
                        "query or latitude/longitude required for place action",
                    ));
                }
            }
            "directions" => {
                let dest = if let (Some(lat), Some(lng)) = (latitude, longitude) {
                    format!("{lat},{lng}")
                } else if !query.is_empty() {
                    encoded_query.clone()
                } else {
                    return Ok(ToolResult::failure(
                        ctx.tool_call_id,
                        "query or latitude/longitude required for directions",
                    ));
                };
                let mode_flag = match travel_mode {
                    "walking" => "w",
                    "transit" => "r",
                    "bicycling" => "b",
                    _ => "d",
                };
                format!("google.navigation:q={dest}&mode={mode_flag}")
            }
            _ => {
                return Ok(ToolResult::failure(
                    ctx.tool_call_id,
                    "action must be 'search', 'place', or 'directions'",
                ));
            }
        };

        let mut output = json!({
            "action": "open_maps",
            "uri": uri,
        });

        // Attach a place card when we have a named place or search
        if !query.is_empty() && (action == "search" || action == "place") {
            output["card"] = json!({
                "type": "card",
                "card_type": "place",
                "card_data": {
                    "name": query,
                    "query": query,
                }
            });
        }

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
        let extract_content = ctx
            .args
            .get("extract_content")
            .and_then(Value::as_bool)
            .unwrap_or(false);

        let output = json!({
            "action": "open_browser",
            "url": url,
            "extract_content": extract_content,
        });
        Ok(ToolResult::success(ctx.tool_call_id, output.to_string()))
    }
}

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

struct GetNotificationsTool {
    store: Store,
    user: String,
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

        // If the user specified a friendly name, resolve it to a package name for the DB query.
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

        // Replace package names with friendly names in the output
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

struct HabitTool {
    store: Store,
    user: String,
    variant: &'static str,
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

struct TaskProgressTool;

#[async_trait]
impl Tool for TaskProgressTool {
    fn name(&self) -> &str {
        "task_progress"
    }
    fn description(&self) -> &str {
        "Report progress on a multi-step task. Call this to show the user a visual checklist of steps and their statuses as you work through them."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "steps": {
                    "type": "array",
                    "description": "List of steps with their current status",
                    "items": {
                        "type": "object",
                        "properties": {
                            "label": { "type": "string", "description": "Short description of this step" },
                            "status": { "type": "string", "enum": ["pending", "in_progress", "completed", "failed"], "description": "Current status of this step" }
                        },
                        "required": ["label", "status"]
                    }
                }
            },
            "required": ["steps"],
            "additionalProperties": false
        })
    }
    async fn execute(&self, ctx: ToolContext<'_>) -> anyhow::Result<ToolResult> {
        let steps = ctx.args.get("steps").cloned().unwrap_or_else(|| json!([]));
        let output = json!({
            "type": "task_progress",
            "steps": steps,
        });
        Ok(ToolResult::success(ctx.tool_call_id, output.to_string()))
    }
}

struct BrowserTool {
    app: Arc<App>,
    run_id: String,
}

#[async_trait]
impl Tool for BrowserTool {
    fn name(&self) -> &str {
        "browser"
    }
    fn description(&self) -> &str {
        "Control the in-app browser on the user's phone. Navigate to URLs, click elements, type text, scroll, and read page content using CSS selectors."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["navigate", "click", "type", "scroll", "get_text", "get_page_info"],
                    "description": "The browser action to perform"
                },
                "url": {
                    "type": "string",
                    "description": "URL to navigate to (required for navigate)"
                },
                "selector": {
                    "type": "string",
                    "description": "CSS selector for the target element (for click, type, get_text)"
                },
                "text": {
                    "type": "string",
                    "description": "Text to type into the selected element (for type action)"
                },
                "direction": {
                    "type": "string",
                    "enum": ["up", "down"],
                    "description": "Scroll direction (for scroll action, default: down)"
                }
            },
            "required": ["action"],
            "additionalProperties": false
        })
    }
    fn timeout_secs(&self) -> Option<u64> {
        Some(20)
    }
    async fn execute(&self, ctx: ToolContext<'_>) -> anyhow::Result<ToolResult> {
        let action = ctx.args.get("action").and_then(Value::as_str).unwrap_or("");

        if action == "navigate" {
            let url = ctx.args.get("url").and_then(Value::as_str).unwrap_or("");
            if url.is_empty() {
                return Ok(ToolResult::failure(
                    ctx.tool_call_id,
                    "url is required for navigate action",
                ));
            }
        }

        if matches!(action, "click" | "type" | "get_text") {
            let selector = ctx
                .args
                .get("selector")
                .and_then(Value::as_str)
                .unwrap_or("");
            if selector.is_empty() {
                return Ok(ToolResult::failure(
                    ctx.tool_call_id,
                    "selector is required for click, type, and get_text actions",
                ));
            }
        }

        if action == "type" {
            let text = ctx.args.get("text").and_then(Value::as_str).unwrap_or("");
            if text.is_empty() {
                return Ok(ToolResult::failure(
                    ctx.tool_call_id,
                    "text is required for type action",
                ));
            }
        }

        let mut query = json!({ "action": "browser_command" });
        query["browser_action"] = json!(action);
        if let Some(v) = ctx.args.get("url") {
            query["url"] = v.clone();
        }
        if let Some(v) = ctx.args.get("selector") {
            query["selector"] = v.clone();
        }
        if let Some(v) = ctx.args.get("text") {
            query["text"] = v.clone();
        }
        if let Some(v) = ctx.args.get("direction") {
            query["direction"] = v.clone();
        }

        match self.app.device_query(&self.run_id, query, 15).await {
            Ok(result) => {
                if result.get("error").is_some() {
                    Ok(ToolResult::failure(ctx.tool_call_id, result.to_string()))
                } else {
                    Ok(ToolResult::success(ctx.tool_call_id, result.to_string()))
                }
            }
            Err(e) => Ok(ToolResult::failure(
                ctx.tool_call_id,
                format!("Could not reach the user's device: {e}"),
            )),
        }
    }
}

struct ContactsTool {
    app: Arc<App>,
    run_id: String,
}

#[async_trait]
impl Tool for ContactsTool {
    fn name(&self) -> &str {
        "query_contacts"
    }
    fn description(&self) -> &str {
        "Search the user's phone contacts by name. Returns matching contacts with phone numbers and email addresses."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Name to search for (partial match). Leave empty to list recent contacts."
                }
            },
            "required": ["name"],
            "additionalProperties": false
        })
    }
    fn timeout_secs(&self) -> Option<u64> {
        Some(15)
    }
    async fn execute(&self, ctx: ToolContext<'_>) -> anyhow::Result<ToolResult> {
        let name = ctx.args.get("name").and_then(Value::as_str).unwrap_or("");
        let query = json!({
            "action": "query_contacts",
            "name": name,
        });
        match self.app.device_query(&self.run_id, query, 10).await {
            Ok(result) => {
                if result.get("error").is_some() {
                    Ok(ToolResult::failure(ctx.tool_call_id, result.to_string()))
                } else {
                    Ok(ToolResult::success(ctx.tool_call_id, result.to_string()))
                }
            }
            Err(e) => Ok(ToolResult::failure(
                ctx.tool_call_id,
                format!("Could not reach the user's device: {e}"),
            )),
        }
    }
}

pub fn registry(app: Arc<App>, run_id: &str, user: &str) -> ToolRegistry {
    let store = app.store.clone();
    let mut registry = ToolRegistry::new();
    for name in [
        "memory_store",
        "memory_recall",
        "memory_forget",
        "memory_update",
        "get_time",
    ] {
        registry.register(Arc::new(MemoryTool {
            store: store.clone(),
            user: user.into(),
            name,
        }));
    }
    for variant in ["cron_create", "cron_list", "cron_delete"] {
        registry.register(Arc::new(CronTool {
            store: store.clone(),
            user: user.into(),
            variant,
        }));
    }
    registry.register(Arc::new(OpenAppTool));
    registry.register(Arc::new(GetLocationTool {
        store: store.clone(),
        user: user.into(),
    }));
    registry.register(Arc::new(OpenBrowserTool));
    registry.register(Arc::new(OpenMapsTool));
    registry.register(Arc::new(GetNotificationsTool {
        store: store.clone(),
        user: user.into(),
    }));
    for variant in ["habit_create", "habit_log", "habit_status", "habit_delete"] {
        registry.register(Arc::new(HabitTool {
            store: store.clone(),
            user: user.into(),
            variant,
        }));
    }
    registry.register(Arc::new(TaskProgressTool));
    registry.register(Arc::new(BrowserTool {
        app: app.clone(),
        run_id: run_id.to_owned(),
    }));
    registry.register(Arc::new(ContactsTool {
        app,
        run_id: run_id.to_owned(),
    }));
    registry
}
