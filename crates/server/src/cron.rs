use std::{path::Path, sync::Arc, time::Duration};

use anyhow::Result;
use async_trait::async_trait;
use flashmind::core::CancellationToken;
use flashmind::cron::{
    CronCreateTool, CronDeleteTool, CronEditTool, CronHandler, CronJob, CronListTool, CronLog,
    CronRegistry, CronRunner, CronStore, ScheduleOnceTool, TimezoneResolver,
};
use flashmind::types::{
    ToolRegistry,
    tool::{Tool, ToolContext, ToolResult},
};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::agents::AgentId;
use crate::app::{App, Draws, Message};
use crate::digest;
use crate::protocol::{DeviceKind, Run};
use crate::store::{CreateRun, Store};
use crate::user::User;

/// Cron jobs kept in the `crons` table, all of them or one user's.
pub struct BeepCronStore {
    store: Store,
    user: Option<String>,
}

impl BeepCronStore {
    pub fn for_user(store: Store, user: &str) -> Self {
        Self {
            store,
            user: Some(user.to_owned()),
        }
    }
}

#[async_trait]
impl CronStore for BeepCronStore {
    async fn list(&self) -> Result<Vec<CronJob>> {
        self.store.cron_jobs(self.user.as_deref()).await
    }

    async fn get(&self, id: Uuid) -> Result<Option<CronJob>> {
        self.store.cron_job(id, self.user.as_deref()).await
    }

    async fn upsert(&self, job: &CronJob) -> Result<()> {
        self.store.upsert_cron_job(job).await?;
        self.store.notify_crons_changed();
        Ok(())
    }

    async fn delete(&self, id: Uuid) -> Result<bool> {
        let deleted = self.store.delete_cron_job(id, self.user.as_deref()).await?;
        self.store.notify_crons_changed();
        Ok(deleted)
    }
}

/// The scheduler's view of the `crons` table: every job it runs. Its only
/// write after a run is when the job last ran. It reads the job before the
/// run starts, so writing the whole job back would bring back a routine
/// that `end_routine` or the user deleted meanwhile, and undo their edits.
struct SchedulerStore {
    store: Store,
}

#[async_trait]
impl CronStore for SchedulerStore {
    async fn list(&self) -> Result<Vec<CronJob>> {
        self.store.cron_jobs(None).await
    }

    async fn get(&self, id: Uuid) -> Result<Option<CronJob>> {
        self.store.cron_job(id, None).await
    }

    async fn upsert(&self, job: &CronJob) -> Result<()> {
        if !self.store.record_cron_run(job.id, job.last_run).await? {
            log::debug!(
                "cron job gone before its run was recorded; job_id={}",
                job.id
            );
        }
        Ok(())
    }

    async fn delete(&self, id: Uuid) -> Result<bool> {
        let deleted = self.store.delete_cron_job(id, None).await?;
        self.store.notify_crons_changed();
        Ok(deleted)
    }
}

/// The tool a routine's run ends its own routine with.
pub(crate) const END_ROUTINE: &str = "end_routine";

/// Starts the context line that marks a routine's turn.
const SCHEDULED_TASK: &str = "Scheduled task";

/// How the model answers a routine, sent only on routine turns.
const SCHEDULED_RULE: &str = "The message is the note you wrote yourself when you set up \
this task, not something the user said, and nobody is watching this chat. You see only \
this note, not the chat's earlier messages. Do the task. \
To tell the user something, call notify_user, then write it to them in one or two short, \
specific sentences of plain text, without Markdown; it is sent as the notification and \
added to this chat as your message. A reminder is always sent. A check is sent only when \
the note's condition is met, judged against what the note says is already known; \
otherwise do not call notify_user and end with one line on what you found, which the user \
never sees. A check that waits for one thing, such as a reply, a delivery or a launch, is \
done once you have told the user: then also call end_routine, so it stops and the user \
hears the news once. A routine meant to repeat, such as a daily digest or a weekly check, \
keeps running: never end it. Do not mention that a task ran or that this is a reminder. \
You cannot set up or change reminders or routines here; end_routine ends only this one.";

