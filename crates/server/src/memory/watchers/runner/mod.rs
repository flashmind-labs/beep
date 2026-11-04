//! Runs the watchers: a minute sweep plus wakes from ended tasks, a few reviews at a time.

use super::{
    WatcherKind, WatchersConfig,
    transcript::{BUDGET, Transcript, transcript},
    trigger::{self, Cause, MAX_FAILURES, Timing},
};
use crate::{
    account::user::User,
    app::App,
    lock,
    memory::starters::Generator,
    store::{StoredBody, StoredMessage},
};
use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use chrono::{DateTime, TimeDelta, Utc};
use flashmind::{core::EntryKind, types::AgentLlmConfig};
use futures::FutureExt;
use std::{
    collections::{HashMap, HashSet},
    panic::AssertUnwindSafe,
    sync::{Arc, Mutex, Weak},
    time::Duration,
};
use tokio::{
    sync::{Semaphore, mpsc},
    task::JoinSet,
    time::{MissedTickBehavior, interval, timeout},
};
use tokio_util::task::AbortOnDropHandle;
use uuid::Uuid;

/// How many ended tasks may wait for a read; more are read once quiet.
const WAKES: usize = 64;
/// Chats asked for by tasks that wait for a free watcher at most.
const WOKEN_MAX: usize = 256;
/// The most messages one review loads.
const MAX_MESSAGES: i64 = 400;

/// One watcher's job.
#[async_trait]
pub(crate) trait Watcher: Send + Sync {
    fn kind(&self) -> WatcherKind;

    /// Whether the new `messages` are worth a review; if not they are marked read.
    fn worth_reading(&self, _messages: &[StoredMessage]) -> bool {
        true
    }

    /// Reads `review` and acts on it. An error keeps the watcher's place.
    async fn review(&self, review: &Review<'_>) -> Result<()>;
}

/// What one review reads and works with.
pub(crate) struct Review<'a> {
    pub app: &'a Arc<App>,
    pub user: &'a Arc<User>,
    pub chat: Uuid,
    pub transcript: &'a Transcript,
    /// Completions on the watcher's own model, metered to the user.
    pub generator: Generator<'a>,
}

/// How the watchers run.
#[derive(Clone)]
pub struct Settings {
    pub timing: TimingSettings,
    /// How often ready chats are looked for.
    pub sweep_every: Duration,
    /// Older chats and messages are left alone, so the first sweep skips history.
    pub lookback: Duration,
    /// Reviews running at once across the server.
    pub concurrency: usize,
    pub watchers: Vec<WatcherSettings>,
}

/// The waits of [`Settings`].
#[derive(Debug, Clone, Copy)]
pub struct TimingSettings {
    pub quiet: Duration,
    pub retry: Duration,
}

/// One watcher's model and time limit.
#[derive(Clone)]
pub struct WatcherSettings {
    pub kind: WatcherKind,
    pub llm: AgentLlmConfig,
    pub timeout: Duration,
}

impl Settings {
    /// From the config, with `base`'s provider routing for every model.
    pub fn new(config: &WatchersConfig, base: &AgentLlmConfig) -> Self {
        let watchers = WatcherKind::ALL
            .into_iter()
            .filter(|kind| config.watcher(*kind).enabled)
            .map(|kind| {
                let mut llm = AgentLlmConfig::new(config.model(kind).clone());
                llm.provider_preferences = base.provider_preferences.clone();
                WatcherSettings {
                    kind,
                    llm,
                    timeout: Duration::from_secs(config.watcher(kind).timeout_secs),
                }
            })
            .collect();
        Self {
            timing: TimingSettings {
                quiet: config.quiet(),
                retry: Duration::from_secs(10 * 60),
            },
            sweep_every: Duration::from_secs(60),
            lookback: Duration::from_secs(2 * 24 * 60 * 60),
            concurrency: config.concurrency.max(1),
            watchers,
        }
    }

    fn timing(&self) -> Timing {
        Timing {
            quiet: delta(self.timing.quiet),
            retry: delta(self.timing.retry),
        }
    }
}

