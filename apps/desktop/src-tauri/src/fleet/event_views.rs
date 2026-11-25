//! Render controller-owned state once, then send the same snapshot to every window.

use super::{Fleet, events::Events, session_mirror::Rows};
use crate::error::Result;
use beep_proto::node::events::Topic;
use futures::{StreamExt, stream::FuturesUnordered};
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    future::Future,
    sync::{Arc, Mutex, PoisonError},
    time::{Duration, Instant},
};
use tauri::{AppHandle, Listener, Manager};
use tokio::{sync::watch, task::JoinSet};
use uuid::Uuid;

/// The least time between two sync views while files move, whose progress
/// changes twice a second on each machine.
const SYNC_GAP: Duration = Duration::from_secs(1);
/// The least time between two Home views for session changes, which come many
/// times a second while agents work; each draw lists every machine's sessions.
const HOME_GAP: Duration = Duration::from_secs(1);

#[derive(Default)]
pub(crate) struct Views {
    pending: Mutex<Pending>,
    /// The draws under way, which end with the app.
    tasks: Mutex<JoinSet<()>>,
    sync: Mutex<DrawPace>,
    home: Mutex<DrawPace>,
    /// Views that changed while only hidden windows wanted them, drawn once one shows.
    deferred: Mutex<BTreeSet<&'static str>>,
}

/// When a paced view was drawn last, and whether a draw waits for its gap.
#[derive(Default)]
struct DrawPace {
    drawn: Option<Instant>,
    waiting: bool,
}

#[derive(Debug, PartialEq, Eq)]
enum Pace {
    Now,
    At(Instant),
    /// A draw waits already and will show this change too.
    Waiting,
}

impl DrawPace {
    /// When to draw for a change; a frequent change waits for `gap` after the last
    /// draw, and any other change, such as the end of a transfer, draws now.
    fn next(&mut self, now: Instant, gap: Duration, frequent: bool) -> Pace {
        if !frequent {
            return Pace::Now;
        }

        if self.waiting {
            return Pace::Waiting;
        }

        match self.drawn.map(|drawn| drawn + gap) {
            Some(due) if due > now => {
                self.waiting = true;
                Pace::At(due)
            }
            _ => Pace::Now,
        }
    }
}

/// What waits to be drawn. Each topic draws on its own, so a slow one, such as the accounts
/// that probe every machine, never holds Home back; a topic draws once at a time.
#[derive(Default)]
struct Pending {
    topics: BTreeSet<&'static str>,
    /// The topics drawing now. A change to one meanwhile waits in `topics` and draws after.
    drawing: BTreeSet<&'static str>,
    projects: BTreeMap<Uuid, crate::projects::live::LiveRow>,
    drawing_projects: bool,
}

/// What to start drawing now.
#[derive(Debug, Default, PartialEq, Eq)]
struct Due {
    topics: Vec<&'static str>,
    projects: bool,
}

impl Pending {
    /// Moves the topics no draw holds into `drawing`, and the projects when none draw.
    fn due(&mut self) -> Due {
        let topics: Vec<&'static str> = self
            .topics
            .iter()
            .copied()
            .filter(|name| !self.drawing.contains(name))
            .collect();

        for name in &topics {
            self.topics.remove(name);
            self.drawing.insert(name);
        }

        let projects = !self.drawing_projects && !self.projects.is_empty();
        self.drawing_projects |= projects;
        Due { topics, projects }
    }

    /// Ends a draw of `name`: true when it changed meanwhile and draws again.
    fn again(&mut self, name: &'static str) -> bool {
        if self.topics.remove(name) {
            return true;
        }

        self.drawing.remove(name);
        false
    }
}

pub(crate) fn listen(app: &AppHandle) {
    for event in ["agent-title-changed", "reconnected"] {
        let handle = app.clone();
        app.listen(event, move |_| changed(handle.clone(), Topic::new("home")));
    }
}

/// Draws Home again whenever the mirror's rows change. A busy agent changes them many
/// times a second, so the draws keep `HOME_GAP`.
pub(crate) async fn follow_rows(app: AppHandle, mut rows: watch::Receiver<Arc<Rows>>) {
    while rows.changed().await.is_ok() {
        paced(app.clone(), "home", true);
    }
}

/// Draws Home again at each poke of the signal others send for what Home shows besides the
/// mirror's rows.
pub(crate) async fn follow_signal(app: AppHandle, mut pokes: watch::Receiver<u64>) {
    while pokes.changed().await.is_ok() {
        changed(app.clone(), Topic::new("home"));
    }
}