/// The tools that set up, change or delete what runs later. A routine's run
/// does not get them: runs that copied their own routine with a fresher
/// note left the user with the same check several times over. It gets
/// [`EndRoutineTool`] instead, which ends only itself.
pub const CHANGING_TOOLS: &[&str] = &[
    "cron_create",
    "cron_edit",
    "cron_delete",
    "schedule_once",
    "place_routine",
];

/// Takes [`CHANGING_TOOLS`] out of `registry`, for a routine's own run and
/// for a turn on text anyone can send.
pub(crate) fn remove_changing_tools(registry: &mut ToolRegistry) {
    for name in CHANGING_TOOLS {
        registry.remove(name);
    }
}

/// The context lines for a routine's turn. History reads the label back with
/// [`scheduled_label`] so clients show the turn as a routine, not a prompt.
pub(crate) fn scheduled_context(label: &str) -> String {
    let label = label.replace('\n', " ");
    let label = label.trim();
    if label.is_empty() {
        format!("{SCHEDULED_TASK}\n{SCHEDULED_RULE}")
    } else {
        format!("{SCHEDULED_TASK}: {label}\n{SCHEDULED_RULE}")
    }
}

/// The routine's label when `context` is a turn's context from
/// [`scheduled_context`], empty for an unlabelled routine, and `None` for a
/// turn the user started.
pub fn scheduled_label(context: &str) -> Option<&str> {
    context
        .lines()
        .find_map(|line| match line.strip_prefix(SCHEDULED_TASK)? {
            "" => Some(""),
            rest => rest.strip_prefix(": "),
        })
}

/// The label of a routine run before the context carried it, when its task
/// was sent as `[Scheduled: label] task`.
pub fn legacy_scheduled_label(content: &str) -> Option<&str> {
    if content.starts_with("[Scheduled] ") {
        return Some("");
    }
    let rest = content.strip_prefix("[Scheduled: ")?;
    let (label, _) = rest.split_once("] ")?;
    Some(label)
}

/// What a routine's run does when its chat is busy with another run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WhenBusy {
    /// Waits for that run to end, then starts in the chat. The scheduler
    /// does, so its notice still lands where the user set it up.
    Wait,
    /// Starts nothing, as when the user asks for a run now.
    GiveUp,
}

/// How long a routine waits for its chat: a run lasts 15 minutes at most.
const BUSY_WAIT: Duration = Duration::from_secs(16 * 60);

/// Starts a run for `job` and returns it, or `None` when none was created.
/// The scheduler calls it when the job is due, and the API when the user
/// runs it early.
pub async fn run_job(
    app: &Arc<App>,
    user: &Arc<User>,
    job: &CronJob,
    busy: WhenBusy,
) -> Result<Option<Run>> {
    if digest::is_digest(job) {
        return digest::run(app, user, job, busy).await;
    }
    submit(app, user, job, &job.task, busy).await
}

