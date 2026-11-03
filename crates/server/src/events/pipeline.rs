//! The one path every account event takes: accepted or dropped, stored
//! once, batched per user and handed to the triage agent, which decides
//! whether the main agent hears about it (see [`super::triage`]).

use super::{
    AppEvent, EventApp,
    limit::{Limits, RateLimiter},
    poll, realtime, triage, triggers,
};
use crate::{
    app::App,
    lock,
    memories::clip,
    research::RoleModel,
    store::{EventRow, EventState},
    subagents::Pool,
    user::User,
    wake::{self, Event, Rule},
};
use anyhow::Result;
use chrono::{Duration as Days, Utc};
use flashmind::core::CancellationToken;
use futures::{StreamExt, stream};
use serde_json::Value;
use std::{
    collections::HashMap,
    fmt::Write,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::{
    sync::mpsc,
    task::JoinHandle,
    time::{MissedTickBehavior, interval},
};

/// Events waiting for the worker. A full queue makes senders wait.
const QUEUE: usize = 256;
/// Events older than this when they arrive are dropped, so a trigger's
/// first run or a long outage does not replay old news.
const MAX_AGE: Days = Days::days(2);
/// How long stored events are kept.
const KEEP: Days = Days::days(14);
/// Users swept at once.
const SWEEP_CONCURRENCY: usize = 4;
/// How long a sweep waits for one user's polls and triggers.
const SWEEP_USER_TIMEOUT: Duration = Duration::from_secs(120);
/// What a wake label shows of a single event.
const LABEL_CHARS: usize = 60;

/// How the pipeline paces itself.
#[derive(Debug, Clone)]
pub struct Settings {
    /// How long after a user's first waiting event the batch goes out, so
    /// events that come together wake the agent once.
    pub batch_window: Duration,
    /// How often waiting batches are checked.
    pub tick: Duration,
    pub limits: Limits,
    /// Most events in one wake; the rest go in the next.
    pub max_batch: i64,
    /// How long held events wait before they come back as a digest.
    pub digest_after: Duration,
    /// How often each user's polls, triggers and held events are seen to.
    pub sweep_every: Duration,
    /// Whether the sweep polls and sets up triggers. Off in tests.
    pub poll: bool,
    /// The model the triage agent runs on (`[events] triage_model`).
    pub triage: RoleModel,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            batch_window: Duration::from_secs(90),
            tick: Duration::from_secs(10),
            limits: Limits {
                events_per_hour: 200,
                wakes_per_hour: 6,
                triages_per_hour: 30,
            },
            max_batch: 25,
            digest_after: Duration::from_secs(3 * 60 * 60),
            sweep_every: Duration::from_secs(5 * 60),
            poll: true,
            triage: triage::default_triage_model(),
        }
    }
}

/// An event for one user, on its way to the worker.
struct Incoming {
    user: String,
    event: AppEvent,
}

/// The pipeline's shared state, kept on [`App`].
pub struct Hub {
    queue: mpsc::Sender<Incoming>,
    /// Taken by [`start`].
    receiver: Mutex<Option<mpsc::Receiver<Incoming>>>,
    stop: CancellationToken,
    /// Users with stored events waiting for a wake, and since when.
    pending: Mutex<HashMap<String, Instant>>,
    limiter: Mutex<RateLimiter>,
    /// Set while the realtime channel is subscribed.
    realtime: AtomicBool,
    tasks: Mutex<Vec<JoinHandle<()>>>,
}

impl Default for Hub {
    fn default() -> Self {
        let (queue, receiver) = mpsc::channel(QUEUE);
        Self {
            queue,
            receiver: Mutex::new(Some(receiver)),
            stop: CancellationToken::new(),
            pending: Mutex::default(),
            limiter: Mutex::new(RateLimiter::new(Settings::default().limits)),
            realtime: AtomicBool::new(false),
            tasks: Mutex::default(),
        }
    }
}

impl Hub {
    /// Ends the pipeline's tasks.
    pub fn stop(&self) {
        self.stop.cancel();
        for task in lock(&self.tasks).drain(..) {
            task.abort();
        }
    }

    pub(crate) fn stopping(&self) -> &CancellationToken {
        &self.stop
    }

