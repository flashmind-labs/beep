//! The main agent's tools for background work, for admins: `spawn_agent`
//! starts a sub-agent, `run_in_background` moves one tool call off the turn,
//! `jobs` lists both, `wait_job` waits for one and `cancel_job` stops one.
//!
//! A job names its tool and arguments (`run_in_background {tool, arguments}`)
//! rather than every tool taking a `background` flag: Composio's and the MCP
//! servers' schemas stay as they are, and one place decides which tools may
//! leave the turn.

use super::{
    Decision, Ended, Failure, Kind, Pool, Spec, Watch,
    kept::{Kept, Next},
    run::Session,
    work::{
        AfterFinish, Delivery, MAX_WAIT, Outcome, REPORT, Submitted, WorkId, WorkKind, deliver,
    },
};
use crate::{
    app::{App, Draws, Message},
    grants::RoutineGrants,
    nodes::head_and_tail,
    permissions::{self, Gate, Rules, ToolSlug},
    protocol::DeviceKind,
    research::RoleModel,
    store::CreateRun,
    tools::{file, mac_files, suggest},
    user::User,
};
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use chrono::Utc;
use flashmind::types::{
    ToolCall, ToolGate, ToolRegistry,
    tool::{Tool, ToolContext, ToolResult},
};
use serde_json::{Value, json};
use std::{
    collections::HashSet,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::time::timeout;
use uuid::Uuid;

/// Tools that only work inside a turn: they draw in the reply, reach the
/// device the turn came from, or steer the turn itself.
const TURN_ONLY: &[&str] = &[
    "notify_user",
    suggest::NAME,
    "decision",
    "show_cards",
    "show_products",
    "show_chart",
    "show_stock_chart",
    "task_progress",
    file::NAME,
    "send_image",
    mac_files::NAME,
    "browser",
    "query_contacts",
    "open_app",
    "set_alarm",
    "open_browser",
    "open_maps",
    "hold_events",
    "start_research",
    "research_more",
    "add_scouts",
    "stop_research",
];

const SPAWN: &str = "spawn_agent";
const RUN: &str = "run_in_background";
const JOBS: &str = "jobs";
const WAIT: &str = "wait_job";
const CANCEL: &str = "cancel_job";
const MESSAGE: &str = "message_agent";
const OWN: &[&str] = &[SPAWN, RUN, JOBS, WAIT, CANCEL, MESSAGE];

/// The kind `spawn_agent` starts.
const GENERAL: Kind = Kind {
    name: "general",
    instructions: include_str!("../../prompts/subagents/general.md"),
    max_turns: 30,
    max_tokens: 400_000,
    time_limit: Duration::from_secs(20 * 60),
};

/// A job's time when its tool sets none.
const JOB_TIME: Duration = Duration::from_secs(10 * 60);
/// Longest task text kept for listing.
const TASK_CHARS: usize = 160;
/// The label of a turn that hands results back.
pub(crate) const DELIVERY_LABEL: &str = "Background work";

/// What the turn hands its background tools.
pub(crate) struct Setup {
    pub(crate) app: Arc<App>,
    pub(crate) user: Arc<User>,
    pub(crate) chat: Uuid,
    /// The turn's model, which sub-agents run on.
    pub(crate) model: RoleModel,
    /// The user's app tools, unguarded: the gate checks them as they run.
    pub(crate) app_tools: Vec<Arc<dyn Tool>>,
    pub(crate) rules: Rules,
    pub(crate) hinted: HashSet<ToolSlug>,
    /// The grants of the routine whose run this is, if it is one.
    pub(crate) grants: Option<Arc<RoutineGrants>>,
}

/// One chat's background work: who it is for, and the tools it may call.
struct Background {
    app: Arc<App>,
    user: Arc<User>,
    chat: Uuid,
    model: RoleModel,
    /// Every tool a sub-agent or job may call, behind `gate`.
    tools: ToolRegistry,
    gate: Arc<dyn ToolGate>,
}

/// Adds the background tools to a turn's `registry`. Sub-agents and jobs
/// get the tools registered so far, less the turn-only ones, and the app
/// tools under the user's rules.
pub(crate) fn register(registry: &mut ToolRegistry, setup: Setup) {
    let Setup {
        app,
        user,
        chat,
        model,
        app_tools,
        rules,
        hinted,
        grants,
    } = setup;
    let mut tools = registry.clone();
    tools.retain(|name| !TURN_ONLY.contains(&name) && !OWN.contains(&name));
    for tool in permissions::for_gate(app_tools, &rules, &hinted) {
        tools.register(tool);
    }
    let gate: Arc<dyn ToolGate> = Arc::new(Gate {
        rules,
        hinted,
        user: user.clone(),
        chat,
        approvals: app.subagents.approvals.clone(),
        grants,
    });
    tools.set_gate(gate.clone());
    let background = Arc::new(Background {
        app,
        user,
        chat,
        model,
        tools,
        gate,
    });
    registry.register(Arc::new(SpawnAgentTool(background.clone())));
    registry.register(Arc::new(RunInBackgroundTool(background.clone())));
    registry.register(Arc::new(JobsTool(background.clone())));
    registry.register(Arc::new(WaitJobTool(background.clone())));
    registry.register(Arc::new(CancelJobTool(background.clone())));
    registry.register(Arc::new(MessageAgentTool(background)));
}

impl Background {
    fn id(&self) -> &str {
        self.user.id()
    }

    /// Records how `id` ended, and hands it back to the chat when nobody
    /// waited for it.
    fn finish(self: &Arc<Self>, id: WorkId, outcome: Outcome) {
        let work = &self.app.subagents.work;
        if work.finish(id, outcome) == AfterFinish::Nothing {
            return;
        }
        let background = self.clone();
        let delivering = self
            .app
            .subagents
            .spawn(Pool::Deliveries, self.id(), async move {
                background.deliver().await;
            });
        if delivering.is_err() {
            log::warn!(
                "subagents: no room to deliver; results wait for the next message; chat={}",
                self.chat
            );
            work.abandon_delivery(self.chat);
        }
    }

    async fn deliver(&self) {
        let subagents = &self.app.subagents;
        deliver(
            &subagents.work,
            self.chat,
            &subagents.turn_ended,
            || async {
                let message = delivery_message(self.chat);
                Ok(match self.app.submit_delivery(&self.user, message).await? {
                    None => Submitted::Nothing,
                    Some(CreateRun::Conflict) => Submitted::Busy,
                    Some(CreateRun::Created(_) | CreateRun::Existing(_)) => Submitted::Started,
                })
            },
        )
        .await;
    }

    /// Follows the kept sub-agent `kept` until it is idle, then lists how it
    /// ended. A message that reaches it meanwhile keeps it going.
    async fn follow(&self, kept: &Kept<Session<Report>>, watch: &Watch, running: Unfinished) {
        let subagents = &self.app.subagents;
        loop {
            let ended = subagents.follow(&kept.session, watch).await;
            let mut status = kept.lock();
            // `message_agent` queues under this lock, so a message sent
            // before the run ended is taken here, and one after finds it idle.
            match status.after_run(watch.cancel.is_cancelled(), Instant::now()) {
                Next::Continue(text) => {
                    if let Err(error) = kept.session.follow_up(&text) {
                        status.idle_since = Some(Instant::now());
                        let failure = Failure::Failed(format!("{error:#}"));
                        running.finish(outcome_of(Err(failure)));
                        return;
                    }
                }
                Next::Idle => {
                    running.finish(outcome_of(ended));
                    return;
                }
            }
        }
    }

    /// The work id the model passed.
    fn work_id(args: &Value) -> Result<WorkId> {
        let id = args
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("missing string argument: id"))?;
        Ok(id.parse()?)
    }
}