fn delta(duration: Duration) -> TimeDelta {
    TimeDelta::from_std(duration).unwrap_or(TimeDelta::MAX)
}

/// A task that ended in `chat`.
struct Wake {
    user: String,
    chat: Uuid,
}

/// The app's handle on the running watchers.
#[derive(Default)]
pub struct Hub {
    wakes: Mutex<Option<mpsc::Sender<Wake>>>,
    /// The watchers that run, once started.
    kinds: Mutex<Vec<WatcherKind>>,
    task: Mutex<Option<AbortOnDropHandle<()>>>,
}

impl Hub {
    /// Asks for `chat` to be read now that a task in it ended. No-op before `start`.
    pub fn task_ended(&self, user: &str, chat: Uuid) {
        let Some(wakes) = lock(&self.wakes).clone() else {
            return;
        };
        let wake = Wake {
            user: user.to_owned(),
            chat,
        };
        if wakes.try_send(wake).is_err() {
            log::info!("watchers: too many ended tasks waiting; chat={chat} is read once quiet");
        }
    }

    /// Whether the watcher `kind` runs, for `user`'s chats.
    pub fn watches(&self, kind: WatcherKind) -> bool {
        lock(&self.kinds).contains(&kind)
    }

    /// Stops the watchers; running reviews run again after the next start.
    pub fn stop(&self) {
        lock(&self.wakes).take();
        lock(&self.task).take();
    }
}

/// Starts the watchers. Starting again does nothing.
pub fn start(app: &Arc<App>, settings: Settings) {
    let mut task = lock(&app.watchers.task);
    if task.is_some() {
        return;
    }
    let (wakes, queue) = mpsc::channel(WAKES);
    let watchers = settings
        .watchers
        .iter()
        .map(|slot| (built_in(slot.kind), slot.clone()))
        .collect();
    *lock(&app.watchers.kinds) = settings.watchers.iter().map(|slot| slot.kind).collect();
    let runner = Runner::new(app, settings, watchers);
    *lock(&app.watchers.wakes) = Some(wakes);
    *task = Some(AbortOnDropHandle::new(tokio::spawn(run(runner, queue))));
}

fn built_in(kind: WatcherKind) -> Arc<dyn Watcher> {
    match kind {
        WatcherKind::Memory => Arc::new(super::memory::MemoryWatcher),
        WatcherKind::Skills => Arc::new(super::skills::SkillsWatcher),
        WatcherKind::Improvement => Arc::new(super::improvement::ImprovementWatcher),
    }
}

async fn run(mut runner: Runner, mut queue: mpsc::Receiver<Wake>) {
    let mut ticks = interval(runner.settings.sweep_every);
    ticks.set_missed_tick_behavior(MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            wake = queue.recv() => {
                let Some(wake) = wake else { return };
                runner.wake(wake.user, wake.chat);
                runner.tick(Utc::now()).await;
            }
            _ = ticks.tick() => runner.tick(Utc::now()).await,
            Some(done) = runner.running.join_next() => {
                if let Err(error) = done {
                    log::warn!("watchers: a review stopped: {error}");
                }
            }
        }
    }
}

/// One review to run.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Job {
    pub chat: Uuid,
    pub user: String,
    pub kind: WatcherKind,
    /// The last message the watcher read.
    pub after: i64,
    /// The chat's newest message when the job was planned.
    pub upto: i64,
}

/// Owns every review task and what they share.
pub(crate) struct Runner {
    app: Weak<App>,
    settings: Settings,
    watchers: Vec<(Arc<dyn Watcher>, WatcherSettings)>,
    limit: Arc<Semaphore>,
    busy: Arc<Mutex<HashSet<(Uuid, WatcherKind)>>>,
    /// Chats a task ended in, still to be read, with their user.
    woken: HashMap<Uuid, String>,
    pub(crate) running: JoinSet<()>,
}

/// Keeps a watcher busy while its review runs, even on panic.
struct Busy {
    set: Arc<Mutex<HashSet<(Uuid, WatcherKind)>>>,
    key: (Uuid, WatcherKind),
}