    pub(crate) fn set_realtime(&self, up: bool) {
        self.realtime.store(up, Ordering::Relaxed);
    }

    pub(crate) fn realtime_up(&self) -> bool {
        self.realtime.load(Ordering::Relaxed)
    }

    /// Whether `user` may wake the main agent now, counting it if so.
    pub(super) fn take_wake(&self, user: &str) -> bool {
        lock(&self.limiter).take_wake(user, Instant::now())
    }

    /// Uncounts a wake taken for `user` that started no run.
    pub(super) fn give_back_wake(&self, user: &str) {
        lock(&self.limiter).give_back_wake(user);
    }

    /// Queues `user`'s next triage, counting the wait from now.
    pub(super) fn mark_pending_now(&self, user: &str) {
        self.mark_pending(user, Instant::now());
    }

    fn mark_pending(&self, user: &str, since: Instant) {
        lock(&self.pending).entry(user.to_owned()).or_insert(since);
    }

    /// The users whose batch has waited `window`, taken off the list.
    fn due(&self, now: Instant, window: Duration) -> Vec<String> {
        let mut pending = lock(&self.pending);
        let due: Vec<String> = pending
            .iter()
            .filter(|(_, since)| now.saturating_duration_since(**since) >= window)
            .map(|(user, _)| user.clone())
            .collect();
        for user in &due {
            pending.remove(user);
        }
        due
    }
}

/// Starts the worker, the sweep and, with Composio, the realtime channel.
/// Called once; later calls do nothing.
pub fn start(app: &Arc<App>, settings: Settings) {
    let hub = &app.events;
    let Some(receiver) = lock(&hub.receiver).take() else {
        return;
    };
    *lock(&hub.limiter) = RateLimiter::new(settings.limits);
    let mut tasks = vec![
        tokio::spawn(work(app.clone(), receiver, settings.clone())),
        tokio::spawn(sweep(app.clone(), settings.clone())),
    ];
    if settings.poll
        && let Some(composio) = app.composio()
    {
        tasks.push(tokio::spawn(realtime::run(app.clone(), composio.clone())));
    }
    lock(&hub.tasks).extend(tasks);
}

/// Queues `event` for user `user_id`. Waits while the queue is full; a
/// stopped pipeline drops it.
pub async fn submit(app: &App, user_id: &str, event: AppEvent) {
    let incoming = Incoming {
        user: user_id.to_owned(),
        event,
    };
    tokio::select! {
        _ = app.events.stop.cancelled() => {}
        sent = app.events.queue.send(incoming) => {
            if sent.is_err() {
                log::debug!("events: pipeline stopped; event dropped");
            }
        }
    }
}

/// What became of an event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Accepted {
    Stored(i64),
    /// The same happening was stored before.
    Duplicate,
    /// The user turned its topic off.
    Unsubscribed,
    /// Too old to be news.
    Stale,
    /// Past the user's hourly limit.
    Limited,
}

/// Stores `event` for `user` unless it is dropped, and queues the user's
/// next wake.
pub async fn accept(app: &App, user: &User, event: AppEvent) -> Result<Accepted> {
    if !user.event_topics().await?.is_on(event.kind.topic()) {
        return Ok(Accepted::Unsubscribed);
    }
    if event.occurred < Utc::now() - MAX_AGE {
        return Ok(Accepted::Stale);
    }
    let now = Instant::now();
    if !lock(&app.events.limiter).take_event(user.id(), now) {
        log::warn!("events: hourly limit reached, event dropped; user={user}");
        return Ok(Accepted::Limited);
    }
    // Only a stored event counts: a duplicate, or one the store failed on,
    // gives its place back.
    let stored = user.store_event(&event).await;
    if !matches!(stored, Ok(Some(_))) {
        lock(&app.events.limiter).give_back_event(user.id());
    }
    let Some(id) = stored? else {
        return Ok(Accepted::Duplicate);
    };
    log::info!(
        "events: stored; user={user} id={id} app={} kind={} source={}",
        event.app.slug(),
        event.kind.as_str(),
        event.source.as_str()
    );
    app.events.mark_pending(user.id(), now);
    Ok(Accepted::Stored(id))
}

