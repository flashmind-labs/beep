//! Project views share one source per project while any window follows it.
//! Server subscriptions and periodic forge reads publish project changes locally.

use crate::{
    beep::{Beep, Service},
    error::Result,
    fleet::projects::{FleetState, mesh_project},
    projects::unknown_op,
};
use beep_fleet::Changed;
use beep_node::link::reconnect::Backoff;
use beep_proto::{ProjectChange, WatchProject};
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    sync::{Arc, Mutex, PoisonError},
    time::Duration,
};
use tauri::{AppHandle, Emitter, Manager, State, WebviewWindow, async_runtime::JoinHandle};
use tokio::time::interval;
use uuid::Uuid;

/// How often a polled project's page reads the host again.
const POLL_EVERY: Duration = Duration::from_secs(120);
/// Tells the usage pill a plan or the spending changed.
const USAGE_CHANGED: &str = "usage-changed";
/// Why a mesh project is not live.
const POLLING_WHY: &str = "Beep reads the host every couple of minutes while this page shows.";

/// Project pages followed by one window; its oldest ownership goes for a new one.
const MAX_FOLLOWED: usize = 8;

/// What the page hears of one project.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct LiveRow {
    pub(crate) project_id: Uuid,
    /// None from a server too old to say.
    live: Option<bool>,
    polling_why: Option<String>,
    pub(crate) lists: bool,
    pub(crate) runs: bool,
    views: Vec<crate::fleet::event_views::View>,
}

impl LiveRow {
    fn of(project_id: Uuid, change: ProjectChange) -> Self {
        Self {
            project_id,
            live: Some(change.live),
            polling_why: change.polling_why,
            lists: change.lists,
            runs: change.runs,
            views: Vec::new(),
        }
    }
}

/// The followed projects, each with the task that follows it.
#[derive(Default)]
pub struct Followed {
    tasks: Mutex<Vec<Following>>,
    pending: Mutex<BTreeMap<(String, Uuid), SelectedRun>>,
}

struct Following {
    project_id: Uuid,
    windows: HashSet<String>,
    selected: BTreeMap<String, SelectedRun>,
    task: JoinHandle<()>,
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
struct SelectedRun {
    id: u64,
    host: Option<String>,
}

impl Drop for Following {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl Followed {
    fn select(&self, window: &str, project_id: Uuid, run: Option<SelectedRun>) {
        let mut tasks = self.tasks.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(following) = tasks.iter_mut().find(|following| {
            following.project_id == project_id && following.windows.contains(window)
        }) {
            match run {
                Some(run) => {
                    following.selected.insert(window.to_owned(), run);
                }
                None => {
                    following.selected.remove(window);
                }
            }
        } else {
            let mut pending = self.pending.lock().unwrap_or_else(PoisonError::into_inner);
            let key = (window.to_owned(), project_id);
            if let Some(run) = run {
                if pending.contains_key(&key) || pending.len() < 256 {
                    pending.insert(key, run);
                }
            } else {
                pending.remove(&key);
            }
        }
    }

    pub(crate) fn contains(&self, project_id: Uuid) -> bool {
        self.tasks
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .any(|following| following.project_id == project_id)
    }

    fn selected(&self, project_id: Uuid) -> Vec<SelectedRun> {
        self.tasks
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .filter(|following| following.project_id == project_id)
            .flat_map(|following| following.selected.values().cloned())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .take(16)
            .collect()
    }

    pub(crate) fn clear(&self, window: &str) {
        self.remove(window, None);
    }

    fn remove(&self, window: &str, project_id: Option<Uuid>) {
        let mut tasks = self.tasks.lock().unwrap_or_else(PoisonError::into_inner);

        for following in tasks.iter_mut() {
            if project_id.is_none_or(|id| following.project_id == id) {
                following.windows.remove(window);
                following.selected.remove(window);
            }
        }

        tasks.retain(|following| !following.windows.is_empty());
        self.pending
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .retain(|(owner, project), _| {
                owner != window || project_id.is_some_and(|id| id != *project)
            });
    }