/// The turn that hands results back: it runs like a finished research,
/// in the background, and tells the user.
fn delivery_message(chat: Uuid) -> Message {
    Message {
        session_id: chat,
        request_id: Uuid::new_v4(),
        message: REPORT.to_owned(),
        cron_id: None,
        agent: None,
        scheduled: Some(DELIVERY_LABEL.to_owned()),
        always_sent: true,
        woken: None,
        images: Vec::new(),
        files: Vec::new(),
        answer: None,
        model: None,
        thinking: None,
        origin: DeviceKind::Phone {},
        platform: None,
        draws: Draws::default(),
        finished_research: true,
        voice: None,
        spoken: false,
    }
}

fn clip(text: &str, max: usize) -> String {
    match text.char_indices().nth(max) {
        Some((at, _)) => format!("{}...", &text[..at]),
        None => text.to_owned(),
    }
}

/// What a sub-agent ends with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Report {
    summary: String,
    done: bool,
}

fn report() -> Decision<Report> {
    Decision {
        name: "report",
        description: "Finish with your report to Beep: what you found or did, with the \
                      facts it needs (names, IDs, links, numbers). Ends your work.",
        parameters: || {
            json!({"type": "object", "properties": {
                "summary": {"type": "string", "description": "The report, a few sentences."},
                "status": {"type": "string", "enum": ["done", "failed"],
                           "description": "failed when you could not do the task."}
            }, "required": ["summary", "status"], "additionalProperties": false})
        },
        parse: |args| {
            let summary = args["summary"]
                .as_str()
                .map(str::trim)
                .filter(|summary| !summary.is_empty())
                .ok_or("summary must say what you found or did")?;
            let done = match args["status"].as_str() {
                Some("done") => true,
                Some("failed") => false,
                _ => return Err("status must be done or failed".to_owned()),
            };
            Ok(Report {
                summary: summary.to_owned(),
                done,
            })
        },
    }
}