/// Takes queued events in and wakes the agent for batches that are due.
async fn work(app: Arc<App>, mut receiver: mpsc::Receiver<Incoming>, settings: Settings) {
    // Triages cut short by a restart start over. Only this task hands events
    // to triage, so none is running yet.
    match app.store.requeue_triaging().await {
        Ok(0) => {}
        Ok(n) => log::info!("events: {n} events back in the queue after a restart"),
        Err(error) => log::warn!("events: unable to requeue triaging events: {error:#}"),
    }
    let mut ticks = interval(settings.tick);
    ticks.set_missed_tick_behavior(MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            _ = app.events.stop.cancelled() => break,
            incoming = receiver.recv() => {
                let Some(Incoming { user, event }) = incoming else { break };
                let Some(user) = app.user(&user) else {
                    log::debug!("events: no such user; user={user}");
                    continue;
                };
                if let Err(error) = accept(&app, &user, event).await {
                    log::warn!("events: unable to store an event: {error:#}; user={user}");
                }
            }
            _ = ticks.tick() => flush(&app, &settings).await,
        }
    }
}

/// Hands each due batch to a triage run, for users with a run free and a
/// triage left this hour.
async fn flush(app: &Arc<App>, settings: &Settings) {
    let now = Instant::now();
    for id in app.events.due(now, settings.batch_window) {
        let Some(user) = app.user(&id) else { continue };
        // Kept waiting; it goes out with whatever comes next.
        if !app.subagents.has_room(Pool::Triage, &id)
            || !lock(&app.events.limiter).take_triage(&id, now)
        {
            app.events.mark_pending(&id, now);
            continue;
        }
        match start_triage(app, &user, settings).await {
            Ok(Batch::More) => app.events.mark_pending(&id, now),
            Ok(Batch::Done) => {}
            Err(error) => {
                log::warn!("events: unable to start a triage: {error:#}; user={user}");
                app.events.mark_pending(&id, now);
            }
        }
    }
}

/// Moves up to a batch of `user`'s pending events to triaging and starts a
/// triage run on them.
async fn start_triage(app: &Arc<App>, user: &Arc<User>, settings: &Settings) -> Result<Batch> {
    let rows = user
        .events_in(EventState::Pending, Duration::ZERO, settings.max_batch)
        .await?;
    if rows.is_empty() {
        return Ok(Batch::Done);
    }
    let ids: Vec<i64> = rows.iter().map(|row| row.id).collect();
    user.move_events(&ids, EventState::Pending, EventState::Triaging, None)
        .await?;
    let work = triage::run(app.clone(), user.clone(), rows.clone(), settings.clone());
    if app.subagents.spawn(Pool::Triage, user.id(), work).is_err() {
        user.move_events(&ids, EventState::Triaging, EventState::Pending, None)
            .await?;
        return Ok(Batch::More);
    }
    let full = i64::try_from(rows.len()).is_ok_and(|n| n >= settings.max_batch);
    Ok(if full { Batch::More } else { Batch::Done })
}

/// Whether events of the same state are left after a wake.
pub(super) enum Batch {
    Done,
    More,
}

/// Wakes the agent with up to a batch of `user`'s events in `state`, and
/// moves them on.
async fn wake_batch(
    app: &Arc<App>,
    user: &Arc<User>,
    state: EventState,
    rule: Rule,
    settings: &Settings,
) -> Result<Batch> {
    let age = match state {
        EventState::Held => settings.digest_after,
        _ => Duration::ZERO,
    };
    let rows = user.events_in(state, age, settings.max_batch).await?;
    if rows.is_empty() {
        return Ok(Batch::Done);
    }
    wake_rows(app, user, &rows, state, rule, settings).await
}