/// Source reads may wait for node projections, so they run outside the stream reader.
pub(crate) fn changed(app: AppHandle, topic: Topic) {
    let views = app.state::<Views>();
    let events = app.state::<Events>();
    let affected = affected(topic.name());

    if affected.contains(&"home") {
        app.state::<crate::center::home::HomeReads>().forget();
    }

    let mut pending = views.pending.lock().unwrap_or_else(PoisonError::into_inner);

    for &name in affected {
        let topic = Topic::new(name);

        if events.controller_wanted(&topic) {
            pending.topics.insert(name);
        } else if events.controller_held(&topic) {
            defer(&views, name);
        }
    }

    start(&app, &views, pending);
}

/// A window showed again: draw the views that changed while it was hidden.
pub(crate) fn resumed(app: AppHandle) {
    let views = app.state::<Views>();
    let events = app.state::<Events>();
    let deferred = std::mem::take(
        &mut *views
            .deferred
            .lock()
            .unwrap_or_else(PoisonError::into_inner),
    );
    let mut pending = views.pending.lock().unwrap_or_else(PoisonError::into_inner);

    for name in deferred {
        if events.controller_wanted(&Topic::new(name)) {
            pending.topics.insert(name);
        } else {
            defer(&views, name);
        }
    }

    start(&app, &views, pending);
}

fn defer(views: &Views, name: &'static str) {
    views
        .deferred
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(name);
}

/// A sync change; `moving` when it only reports files in transit, which waits for `SYNC_GAP`.
pub(crate) fn sync_changed(app: AppHandle, moving: bool) {
    paced(app, "sync", moving);
}

/// A machine's sessions, one session or the moves changed. Home lists every machine's
/// sessions for it, so it draws at most once a `HOME_GAP`.
pub(crate) fn sessions_changed(app: AppHandle, topic: Topic) {
    if matches!(topic.name(), "agents" | "agent" | "moves") {
        if topic.name() == "moves" {
            changed(app.clone(), Topic::new("moves"));
        }

        paced(app, "home", true);
    } else {
        changed(app, topic);
    }
}

/// The user changed a session on this desktop: Home draws at once, not after the gap that
/// keeps what machines push from drawing it many times a second.
pub(crate) fn session_edited(app: AppHandle) {
    paced(app, "home", false);
}

/// Draws `name` now, or once its gap after the last draw when the change is `frequent`.
fn paced(app: AppHandle, name: &'static str, frequent: bool) {
    let views = app.state::<Views>();
    let Some((pace, gap)) = views.pace(name) else {
        return changed(app.clone(), Topic::new(name));
    };
    let next =
        pace.lock()
            .unwrap_or_else(PoisonError::into_inner)
            .next(Instant::now(), gap, frequent);

    match next {
        Pace::Now => changed(app, Topic::new(name)),
        Pace::At(due) => {
            let app = app.clone();
            views.spawn(async move {
                tokio::time::sleep_until(due.into()).await;

                if let Some((pace, _)) = app.state::<Views>().pace(name) {
                    pace.lock().unwrap_or_else(PoisonError::into_inner).waiting = false;
                }

                changed(app, Topic::new(name));
            });
        }
        Pace::Waiting => {}
    }
}

impl Views {
    /// Event callbacks can run on the main thread, outside a Tokio runtime.
    fn spawn(&self, task: impl Future<Output = ()> + Send + 'static) {
        let mut tasks = self.tasks.lock().unwrap_or_else(PoisonError::into_inner);
        while tasks.try_join_next().is_some() {}

        tasks.spawn_on(task, tauri::async_runtime::handle().inner());
    }

    /// The pace of a view that frequent changes draw, and its gap.
    fn pace(&self, name: &str) -> Option<(&Mutex<DrawPace>, Duration)> {
        match name {
            "sync" => Some((&self.sync, SYNC_GAP)),
            "home" => Some((&self.home, HOME_GAP)),
            _ => None,
        }
    }
}

/// Whether a sync snapshot shows files in transit on that machine.
pub(crate) fn moving(value: &beep_proto::node::events::Value) -> bool {
    use beep_proto::node::{events::Value, sync::Reply};

    matches!(value, Value::Sync(reply) if matches!(reply.as_ref(),
        Reply::Status { projects } if projects.iter().any(|project| project.progress.is_some())))
}

/// Errors wake consumers without starting a read loop inside the source worker.
pub(crate) fn failed(app: &AppHandle, topic: &Topic, reason: &str) {
    let machine = app.state::<Arc<Fleet>>().id();
    let events = app.state::<Events>();

    for &name in affected(topic.name()) {
        let views: Vec<_> = commands(name)
            .iter()
            .map(|&command| View::failed(command, json!({}), reason))
            .collect();

        if !views.is_empty() {
            events.publish_views(machine, views_topic(name), json!({ "views": views }));
        }
    }
}