fn outcome_of(ended: Result<Ended<Report>, super::Failure>) -> Outcome {
    match ended {
        Ok(Ended::Decided(report)) => Outcome {
            ok: report.done,
            text: head_and_tail(&report.summary),
        },
        // It answered without calling `report`; its answer is the report.
        Ok(Ended::Replied(reply)) => Outcome {
            ok: true,
            text: head_and_tail(&reply),
        },
        Err(failure) => Outcome {
            ok: false,
            text: format!("The agent {failure}."),
        },
    }
}

struct SpawnAgentTool(Arc<Background>);

#[async_trait]
impl Tool for SpawnAgentTool {
    fn name(&self) -> &str {
        SPAWN
    }

    fn description(&self) -> &str {
        "Start a sub-agent on a task that takes many steps, such as going through a \
         mailbox or comparing several sources, while you keep talking with the user. It \
         has your tools, under the same app permissions: an action that asks first shows \
         the user a card and waits. Its report comes back to this chat when it ends. Tell \
         the user it started, then go on. urgency: now (the default) reports as soon as it \
         ends; later keeps the report for the user's next message."
    }

    fn parameters(&self) -> Value {
        json!({"type": "object", "properties": {
            "task": {"type": "string", "description": "The whole task, with everything it needs to know: it cannot see this chat."},
            "urgency": {"type": "string", "enum": ["now", "later"]}
        }, "required": ["task"], "additionalProperties": false})
    }

    async fn execute(&self, ctx: ToolContext<'_>) -> Result<ToolResult> {
        let background = &self.0;
        let task = ctx
            .args
            .get("task")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|task| !task.is_empty())
            .ok_or_else(|| anyhow!("missing string argument: task"))?;
        let delivery = match ctx.args.get("urgency").and_then(Value::as_str) {
            Some("later") => Delivery::Later,
            _ => Delivery::Now,
        };
        let subagents = &background.app.subagents;
        if !subagents.has_room(Pool::Agents, background.id()) {
            return Ok(ToolResult::success(ctx.tool_call_id, BUSY));
        }
        let (id, watch) = subagents.work.start(
            background.id(),
            background.chat,
            WorkKind::Agent,
            &clip(task, TASK_CHARS),
            delivery,
        );
        let spec = Spec {
            kind: &GENERAL,
            model: background.model.clone(),
            routing: background
                .app
                .models
                .routing(&background.model.model)
                .cloned(),
            tools: background
                .tools
                .list()
                .into_iter()
                .filter_map(|name| background.tools.get(name).cloned())
                .collect(),
            decisions: vec![report()],
            task: task.to_owned(),
            context: Some(format!("Now: {}", Utc::now().format("%Y-%m-%d %H:%M UTC"))),
            watch: Some(watch),
            gate: Some(background.gate.clone()),
        };
        let running = Unfinished::new(background, id);
        let started = subagents.spawn(Pool::Agents, background.id(), async move {
            let background = running.background.clone();
            let subagents = &background.app.subagents;
            let watch = spec.watch.clone().unwrap_or_default();
            let session = match subagents.open(background.id(), spec).await {
                Ok(session) => session,
                Err(failure) => return running.finish(outcome_of(Err(failure))),
            };
            let kept = subagents.kept.keep(
                id,
                background.id(),
                background.chat,
                session,
                Instant::now(),
            );
            background.follow(&kept, &watch, running).await;
        });
        if started.is_err() {
            subagents.work.forget(id);
            return Ok(ToolResult::success(ctx.tool_call_id, BUSY));
        }
        Ok(ToolResult::success(
            ctx.tool_call_id,
            format!(
                "Started agent {id}. Its report comes back to this chat when it ends; \
                 `jobs` shows how it is going and `message_agent` tells it more."
            ),
        ))
    }
}

/// Work started in the background until it records how it ended. Dropped
/// before that, by a panic, an abort or the server stopping, it lists the
/// work as failed, so it never shows as running forever.
struct Unfinished {
    background: Arc<Background>,
    id: WorkId,
    armed: bool,
}

/// What work that ended without a result reports.
const ENDED_WITHOUT_RESULT: &str = "Ended without a result: it crashed or was stopped.";

impl Unfinished {
    fn new(background: &Arc<Background>, id: WorkId) -> Self {
        Self {
            background: background.clone(),
            id,
            armed: true,
        }
    }

    fn finish(mut self, outcome: Outcome) {
        self.armed = false;
        self.background.finish(self.id, outcome);
    }
}

