//! Render controller-owned state once, then send the same snapshot to every window.

use super::{Fleet, events::Events};
use crate::error::Result;
use beep_proto::node::events::Topic;
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Mutex, PoisonError},
};
use tauri::{AppHandle, Listener, Manager};
use tokio::task::JoinHandle;
use uuid::Uuid;

#[derive(Default)]
pub(crate) struct Views {
    pending: Mutex<Pending>,
    task: Mutex<Option<JoinHandle<()>>>,
}

#[derive(Default)]
struct Pending {
    topics: BTreeSet<&'static str>,
    projects: BTreeMap<Uuid, crate::projects::live::LiveRow>,
    running: bool,
}

impl Drop for Views {
    fn drop(&mut self) {
        if let Some(task) = self.task.get_mut().unwrap_or_else(PoisonError::into_inner) {
            task.abort();
        }
    }
}

pub(crate) fn listen(app: &AppHandle) {
    for event in [
        crate::center::home_sessions::HOME_CHANGED,
        "sessions-changed",
        "agent-title-changed",
        "work-question",
        "work-resolved",
        "group-approval",
        "approval-answered",
        "reconnected",
    ] {
        let handle = app.clone();
        app.listen(event, move |_| changed(handle.clone(), Topic::new("home")));
    }
    let handle = app.clone();
    app.listen("link-state", move |_| {
        changed(handle.clone(), Topic::new("*"))
    });
}

/// Source reads may wait for node projections, so they run outside the stream reader.
pub(crate) fn changed(app: AppHandle, topic: Topic) {
    let views = app.state::<Views>();
    let events = app.state::<Events>();
    let mut pending = views.pending.lock().unwrap_or_else(PoisonError::into_inner);

    for &name in affected(topic.name()) {
        if events.controller_wanted(&Topic::new(name)) {
            pending.topics.insert(name);
        }
    }

    start(&app, &views, pending);
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
        events.publish_views(machine, Topic::new(name), json!({ "views": views }));
    }
}

fn commands(name: &str) -> &'static [&'static str] {
    match name {
        "home" => &["center_home"],
        "machines" => &["list_machines", "mac_control"],
        "projects" => &["projects_on_top", "project_groups"],
        "sync" => &["project_sync"],
        "usage" => &["usage_get"],
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
        row.lists |= previous.lists;
        row.runs |= previous.runs;
    } else if pending.projects.len() >= 128 {
        return;
    }

    pending.projects.insert(row.project_id, row);
    start(&app, &views, pending);
}

fn start(app: &AppHandle, views: &Views, mut pending: std::sync::MutexGuard<'_, Pending>) {
    if pending.running || (pending.topics.is_empty() && pending.projects.is_empty()) {
        return;
    }

    pending.running = true;
    let task_app = app.clone();
    *views.task.lock().unwrap_or_else(PoisonError::into_inner) =
        Some(tokio::spawn(async move { render(task_app).await }));
    drop(pending);
}

fn affected(name: &str) -> &'static [&'static str] {
    match name {
        "*" => &[
            "home", "machines", "projects", "sync", "usage", "accounts", "forge",
        ],
        "agents" | "agent" | "processes" | "moves" | "home" => &["home"],
        "machines" => &["machines", "home"],
        "projects" => &["projects", "home"],
        "sync" => &["sync"],
        "usage" => &["usage"],
        "accounts" => &["accounts"],
        "forge" => &["forge"],
        _ => &[],
    }
}

async fn render(app: AppHandle) {
    loop {
        let (names, projects) = {
            let views = app.state::<Views>();
            let mut pending = views.pending.lock().unwrap_or_else(PoisonError::into_inner);

            if pending.topics.is_empty() && pending.projects.is_empty() {
                pending.running = false;
                return;
            }
            (
                std::mem::take(&mut pending.topics),
                std::mem::take(&mut pending.projects),
            )
        };

        for name in names {
            let topic = Topic::new(name);

            if !app.state::<Events>().controller_wanted(&topic) {
                continue;
            }

            let data = snapshot(&app, name).await;

            if !data.is_empty() {
                let machine = app.state::<Arc<Fleet>>().id();
                app.state::<Events>()
                    .publish_views(machine, topic, json!({ "views": data }));
            }
        }

        for (_, row) in projects {
            crate::projects::live::emit_snapshot(&app, row).await;
        }
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
        "home" => append(
            &mut views,
            "center_home",
            crate::center::home::center_home(
                app.clone(),
                app.state(),
                app.state(),
                app.state(),
                app.state(),
                app.state(),
                app.state(),
            )
            .await,
        ),
        "machines" => {
            append(
                &mut views,
                "list_machines",
                crate::machines::list::list_machines(app.clone(), app.state(), app.state()).await,
            );
            views.extend(View::new(
                "mac_control",
                json!({}),
                crate::bar::app::mac_control(app.state(), app.state()),
            ));
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
                crate::projects::groups::project_groups(app.state(), app.state(), app.state())
                    .await,
            );
        }
        "sync" => append(
            &mut views,
            "project_sync",
            crate::projects::file_sync::project_sync(app.state(), app.state(), app.state()).await,
        ),
        "usage" => append(
            &mut views,
            "usage_get",
            crate::center::usage::usage_get(app.state(), false).await,
        ),
        "forge" => append(
            &mut views,
            "git_items",
            crate::projects::git_items::git_items(app.clone(), app.state(), app.state()).await,
        ),
        "accounts" => append(
            &mut views,
            "agent_accounts",
            crate::agents::accounts::agent_accounts(app.state()).await,
        ),
        _ => {}
    }

    views
}

#[cfg(test)]
#[path = "../tests/fleet_event_views.rs"]
mod tests;