fn commands(name: &str) -> &'static [&'static str] {
    match name {
        "home" => &["center_home", "agent_sessions", "session_moves"],
        "machines" => &[
            "list_machines",
            "mac_control",
            "updatable_machines",
            "agent_hosts",
            "device_hosts",
            "machine_clocks",
        ],
        "tools" => &["tool_updates"],
        "projects" => &["projects_on_top", "project_groups"],
        "sync" => &["project_sync"],
        "usage" => &["usage_get"],
        "terminals" => &["terminals_all"],
        "accounts" => &["agent_accounts"],
        _ => &[],
    }
}

pub(crate) fn project_changed(app: AppHandle, mut row: crate::projects::live::LiveRow) {
    if !app
        .state::<crate::projects::live::Followed>()
        .contains(row.project_id)
    {
        return;
    }

    let views = app.state::<Views>();
    let mut pending = views.pending.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(previous) = pending.projects.get(&row.project_id) {
        // What the newer row carries wins; one that carries nothing keeps the earlier read.
        row.lists_read = row
            .lists_read
            .take()
            .or_else(|| previous.lists_read.clone());
        row.runs_read = row.runs_read.take().or_else(|| previous.runs_read.clone());
        row.lists |= previous.lists;
        row.runs |= previous.runs;
    } else if pending.projects.len() >= 128 {
        return;
    }

    pending.projects.insert(row.project_id, row);
    start(&app, &views, pending);
}

fn start(app: &AppHandle, views: &Views, mut pending: std::sync::MutexGuard<'_, Pending>) {
    let due = pending.due();
    drop(pending);

    if due == Due::default() {
        return;
    }

    for name in due.topics {
        views.spawn(draw(app.clone(), name));
    }

    if due.projects {
        views.spawn(draw_projects(app.clone()));
    }
}

fn affected(name: &str) -> &'static [&'static str] {
    match name {
        "*" => &[
            "home",
            "machines",
            "tools",
            "projects",
            "sync",
            "usage",
            "accounts",
            "forge",
            "terminals",
            "moves",
        ],
        // Home shows no processes.
        "agents" | "agent" | "home" => &["home"],
        "machines" => &["machines", "home", "tools"],
        "tools" => &["tools"],
        "projects" => &["projects", "home"],
        "sync" => &["sync"],
        "usage" => &["usage"],
        // A harness switched on or off changes the hosts the machines view offers.
        "accounts" => &["accounts", "machines"],
        "forge" => &["forge"],
        // Any machine's terminals changed; the list of every terminal is read again.
        "terminals" => &["terminals"],
        // The progress of each move, as its target says.
        "moves" => &["moves"],
        _ => &[],
    }
}

/// Where the views of `name` are published. A node's own topic of that name has its own
/// updates, so the views ride a keyed topic under it: a window that follows the topic hears both.
fn views_topic(name: &str) -> Topic {
    match name {
        "terminals" | "moves" => Topic::keyed(name, "views"),
        _ => Topic::new(name),
    }
}

/// Draws `name` and publishes it, again while it changes as it draws.
async fn draw(app: AppHandle, name: &'static str) {
    loop {
        let topic = Topic::new(name);

        if app.state::<Events>().controller_wanted(&topic) {
            if let Some((pace, _)) = app.state::<Views>().pace(name) {
                pace.lock().unwrap_or_else(PoisonError::into_inner).drawn = Some(Instant::now());
            }

            let data = snapshot(&app, name).await;

            if !data.is_empty() {
                let machine = app.state::<Arc<Fleet>>().id();
                app.state::<Events>().publish_views(
                    machine,
                    views_topic(name),
                    json!({ "views": data }),
                );
            }
        } else if app.state::<Events>().controller_held(&topic) {
            defer(&app.state::<Views>(), name);
        }

        let views = app.state::<Views>();
        let mut pending = views.pending.lock().unwrap_or_else(PoisonError::into_inner);

        if !pending.again(name) {
            return;
        }
    }
}