impl Drop for Unfinished {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        // No delivery from here: a drop may run while the task set is being
        // torn down. The result waits for the chat's next message.
        let work = &self.background.app.subagents.work;
        let outcome = Outcome {
            ok: false,
            text: ENDED_WITHOUT_RESULT.to_owned(),
        };
        if work.finish(self.id, outcome) == AfterFinish::Deliver {
            work.abandon_delivery(self.background.chat);
        }
    }
}

const BUSY: &str = "Not started: too much background work is running for this user. \
                    Wait for some of it to end, or do it in this turn.";

struct RunInBackgroundTool(Arc<Background>);

#[async_trait]
impl Tool for RunInBackgroundTool {
    fn name(&self) -> &str {
        RUN
    }

    fn description(&self) -> &str {
        "Run one slow tool call in the background, so the chat goes on meanwhile. Give \
         the tool's name and the arguments you would call it with. It answers at once \
         with a job id; the result comes back to this chat when the call ends, or call \
         wait_job for it. Actions keep their app permissions."
    }

    fn parameters(&self) -> Value {
        json!({"type": "object", "properties": {
            "tool": {"type": "string"},
            "arguments": {"type": "object"}
        }, "required": ["tool", "arguments"], "additionalProperties": false})
    }

    async fn execute(&self, ctx: ToolContext<'_>) -> Result<ToolResult> {
        let background = &self.0;
        let name = ctx
            .args
            .get("tool")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("missing string argument: tool"))?;
        let Some(tool) = background.tools.get(name).cloned() else {
            return Ok(ToolResult::failure(
                ctx.tool_call_id,
                format!("{name} cannot run in the background; call it in this turn."),
            ));
        };
        let arguments = ctx.args.get("arguments").cloned().unwrap_or(json!({}));
        let call = ToolCall {
            id: format!("job-{}", Uuid::new_v4()),
            name: tool.name().to_owned(),
            arguments,
        };
        let subagents = &background.app.subagents;
        if !subagents.has_room(Pool::Jobs, background.id()) {
            return Ok(ToolResult::success(ctx.tool_call_id, BUSY));
        }
        let task = clip(
            &format!("{} {}", call.name, tool.humanize(&call.arguments)),
            TASK_CHARS,
        );
        let (id, watch) = subagents.work.start(
            background.id(),
            background.chat,
            WorkKind::Job,
            task.trim(),
            Delivery::Now,
        );
        // A call that asks first may wait for the user before its own time starts.
        let limit =
            tool.timeout_secs().map_or(JOB_TIME, Duration::from_secs) + permissions::ASK_TIMEOUT;
        let running = Unfinished::new(background, id);
        let started = subagents.spawn(Pool::Jobs, background.id(), async move {
            let outcome = run_job(&running.background.tools, &call, &watch, limit).await;
            running.finish(outcome);
        });
        if started.is_err() {
            subagents.work.forget(id);
            return Ok(ToolResult::success(ctx.tool_call_id, BUSY));
        }
        Ok(ToolResult::success(
            ctx.tool_call_id,
            format!("Job {id} is running. Its result comes back to this chat when it ends."),
        ))
    }
}

/// Runs one job's call through `tools`, gate and all, within `limit`.
async fn run_job(tools: &ToolRegistry, call: &ToolCall, watch: &Watch, limit: Duration) -> Outcome {
    watch.set_step(tools.humanize(call));
    match timeout(limit, tools.execute(call, None, &watch.cancel)).await {
        Ok(result) => Outcome {
            ok: result.is_success(),
            text: head_and_tail(&result.output()),
        },
        Err(_) => {
            watch.cancel.cancel();
            Outcome {
                ok: false,
                text: format!(
                    "Stopped after {} minutes without an answer.",
                    limit.as_secs() / 60
                ),
            }
        }
    }
}

struct JobsTool(Arc<Background>);

#[async_trait]
impl Tool for JobsTool {
    fn name(&self) -> &str {
        JOBS
    }

    fn description(&self) -> &str {
        "List the sub-agents and background jobs of this chat: running ones with what \
         they did last, then those that ended in the last hour."
    }

    fn parameters(&self) -> Value {
        json!({"type": "object", "properties": {}, "additionalProperties": false})
    }