/// Wakes the agent with `task` as `job`'s run.
pub(crate) async fn submit(
    app: &Arc<App>,
    user: &Arc<User>,
    job: &CronJob,
    task: &str,
    busy: WhenBusy,
) -> Result<Option<Run>> {
    let label = job
        .metadata
        .get("label")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    // Run in the chat the routine was set up from, so its reply lands
    // there. A routine made outside a chat gets a chat on its first run and
    // keeps replying in it.
    let origin = match job
        .metadata
        .get("session_id")
        .and_then(|v| v.as_str())
        .and_then(|v| v.parse::<Uuid>().ok())
    {
        Some(id) => Some(id),
        None => {
            let claimed = app.store.claim_cron_session(job.id, Uuid::new_v4()).await?;
            if claimed.is_some() {
                app.store.notify_crons_changed();
            }
            claimed
        }
    };
    let chat = origin.unwrap_or_else(Uuid::new_v4);
    let agent = job
        .metadata
        .get("agent")
        .and_then(|v| v.as_str())
        .and_then(|v| v.parse().ok());
    let msg = || Message {
        session_id: chat,
        request_id: Uuid::new_v4(),
        message: task.to_owned(),
        cron_id: Some(job.id.to_string()),
        agent,
        scheduled: Some(label.to_owned()),
        always_sent: digest::is_digest(job),
        woken: None,
        images: Vec::new(),
        files: Vec::new(),
        answer: None,
        model: None,
        thinking: None,
        origin: DeviceKind::Phone {},
        platform: None,
        draws: Draws::default(),
        finished_research: false,
        voice: None,
        spoken: false,
    };

    let mut result = app.submit(user, msg()).await;
    // The chat is busy with another run, maybe this routine's last one.
    // Waiting keeps the notice in its chat, where a run in a new chat would
    // leave it with nothing around it, and runs of one routine one at a time.
    if busy == WhenBusy::Wait
        && matches!(result, Ok(CreateRun::Conflict))
        && app.run_ended(user, chat, BUSY_WAIT).await?
    {
        result = app.submit(user, msg()).await;
    }
    match result {
        Ok(CreateRun::Created(run)) => Ok(Some(run)),
        Ok(_) => {
            log::info!("cron run not created, its chat is busy; job_id={}", job.id);
            Ok(None)
        }
        Err(e) => {
            log::error!("failed to submit cron run: {e}; job_id={}", job.id);
            Err(e)
        }
    }
}

/// Ends the routine whose run calls it, and no other: a check that waits
/// for one thing stops once it has told the user. It takes no arguments,
/// so a run cannot name another routine.
pub(crate) struct EndRoutineTool {
    store: Store,
    user: String,
    routine: Uuid,
}

impl EndRoutineTool {
    pub(crate) fn new(store: Store, user: &str, routine: Uuid) -> Self {
        Self {
            store,
            user: user.to_owned(),
            routine,
        }
    }
}

#[async_trait]
impl Tool for EndRoutineTool {
    fn name(&self) -> &str {
        END_ROUTINE
    }

    fn description(&self) -> &str {
        "End the routine running now, so it never runs again. Call it once a check that \
         waits for one thing has told the user. It ends only this routine."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {},
            "additionalProperties": false
        })
    }

    async fn execute(&self, ctx: ToolContext<'_>) -> Result<ToolResult> {
        let deleted = self
            .store
            .delete_cron_job(self.routine, Some(&self.user))
            .await?;
        self.store.notify_crons_changed();
        Ok(if deleted {
            ToolResult::success(
                ctx.tool_call_id,
                "This routine is ended and will not run again.",
            )
        } else {
            ToolResult::success(ctx.tool_call_id, "This routine had already ended.")
        })
    }

    fn humanize(&self, _args: &Value) -> String {
        "Ending this routine".to_owned()
    }
}

struct BeepCronHandler {
    app: Arc<App>,
}