impl Drop for Busy {
    fn drop(&mut self) {
        lock(&self.set).remove(&self.key);
    }
}

impl Runner {
    pub(crate) fn new(
        app: &Arc<App>,
        settings: Settings,
        watchers: Vec<(Arc<dyn Watcher>, WatcherSettings)>,
    ) -> Self {
        Self {
            app: Arc::downgrade(app),
            limit: Arc::new(Semaphore::new(settings.concurrency)),
            settings,
            watchers,
            busy: Arc::default(),
            woken: HashMap::new(),
            running: JoinSet::new(),
        }
    }

    fn wake(&mut self, user: String, chat: Uuid) {
        if self.woken.len() < WOKEN_MAX {
            self.woken.insert(chat, user);
        }
    }

    /// Starts every review ready at `now`, up to the limit.
    pub(crate) async fn tick(&mut self, now: DateTime<Utc>) {
        let Some(app) = self.app.upgrade() else {
            return;
        };
        let jobs = match self.plan(&app, now).await {
            Ok(jobs) => jobs,
            Err(error) => {
                log::warn!("watchers: unable to find chats to read: {error:#}");
                return;
            }
        };
        let mut waiting = HashSet::new();
        for job in jobs {
            let key = (job.chat, job.kind);
            if lock(&self.busy).contains(&key) {
                continue;
            }
            let Ok(permit) = self.limit.clone().try_acquire_owned() else {
                waiting.insert(job.chat);
                continue;
            };
            lock(&self.busy).insert(key);
            let busy = Busy {
                set: self.busy.clone(),
                key,
            };
            let Some((watcher, slot)) = self
                .watchers
                .iter()
                .find(|(watcher, _)| watcher.kind() == job.kind)
                .cloned()
            else {
                continue;
            };
            let (app, lookback) = (app.clone(), self.settings.lookback);
            self.running.spawn(async move {
                let _held = (permit, busy);
                review(&app, watcher.as_ref(), &slot, &job, now, lookback).await;
            });
        }
        // A woken chat stays only while a review it asked for could not start.
        self.woken.retain(|chat, _| waiting.contains(chat));
    }

    /// The reviews ready at `now`: quiet chats and chats whose tasks ended.
    async fn plan(&self, app: &Arc<App>, now: DateTime<Utc>) -> Result<Vec<Job>> {
        let since = now - delta(self.settings.lookback);
        let mut chats = app.store.watched_chats(since).await?;
        let mut causes: HashMap<Uuid, Cause> = HashMap::new();
        for (chat, user) in &self.woken {
            causes.insert(*chat, Cause::TaskEnded);
            if chats.iter().any(|found| found.chat == *chat) {
                continue;
            }
            if let Some(found) = app.store.watched_chat(user, *chat).await? {
                chats.push(found);
            }
        }
        let ids: Vec<Uuid> = chats.iter().map(|chat| chat.chat).collect();
        let states = app.store.watcher_states(&ids).await?;
        let timing = self.settings.timing();
        let mut jobs = Vec::new();
        for chat in &chats {
            let cause = causes.get(&chat.chat).copied().unwrap_or(Cause::Sweep);
            for (watcher, _) in &self.watchers {
                let kind = watcher.kind();
                let state = states.get(&(chat.chat, kind));
                if trigger::due(chat, state, cause, now, timing) {
                    jobs.push(Job {
                        chat: chat.chat,
                        user: chat.user.clone(),
                        kind,
                        after: state.map_or(0, |state| state.reviewed_seq),
                        upto: chat.last_seq,
                    });
                }
            }
        }
        Ok(jobs)
    }

    /// Waits for every review under way, for tests.
    #[cfg(test)]
    pub(crate) async fn drain(&mut self) -> Vec<tokio::task::JoinError> {
        let mut errors = Vec::new();
        while let Some(done) = self.running.join_next().await {
            if let Err(error) = done {
                errors.push(error);
            }
        }
        errors
    }
}