    async fn execute(&self, ctx: ToolContext<'_>) -> Result<ToolResult> {
        let background = &self.0;
        let listed = background
            .app
            .subagents
            .work
            .list(background.id(), background.chat);
        if listed.is_empty() {
            return Ok(ToolResult::success(
                ctx.tool_call_id,
                "Nothing has run in the background in this chat lately.",
            ));
        }
        let now = Utc::now();
        let lines: Vec<String> = listed
            .iter()
            .map(|item| {
                let kind: &'static str = item.kind.into();
                let minutes = (now - item.started).num_minutes();
                match &item.outcome {
                    None if item.step.is_empty() => {
                        format!("{} {kind}, running {minutes} min: {}", item.id, item.task)
                    }
                    None => format!(
                        "{} {kind}, running {minutes} min: {} (last step: {})",
                        item.id, item.task, item.step
                    ),
                    Some(outcome) => format!(
                        "{} {kind}, {}: {}",
                        item.id,
                        if outcome.ok { "finished" } else { "failed" },
                        item.task
                    ),
                }
            })
            .collect();
        Ok(ToolResult::success(ctx.tool_call_id, lines.join("\n")))
    }
}

struct WaitJobTool(Arc<Background>);

#[async_trait]
impl Tool for WaitJobTool {
    fn name(&self) -> &str {
        WAIT
    }

    fn description(&self) -> &str {
        "Wait for a sub-agent or job of this chat to end, up to timeout seconds (at most \
         120), and get its result. A result you get here does not come back again."
    }

    fn parameters(&self) -> Value {
        json!({"type": "object", "properties": {
            "id": {"type": "string", "description": "Such as w3."},
            "timeout": {"type": "integer", "description": "Seconds, 60 if left out."}
        }, "required": ["id"], "additionalProperties": false})
    }

    fn timeout_secs(&self) -> Option<u64> {
        Some(MAX_WAIT.as_secs() + 10)
    }

    async fn execute(&self, ctx: ToolContext<'_>) -> Result<ToolResult> {
        let background = &self.0;
        let id = Background::work_id(&ctx.args)?;
        let wait = ctx
            .args
            .get("timeout")
            .and_then(Value::as_u64)
            .map_or(Duration::from_secs(60), Duration::from_secs);
        let work = &background.app.subagents.work;
        let output = match work
            .wait(background.id(), background.chat, id, wait)
            .await?
        {
            Some(outcome) if outcome.ok => format!("{id} finished:\n{}", outcome.text),
            Some(outcome) => format!("{id} failed:\n{}", outcome.text),
            None => {
                format!("{id} is still running; its result comes back to this chat when it ends.")
            }
        };
        Ok(ToolResult::success(ctx.tool_call_id, output))
    }
}

struct MessageAgentTool(Arc<Background>);

#[async_trait]
impl Tool for MessageAgentTool {
    fn name(&self) -> &str {
        MESSAGE
    }

    fn description(&self) -> &str {
        "Tell a sub-agent of this chat more: steer it while it works, or give one that \
         reported a new request; it keeps everything it did. A running agent reads the \
         message once its current run ends; one that reported starts again. Its next \
         report comes back to this chat like the first. Agents are kept for an hour after \
         their last report."
    }

    fn parameters(&self) -> Value {
        json!({"type": "object", "properties": {
            "id": {"type": "string", "description": "The agent's id, such as w3."},
            "text": {"type": "string", "description": "The message, complete on its own."}
        }, "required": ["id", "text"], "additionalProperties": false})
    }

    async fn execute(&self, ctx: ToolContext<'_>) -> Result<ToolResult> {
        let background = &self.0;
        let id = Background::work_id(&ctx.args)?;
        let text = ctx
            .args
            .get("text")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .ok_or_else(|| anyhow!("missing string argument: text"))?;
        let output = background.message(id, text)?;
        Ok(ToolResult::success(ctx.tool_call_id, output))
    }
}

impl Background {
    /// Hands `text` to the kept sub-agent `id`: queued when it runs, or a
    /// new run of it when it is idle.
    fn message(self: &Arc<Self>, id: WorkId, text: &str) -> Result<String> {
        let subagents = &self.app.subagents;
        let Some(kept) = subagents.kept.get(id, self.id(), self.chat, Instant::now()) else {
            return Ok(format!(
                "{id} cannot take messages: it is a job, or an agent that went idle over \
                 an hour ago. Start a new agent instead."
            ));
        };
        let mut status = kept.lock();
        if status.idle_since.is_none() {
            status.queued.push(text.to_owned());
            return Ok(format!(
                "Sent to {id}. It reads it once its current run ends; its report \
                 covers it."
            ));
        }
        if !subagents.has_room(Pool::Agents, self.id()) {
            return Ok(BUSY.to_owned());
        }
        let (watch, unseen) = subagents.work.resume(self.id(), self.chat, id)?;
        kept.session.follow_up(text)?;
        status.idle_since = None;
        let running = Unfinished::new(self, id);
        let following = kept.clone();
        let started = subagents.spawn(Pool::Agents, self.id(), async move {
            let background = running.background.clone();
            background.follow(&following, &watch, running).await;
        });
        if started.is_err() {
            // The dropped task listed it as failed; stop what it started.
            kept.session.cancel();
            status.idle_since = Some(Instant::now());
            return Ok(BUSY.to_owned());
        }
        let mut output = format!("{id} started again with your message.");
        if let Some(outcome) = unseen {
            output.push_str(&format!(
                " Its last report, which had not come back yet:\n{}",
                outcome.text
            ));
        }
        Ok(output)
    }
}

