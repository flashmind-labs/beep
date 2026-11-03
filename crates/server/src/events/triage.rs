//! Event triage: each batch of account events goes to a small sub-agent
//! first, which reads what it needs and decides with one call. Only an
//! inform marked now wakes the main agent, with the triage summary rather
//! than the raw events. An inform marked later waits for the next wake or
//! the next conversation. Every decision is logged, so the main agent can
//! say what came in (`recent_events`) and topics can be tuned.
//!
//! When the run fails, events that look urgent by their kind wake the main
//! agent the old way, with the raw events; the rest are logged and dropped.

use super::{
    EventApp,
    pipeline::{self, Settings},
};
use crate::{
    app::App,
    apps::markdown,
    permissions,
    research::RoleModel,
    store::{EventRow, EventState, NewTriage, TriageRow, UpdateRow},
    subagents::{Decision, Failure, Kind, Spec},
    tools::recall_tool,
    user::User,
    wake::{self, Event, Rule},
};
use anyhow::Result;
use chrono::{Duration as Days, Utc};
use flashmind::types::{AliasedModel, Model, Provider, ReasoningLevel, tool::Tool};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{fmt::Write, sync::Arc, time::Duration};
use strum::{EnumString, IntoStaticStr};

/// The triage agent and its limits.
pub(crate) const KIND: Kind = Kind {
    name: "event_triage",
    instructions: include_str!("../../prompts/events/triage.md"),
    max_turns: 6,
    max_tokens: 60_000,
    time_limit: Duration::from_secs(3 * 60),
};

/// Informs older than this are no longer brought up.
const FOLD_WITHIN: Days = Days::days(2);
/// Most earlier informs folded into one wake.
const MAX_FOLDED: i64 = 10;
/// Most informs the apps list at once.
const MAX_UPDATES: i64 = 20;
/// Held events the triage agent is shown.
const MAX_HELD_SHOWN: i64 = 10;

/// What the triage agent decided about a batch, as the log keeps it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumString, IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum Verdict {
    Inform,
    Ignore,
    /// The run ended without a decision.
    Failed,
}

impl Verdict {
    pub fn as_str(self) -> &'static str {
        self.into()
    }
}

/// When the main agent should hear about an inform.
#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumString, IntoStaticStr, Deserialize)]
#[strum(serialize_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum Urgency {
    Now,
    Later,
}

impl Urgency {
    pub fn as_str(self) -> &'static str {
        self.into()
    }
}

/// The config's `[events]`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventsConfig {
    /// The model the triage agent runs on. A cheap one fits: it reads a few
    /// events and decides once.
    #[serde(default = "default_triage_model")]
    pub triage_model: RoleModel,
}

impl Default for EventsConfig {
    fn default() -> Self {
        Self {
            triage_model: default_triage_model(),
        }
    }
}

pub(crate) fn default_triage_model() -> RoleModel {
    RoleModel {
        model: Model {
            provider: Provider::OpenRouter,
            model: AliasedModel {
                name: "anthropic/claude-haiku-5.5".to_owned(),
                real_name: None,
            },
        },
        thinking: ReasoningLevel::Low,
    }
}

/// The triage agent's decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Triage {
    Inform {
        summary: String,
        why: String,
        urgency: Urgency,
        /// The event ids it covers.
        refs: Vec<i64>,
    },
    Ignore {
        reason: String,
    },
}

#[derive(Deserialize)]
struct InformArgs {
    summary: String,
    why: String,
    urgency: Urgency,
    #[serde(default)]
    refs: Vec<Value>,
}

#[derive(Deserialize)]
struct IgnoreArgs {
    reason: String,
}

/// An event id as models write it: `7`, `"7"` or `"#7"`.
fn event_ref(value: &Value) -> Option<i64> {
    match value {
        Value::Number(n) => n.as_i64(),
        Value::String(text) => text
            .trim()
            .trim_start_matches(['[', '#'])
            .trim_end_matches(']')
            .parse()
            .ok(),
        _ => None,
    }
}

fn nonempty(text: String, what: &str) -> Result<String, String> {
    let text = text.trim().to_owned();
    if text.is_empty() {
        return Err(format!("{what} is empty"));
    }
    Ok(text)
}