/// Publishes the followed projects that changed, together.
async fn draw_projects(app: AppHandle) {
    loop {
        let projects = {
            let views = app.state::<Views>();
            let mut pending = views.pending.lock().unwrap_or_else(PoisonError::into_inner);

            if pending.projects.is_empty() {
                pending.drawing_projects = false;
                return;
            }

            std::mem::take(&mut pending.projects)
        };

        let draws = projects
            .into_values()
            .map(|row| crate::projects::live::emit_snapshot(&app, row))
            .collect::<FuturesUnordered<_>>();

        draws.collect::<()>().await;
    }
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct View {
    command: &'static str,
    args: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    value: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

impl View {
    pub(crate) fn new<T: Serialize>(command: &'static str, args: Value, value: T) -> Option<Self> {
        match serde_json::to_value(value) {
            Ok(value) => Some(Self {
                command,
                args,
                value: Some(value),
                error: None,
            }),
            Err(error) => {
                log::debug!("view snapshot {command} could not be encoded: {error}");
                Some(Self::failed(command, args, &error.to_string()))
            }
        }
    }

    pub(crate) fn failed(command: &'static str, args: Value, reason: &str) -> Self {
        Self {
            command,
            args,
            value: None,
            error: Some(reason.to_owned()),
        }
    }
}

fn append<T: Serialize>(views: &mut Vec<View>, command: &'static str, result: Result<T>) {
    match result {
        Ok(value) => views.extend(View::new(command, json!({}), value)),
        Err(error) => views.push(View::failed(command, json!({}), &error.to_string())),
    }
}

async fn snapshot(app: &AppHandle, name: &'static str) -> Vec<View> {
    let mut views = Vec::new();
    metrics::counter!("beep_view_render_total", "name" => name).increment(1);

    match name {
        "home" => {
            let reads = app.state::<crate::center::home::HomeReads>();
            let (home, sessions, moves) =
                tokio::join!(reads.read(app), reads.sessions(app), reads.moves(app));
            append(&mut views, "center_home", home);
            append(&mut views, "agent_sessions", sessions);
            append(&mut views, "session_moves", moves);
        }
        "machines" => {
            append(
                &mut views,
                "list_machines",
                crate::machines::list::list_machines(app.state()).await,
            );
            views.extend(View::new(
                "mac_control",
                json!({}),
                crate::bar::app::mac_control(app.state(), app.state(), app.state()),
            ));
            views.extend(View::new(
                "updatable_machines",
                json!({}),
                app.state::<Arc<Fleet>>().status().updatable(),
            ));
            append(
                &mut views,
                "agent_hosts",
                crate::fleet::agents::agent_hosts(app.state()).await,
            );
            views.extend(View::new(
                "device_hosts",
                json!({}),
                crate::machines::list::device_hosts(app.state()),
            ));
            // The clocks follow the same status change as the list.
            views.extend(View::new(
                "machine_clocks",
                json!({}),
                app.state::<Arc<Fleet>>().status().clocks_off(),
            ));
            views.extend(crate::projects::page::page_views(app, true).await);
            views.extend(crate::fleet::agents::harness_views(&app.state::<Arc<Fleet>>()).await);
        }
        "tools" => {
            let found = crate::fleet::tool_updates::all(&app.state::<Arc<Fleet>>()).await;
            views.extend(View::new("tool_updates", json!({}), found));
        }
        "projects" => {
            append(
                &mut views,
                "projects_on_top",
                crate::projects::list::projects_on_top(app.state(), app.state()).await,
            );
            append(
                &mut views,
                "project_groups",
                crate::projects::groups::project_groups(app.state(), app.state()).await,
            );
            views.extend(crate::projects::page::page_views(app, true).await);
        }
        "sync" => {
            append(
                &mut views,
                "project_sync",
                crate::projects::file_sync::project_sync(app.state(), app.state()).await,
            );
            views.extend(crate::projects::page::page_views(app, false).await);
        }
        "usage" => append(
            &mut views,
            "usage_get",
            crate::center::usage::usage_get(app.state(), false).await,
        ),
        "moves" => {
            if let Ok(moves) = crate::agents::sessions::moves(app.state()).await {
                views.extend(crate::agents::moving::progress_views(&app.state(), &moves).await);
            }
        }
        "terminals" => append(
            &mut views,
            "terminals_all",
            crate::machines::terminals::all_now(
                app,
                &app.state::<Arc<Fleet>>(),
                &app.state::<crate::machines::list::MachineLists>(),
            )
            .await,
        ),
        "forge" => append(
            &mut views,
            "git_items",
            crate::projects::git_items::git_items(app.state(), app.state()).await,
        ),
        "accounts" => {
            append(
                &mut views,
                "agent_accounts",
                crate::agents::accounts::agent_accounts(app.state()).await,
            );
            views.extend(crate::fleet::agents::harness_views(&app.state::<Arc<Fleet>>()).await);
        }
        _ => {}
    }

    views
}

#[cfg(test)]
#[path = "../tests/fleet_event_views.rs"]
mod tests;