struct CancelJobTool(Arc<Background>);

#[async_trait]
impl Tool for CancelJobTool {
    fn name(&self) -> &str {
        CANCEL
    }

    fn description(&self) -> &str {
        "Stop a sub-agent or job of this chat. Nothing comes back from it."
    }

    fn parameters(&self) -> Value {
        json!({"type": "object", "properties": {
            "id": {"type": "string", "description": "Such as w3."}
        }, "required": ["id"], "additionalProperties": false})
    }

    async fn execute(&self, ctx: ToolContext<'_>) -> Result<ToolResult> {
        let background = &self.0;
        let id = Background::work_id(&ctx.args)?;
        let subagents = &background.app.subagents;
        // Messages sent to a running agent wait in its kept status; they
        // go with the cancel instead of starting a run nobody follows.
        let dropped = subagents
            .kept
            .get(id, background.id(), background.chat, Instant::now())
            .map(|kept| std::mem::take(&mut kept.lock().queued))
            .unwrap_or_default();
        let output = if subagents
            .work
            .cancel(background.id(), background.chat, id)?
        {
            match dropped.len() {
                0 => format!("Stopped {id}."),
                count => format!("Stopped {id}; {count} message(s) sent to it were dropped."),
            }
        } else {
            format!("{id} had already ended.")
        };
        Ok(ToolResult::success(ctx.tool_call_id, output))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        app::UserNotice,
        protocol::{ApprovalDecision, PermissionLevel},
        store::{AppPermissionRow, Store, TestDb},
        subagents::Subagents,
    };
    use flashmind::types::{
        AgentLlmConfig, CompletionRequest, CompletionStream, FinishReason, LlmProvider, Provider,
        Role, StreamEvent,
    };
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::sync::broadcast;

    const SEND: &str = "GMAIL_SEND_EMAIL";

    /// Stands in for Gmail's send tool, counting the mail it sends.
    struct Send(Arc<AtomicUsize>);

    #[async_trait]
    impl Tool for Send {
        fn name(&self) -> &str {
            SEND
        }
        fn description(&self) -> &str {
            "Send an email."
        }
        fn parameters(&self) -> Value {
            json!({"type": "object"})
        }
        async fn execute(&self, ctx: ToolContext<'_>) -> Result<ToolResult> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(ToolResult::success(ctx.tool_call_id, "sent"))
        }
    }

    /// Sends one mail, then reports what the send call gave back.
    struct Sender;

    impl LlmProvider for Sender {
        fn name(&self) -> &str {
            "test"
        }
        fn provider(&self) -> Provider {
            Provider::OpenAi
        }
        fn complete(&self, request: CompletionRequest) -> CompletionStream {
            let result = request
                .messages
                .iter()
                .rev()
                .find(|message| message.role == Role::Tool)
                .map(|message| message.content.clone());
            let (name, arguments) = match result {
                None => (
                    SEND,
                    json!({"recipient_email": "ana@example.com", "body": "Hi"}),
                ),
                Some(result) => ("report", json!({"summary": result, "status": "done"})),
            };
            Box::pin(async_stream::stream! {
                yield Ok(StreamEvent::ToolCallStart { index: 0, id: Uuid::new_v4().to_string(), name: name.to_owned() });
                yield Ok(StreamEvent::ToolCallDelta { index: 0, arguments: arguments.to_string() });
                yield Ok(StreamEvent::Finished(FinishReason::ToolCalls));
            })
        }
    }

    struct Fixture {
        _db: TestDb,
        store: Store,
        user: Arc<User>,
        chat: Uuid,
        subagents: Arc<Subagents>,
        notices: broadcast::Receiver<UserNotice>,
        sent: Arc<AtomicUsize>,
    }

    async fn fixture(level: PermissionLevel) -> Fixture {
        let db = TestDb::new().await.expect("a test database");
        let store = db.store().await.expect("a store");
        let user = User::for_test(store.clone(), "alice");
        user.save_app_permissions(&AppPermissionRow {
            app: "gmail".to_owned(),
            level,
            allowed: Vec::new(),
        })
        .await
        .expect("saved rules");
        let (notifications, notices) = broadcast::channel(8);
        let model = "openai:test".parse().expect("a model id");
        let subagents = Arc::new(Subagents::new(
            Arc::new(Sender),
            AgentLlmConfig::new(model),
            notifications,
        ));
        Fixture {
            _db: db,
            store,
            user,
            chat: Uuid::new_v4(),
            subagents,
            notices,
            sent: Arc::new(AtomicUsize::new(0)),
        }
    }