    fn add(
        &self,
        window: &str,
        project_id: Uuid,
        alive: impl Fn() -> bool,
        spawn: impl FnOnce() -> JoinHandle<()>,
    ) {
        if !alive() {
            self.clear(window);
            return;
        }
        {
            let mut tasks = self.tasks.lock().unwrap_or_else(PoisonError::into_inner);
            let already_owned = tasks.iter().any(|following| {
                following.project_id == project_id && following.windows.contains(window)
            });

            if !already_owned {
                if tasks
                    .iter()
                    .filter(|following| following.windows.contains(window))
                    .count()
                    >= MAX_FOLLOWED
                {
                    if let Some(oldest) = tasks
                        .iter_mut()
                        .find(|following| following.windows.contains(window))
                    {
                        oldest.windows.remove(window);
                        oldest.selected.remove(window);
                    }

                    tasks.retain(|following| !following.windows.is_empty());
                }

                if let Some(following) = tasks
                    .iter_mut()
                    .find(|following| following.project_id == project_id)
                {
                    following.windows.insert(window.to_owned());
                } else {
                    tasks.push(Following {
                        project_id,
                        windows: HashSet::from([window.to_owned()]),
                        selected: BTreeMap::new(),
                        task: spawn(),
                    });
                }
            }

            if let Some(run) = self
                .pending
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .remove(&(window.to_owned(), project_id))
                && let Some(following) = tasks
                    .iter_mut()
                    .find(|following| following.project_id == project_id)
            {
                following.selected.insert(window.to_owned(), run);
            }
        }
        // Destruction may have cleared this window while its project lookup was pending.
        if !alive() {
            self.clear(window);
        }
    }
}

/// Follows `project_id` while this window's page shows, sharing its existing source.
#[tauri::command]
pub async fn follow_project(
    app: AppHandle,
    window: WebviewWindow,
    fleet: FleetState<'_>,
    beep: Service<'_>,
    followed: State<'_, Followed>,
    project_id: Uuid,
) -> Result<()> {
    let on_mesh = mesh_project(&fleet, project_id).await?;
    let alive_app = app.clone();
    followed.add(
        window.label(),
        project_id,
        || alive_app.get_webview_window(window.label()).is_some(),
        || {
            if let Some((_, project)) = on_mesh {
                tauri::async_runtime::spawn(poll(app, fleet.inner().clone(), project_id, project))
            } else {
                tauri::async_runtime::spawn(follow(app, beep.inner().clone(), project_id))
            }
        },
    );
    Ok(())
}

/// Removes this window's ownership of one project, or all its projects without one.
#[tauri::command]
pub fn unfollow_project(
    window: WebviewWindow,
    followed: State<'_, Followed>,
    project_id: Option<Uuid>,
) {
    followed.remove(window.label(), project_id);
}

/// One visible workflow detail per window shares its project's source reads.
#[tauri::command]
pub fn follow_project_run(
    window: WebviewWindow,
    followed: State<'_, Followed>,
    project_id: Uuid,
    run_id: Option<u64>,
    host: Option<String>,
) -> Result<()> {
    if host.as_ref().is_some_and(|host| host.len() > 512) {
        return Err(crate::error::DesktopError::BadRequest(
            "invalid run host".into(),
        ));
    }

    followed.select(
        window.label(),
        project_id,
        run_id.map(|id| SelectedRun { id, host }),
    );
    Ok(())
}

/// What a background read of a mesh project tells its page: read the lists or the runs again.
/// Usage that changed tells the usage pill to read it again.
pub(crate) fn changed(app: AppHandle) -> impl Fn(Changed) + Send + Sync + 'static {
    move |changed| match changed {
        Changed::Project {
            project,
            lists,
            runs,
        } => emit(
            &app,
            &LiveRow {
                project_id: beep_fleet::project_id(&project),
                live: Some(false),
                polling_why: Some(POLLING_WHY.to_owned()),
                lists,
                runs,
                views: Vec::new(),
            },
        ),
        Changed::Usage => {
            beep_node::events::system().changed(beep_proto::node::events::Topic::new("usage"));
            crate::fleet::event_views::changed(
                app.clone(),
                beep_proto::node::events::Topic::new("usage"),
            );

            if let Err(e) = app.emit(USAGE_CHANGED, ()) {
                log::debug!("the usage pill missed an update: {e}");
            }
        }
    }
}

/// Tells the page of a mesh project that it polls, then to read again every [`POLL_EVERY`].
async fn poll(app: AppHandle, fleet: Arc<beep_fleet::Session>, project_id: Uuid, project: String) {
    let mut before_lists = None;
    let mut before_runs = None;
    let mut ticks = interval(POLL_EVERY);

    loop {
        ticks.tick().await;
        let (lists, runs) = tokio::join!(fleet.lists(&project, true), fleet.runs(&project, true));
        let lists_changed = match lists {
            Ok(mut lists) => {
                lists.lists.synced_at = None;
                let changed = before_lists.as_ref() != Some(&lists);
                before_lists = Some(lists);
                changed
            }
            Err(error) => {
                log::debug!("project lists source: {error}");
                false
            }
        };
        let runs_changed = match runs {
            Ok(runs) => {
                let changed = before_runs.as_ref() != Some(&runs);
                before_runs = Some(runs);
                changed
            }
            Err(error) => {
                log::debug!("project runs source: {error}");
                false
            }
        };

        if lists_changed || runs_changed || !app.state::<Followed>().selected(project_id).is_empty()
        {
            emit(
                &app,
                &LiveRow {
                    project_id,
                    live: Some(false),
                    polling_why: Some(POLLING_WHY.into()),
                    lists: lists_changed,
                    runs: runs_changed || !app.state::<Followed>().selected(project_id).is_empty(),
                    views: Vec::new(),
                },
            );
            beep_node::events::system()
                .changed(beep_proto::node::events::Topic::keyed("forge", &project));
        }
    }
}

async fn follow(app: AppHandle, beep: Arc<Beep>, project_id: Uuid) {
    let mut backoff = Backoff::eager();
    let mut again = false;

    loop {
        let mut replies = match beep.stream(WatchProject { project_id }).await {
            Ok(replies) => replies,
            Err(e) if unknown_op(&e) => {
                emit(
                    &app,
                    &LiveRow {
                        project_id,
                        live: None,
                        polling_why: None,
                        lists: false,
                        runs: false,
                        views: Vec::new(),
                    },
                );
                return;
            }
            Err(e) => {
                log::info!("following a project page: {e}");
                tokio::time::sleep(backoff.pause()).await;
                continue;
            }
        };

        loop {
            match replies.next().await {
                Ok(Some(change)) => {
                    backoff.reset();
                    let mut row = LiveRow::of(project_id, change);
                    // Back after a break: what changed meanwhile was missed.
                    if again {
                        row.lists = true;
                        row.runs = true;
                        again = false;
                    }

                    emit(&app, &row);
                }
                Ok(None) => break,
                Err(e) => {
                    log::info!("a project page's updates stopped: {e}");
                    break;
                }
            }
        }
        again = true;
        tokio::time::sleep(backoff.pause()).await;
    }
}

fn emit(app: &AppHandle, row: &LiveRow) {
    crate::fleet::event_views::project_changed(app.clone(), row.clone());
}

pub(crate) async fn emit_snapshot(app: &AppHandle, mut row: LiveRow) {
    if !app.state::<Followed>().contains(row.project_id) {
        return;
    }

    let args = serde_json::json!({ "projectId": row.project_id });

    if row.lists {
        match crate::projects::runs::center_project_lists(
            app.state(),
            app.state(),
            row.project_id,
            false,
        )
        .await
        {
            Ok(value) => row.views.extend(crate::fleet::event_views::View::new(
                "center_project_lists",
                args.clone(),
                value,
            )),
            Err(error) => row.views.push(crate::fleet::event_views::View::failed(
                "center_project_lists",
                args.clone(),
                &error.to_string(),
            )),
        }
    }

    if row.runs {
        match crate::projects::runs::center_project_runs(
            app.state(),
            app.state(),
            row.project_id,
            false,
        )
        .await
        {
            Ok(value) => row.views.extend(crate::fleet::event_views::View::new(
                "center_project_runs",
                args,
                value,
            )),
            Err(error) => row.views.push(crate::fleet::event_views::View::failed(
                "center_project_runs",
                args,
                &error.to_string(),
            )),
        }
        for run in app.state::<Followed>().selected(row.project_id) {
            let args = serde_json::json!({ "projectId": row.project_id, "runId": run.id, "host": run.host });
            match crate::projects::runs::center_project_run(
                app.state(),
                app.state(),
                row.project_id,
                run.id,
                run.host,
                Some(true),
            )
            .await
            {
                Ok(value) => row.views.extend(crate::fleet::event_views::View::new(
                    "center_project_run",
                    args,
                    value,
                )),
                Err(error) => row.views.push(crate::fleet::event_views::View::failed(
                    "center_project_run",
                    args,
                    &error.to_string(),
                )),
            }
        }
    }

    if !app.state::<Followed>().contains(row.project_id) {
        return;
    }

    let machine = app.state::<Arc<crate::fleet::Fleet>>().id();
    app.state::<crate::fleet::events::Events>().publish_views(
        machine,
        beep_proto::node::events::Topic::keyed("forge", row.project_id.to_string()),
        serde_json::json!({ "views": row.views }),
    );

    if let Err(e) = app.emit("project-live", row) {
        log::debug!("the project page missed an update: {e}");
    }
}

#[cfg(test)]
#[path = "../tests/projects_live.rs"]
mod tests;