/// `inform_beep` and `ignore`.
pub(crate) fn decisions() -> Vec<Decision<Triage>> {
    vec![
        Decision {
            name: "inform_beep",
            description: "Pass the events that matter on to Beep, with a summary written to \
                the user. Ends your run.",
            parameters: || {
                json!({
                    "type": "object",
                    "properties": {
                        "summary": {"type": "string", "description": "What happened and what is asked of them, written to the user as \"you\", in two to four plain sentences."},
                        "why": {"type": "string", "description": "Why the user would want to know, in one line."},
                        "urgency": {"type": "string", "enum": ["now", "later"], "description": "now when it needs the user soon; later to bring it up next time."},
                        "refs": {"type": "array", "items": {"type": "integer"}, "description": "The [#id]s of the events the summary covers."}
                    },
                    "required": ["summary", "why", "urgency", "refs"],
                    "additionalProperties": false
                })
            },
            parse: |args| {
                let args: InformArgs =
                    serde_json::from_value(args.clone()).map_err(|error| error.to_string())?;
                Ok(Triage::Inform {
                    summary: nonempty(args.summary, "summary")?,
                    why: nonempty(args.why, "why")?,
                    urgency: args.urgency,
                    refs: args.refs.iter().filter_map(event_ref).collect(),
                })
            },
        },
        Decision {
            name: "ignore",
            description: "Nothing in these events deserves the user's attention. Ends your run.",
            parameters: || {
                json!({
                    "type": "object",
                    "properties": {"reason": {"type": "string", "description": "Why, in one line."}},
                    "required": ["reason"],
                    "additionalProperties": false
                })
            },
            parse: |args| {
                let args: IgnoreArgs =
                    serde_json::from_value(args.clone()).map_err(|error| error.to_string())?;
                Ok(Triage::Ignore {
                    reason: nonempty(args.reason, "reason")?,
                })
            },
        },
    ]
}

/// What a decision leads to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Route {
    /// Wake the main agent with the summary now.
    Wake,
    /// Keep it for the next wake or conversation.
    Defer,
    /// Only log it.
    Drop,
}

/// Where a decision goes, given whether the user has a wake left this hour.
pub(crate) fn route(triage: &Triage, wake_left: bool) -> Route {
    match triage {
        Triage::Inform {
            urgency: Urgency::Now,
            ..
        } if wake_left => Route::Wake,
        Triage::Inform { .. } => Route::Defer,
        Triage::Ignore { .. } => Route::Drop,
    }
}

/// The events of a failed triage that still wake the main agent.
pub(crate) fn urgent_looking(rows: &[EventRow]) -> Vec<EventRow> {
    rows.iter()
        .filter(|row| row.kind.looks_urgent())
        .cloned()
        .collect()
}

/// Triages `rows`, already moved to triaging, and acts on the decision.
/// Runs as a sub-agent task.
pub(crate) async fn run(app: Arc<App>, user: Arc<User>, rows: Vec<EventRow>, settings: Settings) {
    let spec = spec(&app, &user, &rows, &settings.triage).await;
    let decided = app.subagents.run(user.id(), spec).await;
    if let Err(error) = settle(&app, &user, &rows, decided, &settings).await {
        log::warn!("events: unable to settle a triage: {error:#}; user={user}");
        // Those it did not settle go to the next triage.
        let ids: Vec<i64> = rows.iter().map(|row| row.id).collect();
        match user
            .move_events(&ids, EventState::Triaging, EventState::Pending, None)
            .await
        {
            Ok(0) => {}
            Ok(_) => app.events.mark_pending_now(user.id()),
            Err(error) => {
                log::warn!("events: unable to requeue a failed triage: {error:#}; user={user}")
            }
        }
    }
}

/// How long events may sit in triage before the sweep takes them for a run
/// that died: well past the run's own time limit, so a slow settle is not
/// cut short.
pub(crate) const STALE_TRIAGE: Duration = Duration::from_secs(KIND.time_limit.as_secs() * 2);

/// Logs the decision and wakes, defers or drops as it says.
pub(crate) async fn settle(
    app: &Arc<App>,
    user: &Arc<User>,
    rows: &[EventRow],
    decided: Result<Triage, Failure>,
    settings: &Settings,
) -> Result<()> {
    let ids: Vec<i64> = rows.iter().map(|row| row.id).collect();
    let triage = match decided {
        Ok(triage) => triage,
        Err(failure) => return fall_back(app, user, rows, &failure, settings).await,
    };
    let (new, wanted_wake) = match &triage {
        Triage::Inform {
            summary,
            why,
            urgency,
            refs,
        } => {
            let refs: Vec<i64> = refs.iter().copied().filter(|id| ids.contains(id)).collect();
            let entry = NewTriage {
                verdict: Verdict::Inform,
                urgency: Some(*urgency),
                summary: Some(summary),
                reason: why,
                refs: &refs,
            };
            (
                user.log_triage(&entry, &ids).await?,
                *urgency == Urgency::Now,
            )
        }
        Triage::Ignore { reason } => {
            let entry = NewTriage {
                verdict: Verdict::Ignore,
                urgency: None,
                summary: None,
                reason,
                refs: &[],
            };
            (user.log_triage(&entry, &ids).await?, false)
        }
    };
    let wake_left = wanted_wake && app.events.take_wake(user.id());
    let routed = route(&triage, wake_left);
    log::info!(
        "events: triaged; user={user} triage_id={new} events={} route={routed:?}",
        ids.len()
    );
    if routed == Route::Wake {
        let woke = wake_informed(app, user, rows).await;
        if !matches!(woke, Ok(true)) {
            app.events.give_back_wake(user.id());
        }
        woke?;
    }
    Ok(())
}