    impl Fixture {
        async fn gate(&self) -> Arc<dyn ToolGate> {
            Arc::new(Gate {
                rules: self.user.app_rules().await.expect("rules"),
                hinted: HashSet::new(),
                user: self.user.clone(),
                chat: self.chat,
                approvals: self.subagents.approvals.clone(),
                grants: None,
            })
        }

        async fn spec(&self) -> Spec<Report> {
            Spec {
                kind: &GENERAL,
                model: RoleModel {
                    model: "openai:test".parse().expect("a model id"),
                    thinking: Default::default(),
                },
                routing: None,
                tools: vec![Arc::new(Send(self.sent.clone()))],
                decisions: vec![report()],
                task: "Write to Ana.".to_owned(),
                context: None,
                watch: None,
                gate: Some(self.gate().await),
            }
        }

        async fn jobs(&self) -> ToolRegistry {
            let mut tools = ToolRegistry::new();
            tools.register(Arc::new(Send(self.sent.clone())));
            tools.set_gate(self.gate().await);
            tools
        }

        /// The card the next notice asks with, as its query id.
        async fn asked(&mut self) -> String {
            let notice = timeout(Duration::from_secs(5), self.notices.recv())
                .await
                .expect("a card in time")
                .expect("a notice");
            assert_eq!(notice.user, "alice");
            assert_eq!(notice.notice.session_id, self.chat.to_string());
            let pending = notice.notice.approval.expect("an approval card");
            assert_eq!(pending.approval.app, "Gmail");
            pending.query_id
        }
    }

    async fn run_agent(fixture: &Fixture) -> tokio::task::JoinHandle<Outcome> {
        let (subagents, spec) = (fixture.subagents.clone(), fixture.spec().await);
        tokio::spawn(async move { outcome_of(subagents.run_or_reply("alice", spec).await) })
    }

    #[tokio::test]
    async fn a_sub_agents_acting_call_waits_for_the_users_ok_then_runs() {
        let mut fixture = fixture(PermissionLevel::Ask {}).await;
        let running = run_agent(&fixture).await;
        let query_id = fixture.asked().await;
        assert_eq!(
            fixture.sent.load(Ordering::SeqCst),
            0,
            "nothing sent before the OK"
        );
        assert_eq!(
            fixture.subagents.approvals.waiting_for("alice"),
            std::slice::from_ref(&query_id)
        );
        assert!(
            !fixture
                .subagents
                .approvals
                .answer(
                    &User::for_test(fixture.store.clone(), "bob"),
                    &query_id,
                    ApprovalDecision::Allow {}
                )
                .await
                .expect("an answer"),
            "only the user it asked can answer"
        );
        assert!(
            fixture
                .subagents
                .approvals
                .answer(&fixture.user, &query_id, ApprovalDecision::Allow {})
                .await
                .expect("an answer")
        );
        let outcome = running.await.expect("no panic");
        assert_eq!(fixture.sent.load(Ordering::SeqCst), 1);
        assert_eq!(
            outcome,
            Outcome {
                ok: true,
                text: "sent".to_owned()
            }
        );
        assert!(fixture.subagents.approvals.waiting_for("alice").is_empty());
    }

    #[tokio::test]
    async fn a_denied_call_is_refused_and_never_runs() {
        let mut fixture = fixture(PermissionLevel::Ask {}).await;
        let running = run_agent(&fixture).await;
        let query_id = fixture.asked().await;
        fixture
            .subagents
            .approvals
            .answer(&fixture.user, &query_id, ApprovalDecision::Deny {})
            .await
            .expect("an answer");
        let outcome = running.await.expect("no panic");
        assert_eq!(fixture.sent.load(Ordering::SeqCst), 0);
        assert_eq!(outcome.text, permissions::DENIED);
    }

    #[tokio::test]
    async fn read_only_refuses_without_asking_and_allow_runs_without_asking() {
        let mut fixture = fixture(PermissionLevel::ReadOnly {}).await;
        let outcome = run_agent(&fixture).await.await.expect("no panic");
        assert_eq!(fixture.sent.load(Ordering::SeqCst), 0);
        assert!(outcome.text.contains("Gmail to read only"), "{outcome:?}");
        assert!(fixture.notices.try_recv().is_err(), "nothing asked");

        let mut fixture = self::fixture(PermissionLevel::Allow {}).await;
        let outcome = run_agent(&fixture).await.await.expect("no panic");
        assert_eq!(fixture.sent.load(Ordering::SeqCst), 1);
        assert_eq!(outcome.text, "sent");
        assert!(fixture.notices.try_recv().is_err(), "nothing asked");
    }