/// Wakes the agent on `rows`, which are in `state`, under `rule`, and moves
/// them on.
pub(super) async fn wake_rows(
    app: &Arc<App>,
    user: &Arc<User>,
    rows: &[EventRow],
    state: EventState,
    rule: Rule,
    settings: &Settings,
) -> Result<Batch> {
    let read_only = match user.app_rules().await {
        Ok(rules) => apps_of(rows)
            .into_iter()
            .filter(|app| rules.read_only(app.slug()))
            .collect(),
        Err(error) => {
            log::warn!("events: unable to read app permissions: {error:#}; user={user}");
            Vec::new()
        }
    };
    let event = Event {
        label: label(rows, rule),
        description: describe(rows, rule, &read_only),
        data: Value::Null,
    };
    let Some(run) = wake::wake_with(app, user, event, rule).await? else {
        return Ok(Batch::More);
    };
    let ids: Vec<i64> = rows.iter().map(|row| row.id).collect();
    let to = match state {
        EventState::Held => EventState::Digested,
        _ => EventState::Woken,
    };
    user.move_events(&ids, state, to, Some(&run.id)).await?;
    log::info!(
        "events: woke the agent; user={user} run_id={} events={}",
        run.id,
        ids.len()
    );
    let full = i64::try_from(rows.len()).is_ok_and(|n| n >= settings.max_batch);
    Ok(if full { Batch::More } else { Batch::Done })
}

/// The apps `rows` came from, each once, in order.
pub(super) fn apps_of(rows: &[EventRow]) -> Vec<EventApp> {
    let mut apps = Vec::new();
    for row in rows {
        if !apps.contains(&row.app) {
            apps.push(row.app);
        }
    }
    apps
}

/// The chat's title and the notification's: the event itself when there is
/// one, else how many and from where.
fn label(rows: &[EventRow], rule: Rule) -> String {
    match (rows, rule) {
        (_, Rule::Digest) => "Digest".to_owned(),
        ([row], _) => clipped(&row.summary),
        _ => {
            let names: Vec<&str> = apps_of(rows).into_iter().map(EventApp::name).collect();
            format!("{} updates from {}", rows.len(), names.join(", "))
        }
    }
}

/// `text` cut to fit a label.
pub(super) fn clipped(text: &str) -> String {
    let line = text.lines().next().unwrap_or_default();
    let summary = clip(line, LABEL_CHARS);
    if summary.len() < text.len() {
        format!("{}...", summary.trim_end())
    } else {
        summary
    }
}

/// One event with its id, what it is, the line about it and the ids to act
/// on, after a blank line.
pub(super) fn write_event(text: &mut String, row: &EventRow) {
    let _ = write!(
        text,
        "\n\n[#{}] {}, {}, {}: {}",
        row.id,
        row.app.name(),
        row.kind.as_str().replace('_', " "),
        row.occurred.format("%Y-%m-%d %H:%M UTC"),
        row.summary
    );
    if let Some(detail) = &row.detail {
        let _ = write!(text, "\n{detail}");
    }
    if !row.ids.is_null() {
        let _ = write!(text, "\nids: {}", row.ids);
    }
}