/// Logs a failed triage and passes its urgent-looking events on the old way.
async fn fall_back(
    app: &Arc<App>,
    user: &Arc<User>,
    rows: &[EventRow],
    failure: &Failure,
    settings: &Settings,
) -> Result<()> {
    let ids: Vec<i64> = rows.iter().map(|row| row.id).collect();
    let urgent = urgent_looking(rows);
    let reason = format!("triage {failure}");
    let refs: Vec<i64> = urgent.iter().map(|row| row.id).collect();
    let entry = NewTriage {
        verdict: Verdict::Failed,
        urgency: None,
        summary: None,
        reason: &reason,
        refs: &refs,
    };
    user.log_triage(&entry, &ids).await?;
    log::warn!(
        "events: {reason}; user={user} events={} urgent_looking={}",
        ids.len(),
        urgent.len()
    );
    if urgent.is_empty() {
        return Ok(());
    }
    if !app.events.take_wake(user.id()) {
        log::warn!("events: no wake left for a failed triage; dropped; user={user}");
        return Ok(());
    }
    pipeline::wake_rows(
        app,
        user,
        &urgent,
        EventState::Triaged,
        Rule::Accounts,
        settings,
    )
    .await
    .map(drop)
}

/// Wakes the main agent with every inform not yet delivered, the newest
/// first. `rows` are the events of the batch just triaged. Whether a run
/// started.
async fn wake_informed(app: &Arc<App>, user: &Arc<User>, rows: &[EventRow]) -> Result<bool> {
    let informs = user
        .undelivered_informs(Utc::now() - FOLD_WITHIN, MAX_FOLDED)
        .await?;
    let Some(newest) = informs
        .iter()
        .rev()
        .find(|row| row.urgency == Some(Urgency::Now))
    else {
        return Ok(false);
    };
    let earlier: Vec<&TriageRow> = informs.iter().filter(|row| row.id != newest.id).collect();
    let read_only = match user.app_rules().await {
        Ok(rules) => pipeline::apps_of(rows)
            .into_iter()
            .filter(|app| rules.read_only(app.slug()))
            .collect(),
        Err(error) => {
            log::warn!("events: unable to read app permissions: {error:#}; user={user}");
            Vec::new()
        }
    };
    let event = Event {
        label: pipeline::clipped(newest.summary.as_deref().unwrap_or("Account update")),
        description: describe_inform(newest, &earlier, rows, &read_only),
        data: Value::Null,
    };
    let Some(run) = wake::wake_with(app, user, event, Rule::Triaged).await? else {
        return Ok(false);
    };
    let ids: Vec<i64> = informs.iter().map(|row| row.id).collect();
    user.mark_delivered(&ids, Some(&run.id)).await?;
    log::info!(
        "events: woke the agent on a triage; user={user} run_id={} informs={}",
        run.id,
        ids.len()
    );
    Ok(true)
}

/// The message a triage wake starts on: the summary, why, the events it
/// covers with their ids, and earlier informs.
pub(crate) fn describe_inform(
    inform: &TriageRow,
    earlier: &[&TriageRow],
    rows: &[EventRow],
    read_only: &[EventApp],
) -> String {
    let mut text = format!(
        "From your event triage agent, written to the user:\n{}\nWhy now: {}",
        inform.summary.as_deref().unwrap_or_default(),
        inform.reason
    );
    let covered: Vec<&EventRow> = rows
        .iter()
        .filter(|row| inform.refs.contains(&row.id))
        .collect();
    if !covered.is_empty() {
        text.push_str("\n\nThe events it covers:");
        for row in covered {
            pipeline::write_event(&mut text, row);
        }
    }
    if !earlier.is_empty() {
        text.push_str("\n\nEarlier, less urgent:");
        for row in earlier {
            let _ = write!(text, "\n- {}", row.summary.as_deref().unwrap_or_default());
        }
    }
    if !read_only.is_empty() {
        let names: Vec<&str> = read_only.iter().map(|app| app.name()).collect();
        let _ = write!(
            text,
            "\n\nThe user set {} to read only: offer nothing that would act there.",
            names.join(", ")
        );
    }
    text
}

/// Informs the user has not seen yet, newest first, for the apps to show
/// apart from any chat.
pub async fn waiting_updates(user: &User) -> Result<Vec<UpdateRow>> {
    user.waiting_updates(Utc::now() - FOLD_WITHIN, MAX_UPDATES)
        .await
}