#[async_trait]
impl CronHandler for BeepCronHandler {
    async fn execute(self: Arc<Self>, job: &CronJob) -> Result<Option<String>> {
        let user_id = job
            .metadata
            .get("user_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("job missing user_id in metadata"))?;
        let Some(user) = self.app.user(user_id) else {
            log::warn!("cron job for an unknown user; job_id={}", job.id);
            return Ok(None);
        };
        if let Some(group_id) = job
            .metadata
            .get("group_id")
            .and_then(|v| v.as_str())
            .and_then(|v| v.parse::<Uuid>().ok())
        {
            self.app
                .post_group_reminder(&user, group_id, &job.task)
                .await?;
            return Ok(None);
        }
        let run = run_job(&self.app, &user, job, WhenBusy::Wait).await?;
        Ok(run.map(|run| run.id))
    }
}

struct BeepTimezoneResolver {
    app: Arc<App>,
}

#[async_trait]
impl TimezoneResolver for BeepTimezoneResolver {
    async fn resolve(&self, job: &CronJob) -> Option<chrono_tz::Tz> {
        let user_id = job.metadata.get("user_id")?.as_str()?;
        let tz_name = self.app.user(user_id)?.get_timezone().await.ok()?;
        tz_name.parse().ok()
    }
}

pub fn start(app: Arc<App>, log_dir: &Path) -> CronRunner {
    let store = Arc::new(SchedulerStore {
        store: app.store.clone(),
    });
    let registry = Arc::new(CronRegistry::new(store));
    let handler = Arc::new(BeepCronHandler { app: app.clone() });
    let cancel = CancellationToken::new();

    let log_path = log_dir.join("cron.log");
    let log = Arc::new(CronLog::new(log_path));

    let tz = Arc::new(BeepTimezoneResolver { app: app.clone() });

    // The agent's tools and the API write through their own stores, whose
    // registries the runner never hears from. Pass their writes on to it.
    let changed = app.store.crons_changed();
    let reload = registry.notify();
    tokio::spawn(async move {
        loop {
            changed.notified().await;
            reload.notify_one();
        }
    });

    CronRunner::new(registry, handler, cancel)
        .with_log(log)
        .with_timezone_resolver(tz)
}

pub fn register_tools(
    registry: &mut ToolRegistry,
    store: Store,
    user: &str,
    session_id: &str,
    timezone: Option<&str>,
    agent: Option<AgentId>,
) {
    let cron_store = Arc::new(BeepCronStore::for_user(store, user));
    let cron_registry = Arc::new(CronRegistry::new(cron_store));
    // A routine made here runs as the agent this turn is.
    let agent = agent.map(AgentId::as_str);
    let user_meta = json!({"user_id": user, "session_id": session_id, "agent": agent});

    registry.register(Arc::new(CronCreateTool::with_metadata(
        cron_registry.clone(),
        user_meta,
    )));
    registry.register(Arc::new(CronListTool::new(cron_registry.clone())));
    registry.register(Arc::new(CronEditTool::new(cron_registry.clone())));
    registry.register(Arc::new(CronDeleteTool::new(cron_registry.clone())));
    // schedule_once reads a datetime without an offset in this timezone; the
    // model writes local times like "2026-10-05T11:30:00".
    registry.register(Arc::new(ScheduleOnceTool::with_metadata(
        cron_registry,
        json!({"user_id": user, "session_id": session_id, "timezone": timezone, "agent": agent}),
    )));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{TestDb, decode_schedule};
    use chrono::{DateTime, TimeDelta, Utc};
    use flashmind::{cron::JobSchedule, types::tool::ToolContext};
    use std::time::Duration;

    #[test]
    fn scheduled_context_reads_back_its_label() {
        let context = |label| {
            format!(
                "Time: Monday 2026-10-05 09:00 +04\n{}",
                scheduled_context(label)
            )
        };
        assert_eq!(scheduled_label(&context("")), Some(""));
        assert_eq!(scheduled_label(&context("Plants")), Some("Plants"));
        assert_eq!(scheduled_label(&context("Two\nlines")), Some("Two lines"));
        assert_eq!(scheduled_label("Time: Monday 2026-10-05 09:00 +04"), None);
        assert_eq!(scheduled_label("Scheduled tasks are off"), None);
    }

    #[test]
    fn legacy_prompts_read_back_their_label() {
        assert_eq!(
            legacy_scheduled_label("[Scheduled] Water the plants"),
            Some("")
        );
        assert_eq!(
            legacy_scheduled_label("[Scheduled: Plants] Water them"),
            Some("Plants")
        );
        assert_eq!(legacy_scheduled_label("Water the plants"), None);
        assert_eq!(legacy_scheduled_label("[Scheduled]"), None);
    }

    #[test]
    fn one_off_time_reads_back_as_once() {
        let at = "2026-10-04T04:07:00+00:00"
            .parse::<DateTime<Utc>>()
            .expect("valid time");
        assert!(matches!(decode_schedule(at.to_rfc3339()), JobSchedule::Once(t) if t == at));
        assert!(
            matches!(decode_schedule("0 8 * * *".to_owned()), JobSchedule::Cron(e) if e == "0 8 * * *")
        );
    }

    #[tokio::test]
    async fn a_reminder_from_the_tools_wakes_the_scheduler() {
        let db = TestDb::new().await.expect("test database");
        let store = db.store().await.expect("store opens");
        let changed = store.crons_changed();
        let tools = CronRegistry::new(Arc::new(BeepCronStore::for_user(store, "user")));

        let at = Utc::now() + TimeDelta::minutes(1);
        tools
            .create_with_metadata(
                JobSchedule::Once(at),
                "ping".to_owned(),
                true,
                json!({"user_id": "user"}),
            )
            .await
            .expect("reminder saved");

        tokio::time::timeout(Duration::from_secs(1), changed.notified())
            .await
            .expect("saving a reminder notifies the scheduler");
    }

    #[tokio::test]
    async fn no_tool_that_sets_up_routines_is_left() {
        let db = TestDb::new().await.expect("test database");
        let store = db.store().await.expect("store opens");
        let mut registry = ToolRegistry::new();
        register_tools(&mut registry, store, "user", "session", None, None);
        assert!(registry.contains("cron_create"));
        remove_changing_tools(&mut registry);
        for name in CHANGING_TOOLS {
            assert!(!registry.contains(name), "{name} is gone");
        }
        assert!(registry.contains("cron_list"), "reading routines stays");
    }

    #[tokio::test]
    async fn a_local_reminder_time_is_read_in_the_users_timezone() {
        let db = TestDb::new().await.expect("test database");
        let store = db.store().await.expect("store opens");
        let mut registry = ToolRegistry::new();
        register_tools(
            &mut registry,
            store,
            "user",
            "session",
            Some("Asia/Singapore"),
            None,
        );
        let tool = registry
            .get("schedule_once")
            .expect("schedule_once registered");

        let cancel = CancellationToken::new();
        let args = json!({"datetime": "2099-10-05T11:30:00", "task": "ping"});
        let result = tool
            .execute(ToolContext::new("call", args, None, &cancel))
            .await
            .expect("tool runs");

        assert!(result.is_success(), "{}", result.output());
        assert!(
            result.output().contains("2099-10-05 03:30 UTC"),
            "{}",
            result.output()
        );
    }

    #[tokio::test]
    async fn a_reminder_remembers_the_chat_and_agent_it_was_set_in() {
        let db = TestDb::new().await.expect("test database");
        let store = db.store().await.expect("store opens");
        let mut registry = ToolRegistry::new();
        let chat = Uuid::new_v4().to_string();
        register_tools(
            &mut registry,
            store.clone(),
            "user",
            &chat,
            None,
            Some(AgentId::GettingAround),
        );
        let tool = registry
            .get("schedule_once")
            .expect("schedule_once registered");

        let cancel = CancellationToken::new();
        let args = json!({"datetime": "2099-01-01T08:00:00Z", "task": "ping"});
        tool.execute(ToolContext::new("call", args, None, &cancel))
            .await
            .expect("tool runs");

        let jobs = store.cron_jobs(Some("user")).await.expect("jobs load");
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].metadata["session_id"], chat.as_str());
        assert_eq!(jobs[0].metadata["agent"], "getting_around");
    }
    #[tokio::test]
    async fn a_routine_made_outside_a_chat_keeps_its_first_chat() {
        let db = TestDb::new().await.expect("test database");
        let store = db.store().await.expect("store opens");
        let tools = CronRegistry::new(Arc::new(BeepCronStore::for_user(store.clone(), "user")));
        let job = tools
            .create_with_metadata(
                JobSchedule::Cron("0 20 * * *".to_owned()),
                "evening digest".to_owned(),
                false,
                json!({"user_id": "user"}),
            )
            .await
            .expect("routine saved");

        let first = store
            .claim_cron_session(job.id, Uuid::new_v4())
            .await
            .expect("claim runs")
            .expect("routine exists");
        let second = store
            .claim_cron_session(job.id, Uuid::new_v4())
            .await
            .expect("claim runs");
        assert_eq!(second, Some(first));
        let jobs = store.cron_jobs(Some("user")).await.expect("jobs load");
        assert_eq!(jobs[0].metadata["session_id"], first.to_string());
    }

    #[tokio::test]
    async fn the_scheduler_never_brings_back_or_undoes_a_routine() {
        let db = TestDb::new().await.expect("test database");
        let store = db.store().await.expect("store opens");
        let tools = CronRegistry::new(Arc::new(BeepCronStore::for_user(store.clone(), "user")));
        let scheduler = CronRegistry::new(Arc::new(SchedulerStore {
            store: store.clone(),
        }));
        let job = tools
            .create_with_metadata(
                JobSchedule::Cron("*/15 * * * *".to_owned()),
                "Check whether Sam replied".to_owned(),
                false,
                json!({"user_id": "user", "label": "Sam"}),
            )
            .await
            .expect("routine saved");

        // The scheduler read the job, then the user renamed it while it ran.
        let mut ran = scheduler
            .get(job.id)
            .await
            .expect("job loads")
            .expect("job exists");
        let mut renamed = job.clone();
        renamed.metadata["label"] = json!("Sam's reply");
        tools.update(&renamed).await.expect("rename saved");
        ran.last_run = Some(Utc::now());
        scheduler.update(&ran).await.expect("run recorded");
        let now = store
            .cron_job(job.id, None)
            .await
            .expect("job loads")
            .expect("job exists");
        assert_eq!(now.metadata["label"], "Sam's reply");
        assert!(now.last_run.is_some());

        // The run ended its routine before the scheduler recorded it.
        let tool = EndRoutineTool::new(store.clone(), "user", job.id);
        let cancel = CancellationToken::new();
        tool.execute(ToolContext::new("call", json!({}), None, &cancel))
            .await
            .expect("tool runs");
        scheduler.update(&ran).await.expect("run recorded");
        assert!(
            store
                .cron_job(job.id, None)
                .await
                .expect("job loads")
                .is_none()
        );
    }

    #[tokio::test]
    async fn a_routine_ends_itself_and_no_other() {
        let db = TestDb::new().await.expect("test database");
        let store = db.store().await.expect("store opens");
        let routine = |user: &str| {
            let tools = CronRegistry::new(Arc::new(BeepCronStore::for_user(store.clone(), user)));
            let user = user.to_owned();
            async move {
                tools
                    .create_with_metadata(
                        JobSchedule::Cron("*/15 * * * *".to_owned()),
                        "Check whether Sam replied".to_owned(),
                        false,
                        json!({"user_id": user}),
                    )
                    .await
                    .expect("routine saved")
            }
        };
        let running = routine("user").await;
        let other = routine("user").await;
        let theirs = routine("bob").await;
        let tool = EndRoutineTool::new(store.clone(), "user", running.id);

        // Naming another routine still ends only the one running.
        let cancel = CancellationToken::new();
        let args = json!({"id": other.id.to_string()});
        let result = tool
            .execute(ToolContext::new("call", args, None, &cancel))
            .await
            .expect("tool runs");
        assert!(result.is_success(), "{}", result.output());

        let left: Vec<Uuid> = store
            .cron_jobs(None)
            .await
            .expect("jobs load")
            .into_iter()
            .map(|job| job.id)
            .collect();
        assert!(!left.contains(&running.id));
        assert!(left.contains(&other.id));
        assert!(left.contains(&theirs.id));

        // A run bound to someone else's routine cannot end it.
        let foreign = EndRoutineTool::new(store.clone(), "user", theirs.id);
        foreign
            .execute(ToolContext::new("call", json!({}), None, &cancel))
            .await
            .expect("tool runs");
        let theirs_left = store.cron_job(theirs.id, None).await.expect("job loads");
        assert!(theirs_left.is_some());
    }
}