    #[tokio::test]
    async fn running_work_follows_rules_changed_after_it_started() {
        let mut fixture = fixture(PermissionLevel::Allow {}).await;
        let tools = fixture.jobs().await;
        fixture
            .user
            .save_app_permissions(&AppPermissionRow {
                app: "gmail".to_owned(),
                level: PermissionLevel::ReadOnly {},
                allowed: Vec::new(),
            })
            .await
            .expect("saved rules");
        let outcome = run_job(
            &tools,
            &send_call(),
            &Watch::default(),
            Duration::from_secs(30),
        )
        .await;
        assert!(outcome.text.contains("Gmail to read only"), "{outcome:?}");
        assert_eq!(fixture.sent.load(Ordering::SeqCst), 0);
        assert!(fixture.notices.try_recv().is_err(), "nothing asked");
    }

    fn send_call() -> ToolCall {
        ToolCall {
            id: "job-1".to_owned(),
            name: SEND.to_owned(),
            arguments: json!({"recipient_email": "ana@example.com"}),
        }
    }

    #[tokio::test]
    async fn a_job_wrapping_an_acting_tool_asks_like_a_turn() {
        let mut fixture = fixture(PermissionLevel::Ask {}).await;
        let tools = fixture.jobs().await;
        let watch = Watch::default();
        let job = {
            let watch = watch.clone();
            let tools = tools.clone();
            tokio::spawn(async move {
                run_job(&tools, &send_call(), &watch, Duration::from_secs(30)).await
            })
        };
        let query_id = fixture.asked().await;
        assert_eq!(fixture.sent.load(Ordering::SeqCst), 0);
        fixture
            .subagents
            .approvals
            .answer(&fixture.user, &query_id, ApprovalDecision::Always {})
            .await
            .expect("an answer");
        let outcome = job.await.expect("no panic");
        assert!(outcome.ok);
        assert_eq!(fixture.sent.load(Ordering::SeqCst), 1);

        // Always saved the action, so the next job of the same gate sends
        // without asking.
        let outcome = run_job(&tools, &send_call(), &watch, Duration::from_secs(30)).await;
        assert!(outcome.ok);
        assert_eq!(fixture.sent.load(Ordering::SeqCst), 2);
        assert!(
            fixture.notices.try_recv().is_err(),
            "nothing asked the second time"
        );
    }

    #[tokio::test]
    async fn a_cancelled_job_stops_waiting_and_withdraws_its_card() {
        let mut fixture = fixture(PermissionLevel::Ask {}).await;
        let tools = fixture.jobs().await;
        let watch = Watch::default();
        let job = {
            let watch = watch.clone();
            tokio::spawn(async move {
                run_job(&tools, &send_call(), &watch, Duration::from_secs(30)).await
            })
        };
        fixture.asked().await;
        watch.cancel.cancel();
        let outcome = job.await.expect("no panic");
        assert!(!outcome.ok);
        assert!(outcome.text.contains("cancelled"), "{outcome:?}");
        assert_eq!(fixture.sent.load(Ordering::SeqCst), 0);
        assert!(fixture.subagents.approvals.waiting_for("alice").is_empty());
    }

    /// Answers with a lot of text.
    struct Chatty;

    #[async_trait]
    impl Tool for Chatty {
        fn name(&self) -> &str {
            "chatty"
        }
        fn description(&self) -> &str {
            "Says a lot."
        }
        fn parameters(&self) -> Value {
            json!({"type": "object"})
        }
        fn max_output_bytes(&self) -> usize {
            usize::MAX
        }
        fn max_output_lines(&self) -> usize {
            usize::MAX
        }
        async fn execute(&self, ctx: ToolContext<'_>) -> Result<ToolResult> {
            let mut text = String::from("first line\n");
            for n in 0..20_000 {
                text.push_str(&format!("line {n}\n"));
            }
            text.push_str("last line");
            Ok(ToolResult::success(ctx.tool_call_id, text))
        }
    }

    #[tokio::test]
    async fn a_long_result_keeps_its_start_and_end() {
        let mut tools = ToolRegistry::new();
        tools.register(Arc::new(Chatty));
        let call = ToolCall {
            id: "job-1".to_owned(),
            name: "chatty".to_owned(),
            arguments: json!({}),
        };
        let outcome = run_job(&tools, &call, &Watch::default(), Duration::from_secs(5)).await;
        assert!(outcome.text.starts_with("first line"));
        assert!(outcome.text.ends_with("last line"));
        assert!(outcome.text.contains("[... output cut ...]"));
        assert!(outcome.text.len() < 17 * 1024);
    }
}