/// The triage run for `rows`: their apps' read tools, a read-only `memory`, and
/// the user's topics, held events and memory profile as context.
async fn spec(app: &App, user: &Arc<User>, rows: &[EventRow], model: &RoleModel) -> Spec<Triage> {
    Spec {
        kind: &KIND,
        model: model.clone(),
        routing: app.models.routing(&model.model).cloned(),
        tools: tools(user, rows).await,
        decisions: decisions(),
        task: pipeline::describe(rows, Rule::Accounts, &[]),
        context: Some(context(user).await),
        watch: None,
        // Only read tools: nothing it can call acts.
        gate: None,
    }
}

/// The read tools of the apps `rows` came from, and a read-only `memory`.
async fn tools(user: &Arc<User>, rows: &[EventRow]) -> Vec<Arc<dyn Tool>> {
    let apps: Vec<&str> = pipeline::apps_of(rows)
        .into_iter()
        .map(EventApp::slug)
        .collect();
    let (composio, mcp) = tokio::join!(user.app_tools(), user.mcp_tools());
    let reads: Vec<Arc<dyn Tool>> = composio
        .tools
        .into_iter()
        .chain(mcp)
        .filter(|tool| {
            permissions::read_tool_app(tool.name(), &composio.hinted)
                .is_some_and(|app| apps.contains(&app))
        })
        .collect();
    let mut tools = markdown::wrap(reads);
    tools.push(recall_tool(user));
    tools
}

/// Today's date, the user's topics, what is held for a digest and the
/// memory profile.
async fn context(user: &User) -> String {
    let mut ctx = format!("Today is {}.\n", Utc::now().format("%A %Y-%m-%d %H:%M UTC"));
    match user.event_topics().await {
        Ok(topics) => {
            ctx.push_str("\nEvent topics the user chose (only topics that are on reach you):\n");
            for (topic, on) in topics.iter() {
                let state = if on { "on" } else { "off" };
                let _ = writeln!(ctx, "- {}: {state} ({})", topic.as_str(), topic.describe());
            }
        }
        Err(error) => log::warn!("events: unable to read topics: {error:#}; user={user}"),
    }
    match user
        .events_in(EventState::Held, Duration::ZERO, MAX_HELD_SHOWN)
        .await
    {
        Ok(held) if !held.is_empty() => {
            ctx.push_str("\nEvents Beep is holding for a later digest:\n");
            for row in held {
                let _ = writeln!(ctx, "- {}: {}", row.app.name(), row.summary);
            }
        }
        Ok(_) => {}
        Err(error) => log::warn!("events: unable to read held events: {error:#}; user={user}"),
    }
    match user.memory_profile().await {
        Ok(Some(profile)) => {
            let _ = write!(ctx, "\n{profile}");
        }
        Ok(None) => {}
        Err(error) => {
            log::warn!("events: unable to build the memory profile: {error}; user={user}")
        }
    }
    ctx
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decide(name: &str, args: Value) -> Result<Triage, String> {
        let decision = decisions()
            .into_iter()
            .find(|decision| decision.name == name)
            .ok_or("no such decision")?;
        (decision.parse)(&args)
    }

    fn inform(urgency: Urgency) -> Triage {
        Triage::Inform {
            summary: "Ana asked for a review.".to_owned(),
            why: "A person is waiting.".to_owned(),
            urgency,
            refs: vec![1],
        }
    }

    #[test]
    fn decisions_read_their_arguments() {
        assert_eq!(
            decide(
                "inform_beep",
                json!({"summary": " Ana asked for a review. ", "why": "A person is waiting.", "urgency": "now", "refs": [1, "#2", "[#3]", "x"]})
            ),
            Ok(Triage::Inform {
                summary: "Ana asked for a review.".to_owned(),
                why: "A person is waiting.".to_owned(),
                urgency: Urgency::Now,
                refs: vec![1, 2, 3],
            })
        );
        assert_eq!(
            decide("ignore", json!({"reason": "Newsletters."})),
            Ok(Triage::Ignore {
                reason: "Newsletters.".to_owned()
            })
        );
        assert!(
            decide(
                "inform_beep",
                json!({"summary": "x", "why": "y", "urgency": "soon", "refs": []})
            )
            .is_err()
        );
        assert!(decide("ignore", json!({"reason": "  "})).is_err());
    }

    #[test]
    fn only_an_inform_now_with_a_wake_left_wakes() {
        assert_eq!(route(&inform(Urgency::Now), true), Route::Wake);
        assert_eq!(route(&inform(Urgency::Now), false), Route::Defer);
        assert_eq!(route(&inform(Urgency::Later), true), Route::Defer);
        let ignore = Triage::Ignore {
            reason: "Spam.".to_owned(),
        };
        assert_eq!(route(&ignore, true), Route::Drop);
    }
}