/// The message a run starts on: each event as [`write_event`] writes it.
pub(super) fn describe(rows: &[EventRow], rule: Rule, read_only: &[EventApp]) -> String {
    let mut text = match rule {
        Rule::Digest => format!("Events you held earlier ({}):", rows.len()),
        _ => format!(
            "New events from the user's connected accounts ({}):",
            rows.len()
        ),
    };
    for row in rows {
        write_event(&mut text, row);
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

/// Every `sweep_every`: polls, sets up triggers, brings back held events as
/// a digest, finds batches left from before a restart and prunes old events.
async fn sweep(app: Arc<App>, settings: Settings) {
    let mut ticks = interval(settings.sweep_every);
    ticks.set_missed_tick_behavior(MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            _ = app.events.stop.cancelled() => break,
            _ = ticks.tick() => {}
        }
        let users: Vec<Arc<User>> = app.users();
        let sweeping = stream::iter(users).for_each_concurrent(SWEEP_CONCURRENCY, |user| {
            let (app, settings) = (&app, &settings);
            async move {
                let swept =
                    tokio::time::timeout(SWEEP_USER_TIMEOUT, sweep_user(app, &user, settings));
                if swept.await.is_err() {
                    log::warn!("events: sweep timed out; user={user}");
                }
            }
        });
        tokio::select! {
            _ = app.events.stop.cancelled() => break,
            _ = sweeping => {}
        }
    }
}

async fn sweep_user(app: &Arc<App>, user: &Arc<User>, settings: &Settings) {
    if settings.poll {
        if let Err(error) = triggers::sync_user(user).await {
            log::warn!("events: unable to set up triggers: {error:#}; user={user}");
        }
        poll::poll_user(app, user).await;
    }
    // A triage that died, by a panic or a lost task, left its events here.
    match user.requeue_stale_triaging(triage::STALE_TRIAGE).await {
        Ok(0) => {}
        Ok(n) => {
            log::warn!("events: {n} events back in the queue from a stuck triage; user={user}")
        }
        Err(error) => log::warn!("events: unable to requeue stuck triages: {error:#}; user={user}"),
    }
    match user.events_in(EventState::Pending, Duration::ZERO, 1).await {
        Ok(rows) if !rows.is_empty() => app.events.mark_pending(user.id(), Instant::now()),
        Ok(_) => {}
        Err(error) => log::warn!("events: unable to read pending events: {error:#}"),
    }
    match user
        .events_in(EventState::Held, settings.digest_after, 1)
        .await
    {
        Ok(rows) if !rows.is_empty() => {
            if lock(&app.events.limiter).take_wake(user.id(), Instant::now())
                && let Err(error) =
                    wake_batch(app, user, EventState::Held, Rule::Digest, settings).await
            {
                log::warn!("events: unable to send the digest: {error:#}; user={user}");
            }
        }
        Ok(_) => {}
        Err(error) => log::warn!("events: unable to read held events: {error:#}"),
    }
    if let Err(error) = user.prune_events(Utc::now() - KEEP).await {
        log::warn!("events: unable to prune: {error:#}; user={user}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::EventKind;
    use chrono::TimeZone;
    use serde_json::json;

    fn row(id: i64, app: EventApp, summary: &str) -> EventRow {
        EventRow {
            id,
            app,
            kind: EventKind::NewMail,
            summary: summary.to_owned(),
            detail: Some("Can you sign by Friday?".to_owned()),
            ids: json!({"message_id": "m1"}),
            occurred: Utc.with_ymd_and_hms(2026, 10, 20, 9, 30, 0).unwrap(),
            state: EventState::Pending,
        }
    }

    #[test]
    fn a_batch_is_labelled_by_its_event_or_its_apps() {
        let one = [row(1, EventApp::Gmail, "From Ana: Lease renewal")];
        assert_eq!(label(&one, Rule::Accounts), "From Ana: Lease renewal");
        let long = [row(1, EventApp::Gmail, &"x".repeat(80))];
        assert_eq!(
            label(&long, Rule::Accounts),
            format!("{}...", "x".repeat(60))
        );
        let many = [
            row(1, EventApp::Gmail, "a"),
            row(2, EventApp::Github, "b"),
            row(3, EventApp::Gmail, "c"),
        ];
        assert_eq!(label(&many, Rule::Accounts), "3 updates from Gmail, GitHub");
        assert_eq!(label(&many, Rule::Digest), "Digest");
    }

    #[test]
    fn the_message_lists_each_event_with_its_id_and_ids() {
        let rows = [row(7, EventApp::Gmail, "From Ana: Lease renewal")];
        let text = describe(&rows, Rule::Accounts, &[EventApp::Gmail]);
        assert_eq!(
            text,
            "New events from the user's connected accounts (1):\n\n\
             [#7] Gmail, new mail, 2026-10-20 09:30 UTC: From Ana: Lease renewal\n\
             Can you sign by Friday?\n\
             ids: {\"message_id\":\"m1\"}\n\n\
             The user set Gmail to read only: offer nothing that would act there."
        );
    }

    #[test]
    fn only_batches_that_waited_are_due() {
        let hub = Hub::default();
        let start = Instant::now();
        hub.mark_pending("alice", start);
        hub.mark_pending("bob", start + Duration::from_secs(50));
        // A later event does not push a waiting batch back.
        hub.mark_pending("alice", start + Duration::from_secs(80));
        let due = hub.due(start + Duration::from_secs(90), Duration::from_secs(90));
        assert_eq!(due, ["alice"]);
        assert!(
            hub.due(start + Duration::from_secs(91), Duration::from_secs(90))
                .is_empty()
        );
        assert_eq!(
            hub.due(start + Duration::from_secs(140), Duration::from_secs(90)),
            ["bob"]
        );
    }
}