/// Runs one review and moves its watcher's place, or counts a failure (moving on after `MAX_FAILURES`).
async fn review(
    app: &Arc<App>,
    watcher: &dyn Watcher,
    slot: &WatcherSettings,
    job: &Job,
    now: DateTime<Utc>,
    lookback: Duration,
) {
    let kind: &str = job.kind.into();
    let (chat, user) = (job.chat, &job.user);
    let attempt = timeout(
        slot.timeout,
        attempt(app, watcher, slot, job, now, lookback),
    );
    // A panic counts as a failure, so a broken watcher backs off too.
    match AssertUnwindSafe(attempt).catch_unwind().await {
        Ok(Ok(Ok(read))) => {
            if let Err(error) = app.store.mark_watched(chat, job.kind, read, now).await {
                log::warn!(
                    "watchers: unable to keep the {kind} watcher's place: {error:#}; chat={chat}"
                );
            }
            return;
        }
        Ok(Ok(Err(error))) => {
            log::warn!("watchers: the {kind} watcher failed: {error:#}; user={user} chat={chat}");
        }
        Ok(Err(_)) => {
            log::warn!("watchers: the {kind} watcher timed out; user={user} chat={chat}");
        }
        Err(_) => {
            log::error!("watchers: the {kind} watcher panicked; user={user} chat={chat}");
        }
    }
    let failures = match app.store.watch_failed(chat, job.kind, now).await {
        Ok(failures) => failures,
        Err(error) => {
            log::warn!("watchers: unable to count a failure: {error:#}; chat={chat}");
            return;
        }
    };
    if failures >= MAX_FAILURES {
        // What it could not read is given up on, up to where the chat was.
        log::warn!(
            "watchers: the {kind} watcher gave up after {failures} failures; chat={chat} upto={}",
            job.upto
        );
        if let Err(error) = app.store.mark_watched(chat, job.kind, job.upto, now).await {
            log::warn!("watchers: unable to move the {kind} watcher on: {error:#}; chat={chat}");
        }
    }
}

/// Hands what `job` covers to `watcher` and returns the last message read.
async fn attempt(
    app: &Arc<App>,
    watcher: &dyn Watcher,
    slot: &WatcherSettings,
    job: &Job,
    now: DateTime<Utc>,
    lookback: Duration,
) -> Result<i64> {
    let since = now - delta(lookback);
    let messages = app
        .store
        .chat_messages_after(job.chat, job.after, since, MAX_MESSAGES)
        .await?;
    if messages.is_empty() {
        // Only messages older than the lookback were left.
        return Ok(job.upto);
    }
    if !watcher.worth_reading(&messages) {
        return Ok(messages.last().map_or(job.upto, |message| message.id));
    }
    let user = app
        .user(&job.user)
        .ok_or_else(|| anyhow!("the user is not started"))?;
    let arguments = if messages
        .iter()
        .any(|message| matches!(message.body, StoredBody::Tool { .. }))
    {
        tool_arguments(&user, job.chat).await?
    } else {
        HashMap::new()
    };
    let transcript = transcript(&messages, &arguments, BUDGET);
    let review = Review {
        app,
        user: &user,
        chat: job.chat,
        transcript: &transcript,
        generator: app.generator_on(user.id(), &slot.llm),
    };
    watcher.review(&review).await?;
    Ok(transcript.last_seq)
}

/// The arguments of each tool call the chat's context still holds, by call id.
async fn tool_arguments(user: &User, chat: Uuid) -> Result<HashMap<String, String>> {
    let context = user.context(chat).await.context("loading the chat")?;
    let mut arguments = HashMap::new();
    for entry in context
        .iter()
        .flat_map(|context| context.conversation.entries())
    {
        if let EntryKind::Assistant {
            tool_calls: Some(calls),
            ..
        } = &entry.kind
        {
            for call in calls {
                arguments.insert(call.id.clone(), call.arguments.to_string());
            }
        }
    }
    Ok(arguments)
}

#[cfg(test)]
mod tests;
