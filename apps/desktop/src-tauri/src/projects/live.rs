//! Follows the project pages that show (`WatchProject`): the server says
//! when a project's pull requests, issues or runs were read again, and the
//! page redraws them as `project-live` events. A project of the mesh has no
//! server to say: the desktop polls the host itself, and tells the page to
//! read again every [`POLL_EVERY`] while it shows. Both poll, so a project
//! is never live (an older server may still say it is).

use crate::{
    beep::{Beep, Service},
    error::Result,
    fleet::projects::{FleetState, mesh_project},
    projects::unknown_op,
};
use beep_node::link::reconnect::Backoff;
use beep_proto::{ProjectChange, WatchProject};
use serde::Serialize;
use std::{
    sync::{Arc, Mutex, PoisonError},
    time::Duration,
};
use tauri::{AppHandle, Emitter, State, async_runtime::JoinHandle};
use tokio::time::interval;
use uuid::Uuid;

/// How often a polled project's page reads the host again.
const POLL_EVERY: Duration = Duration::from_secs(120);
/// Why a mesh project is not live.
const POLLING_WHY: &str = "Beep reads the host every couple of minutes while this page shows.";

/// Project pages followed at once; the oldest goes for a new one.
const MAX_FOLLOWED: usize = 8;

/// What the page hears of one project.
#[derive(Debug, Clone, Serialize)]
struct LiveRow {
    project_id: Uuid,
    /// None from a server too old to say.
    live: Option<bool>,
    polling_why: Option<String>,
    lists: bool,
    runs: bool,
}

impl LiveRow {
    fn of(project_id: Uuid, change: ProjectChange) -> Self {
        Self {
            project_id,
            live: Some(change.live),
            polling_why: change.polling_why,
            lists: change.lists,
            runs: change.runs,
        }
    }
}

/// The followed projects, each with the task that follows it.
#[derive(Default)]
pub struct Followed {
    tasks: Mutex<Vec<Following>>,
}

struct Following {
    project_id: Uuid,
    task: JoinHandle<()>,
}

impl Drop for Following {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// Follows `project_id` while its page shows. Following it again keeps
/// the stream it has.
#[tauri::command]
pub async fn follow_project(
    app: AppHandle,
    fleet: FleetState<'_>,
    beep: Service<'_>,
    followed: State<'_, Followed>,
    project_id: Uuid,
) -> Result<()> {
    let on_mesh = mesh_project(&fleet, project_id).await?.is_some();
    let mut tasks = followed
        .tasks
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    if tasks.iter().any(|f| f.project_id == project_id) {
        return Ok(());
    }
    if tasks.len() >= MAX_FOLLOWED {
        tasks.remove(0);
    }
    let task = if on_mesh {
        tauri::async_runtime::spawn(poll(app, project_id))
    } else {
        tauri::async_runtime::spawn(follow(app, beep.inner().clone(), project_id))
    };
    tasks.push(Following { project_id, task });
    Ok(())
}

/// Stops following `project_id`, or every project without one.
#[tauri::command]
pub fn unfollow_project(followed: State<'_, Followed>, project_id: Option<Uuid>) {
    let mut tasks = followed
        .tasks
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    match project_id {
        Some(id) => tasks.retain(|f| f.project_id != id),
        None => tasks.clear(),
    }
}

/// What a background read of a mesh project tells its page: read the lists or the runs again.
pub(crate) fn changed(app: AppHandle) -> impl Fn(beep_fleet::Changed) + Send + Sync + 'static {
    move |changed| {
        emit(
            &app,
            &LiveRow {
                project_id: beep_fleet::project_id(&changed.project),
                live: Some(false),
                polling_why: Some(POLLING_WHY.to_owned()),
                lists: changed.lists,
                runs: changed.runs,
            },
        );
    }
}

/// Tells the page of a mesh project that it polls, then to read again every [`POLL_EVERY`].
async fn poll(app: AppHandle, project_id: Uuid) {
    let mut row = LiveRow {
        project_id,
        live: Some(false),
        polling_why: Some(POLLING_WHY.to_owned()),
        lists: false,
        runs: false,
    };
    emit(&app, &row);
    row.lists = true;
    row.runs = true;
    let mut ticks = interval(POLL_EVERY);
    // The first tick is at once, and the page has just read.
    ticks.tick().await;
    loop {
        ticks.tick().await;
        emit(&app, &row);
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
    if let Err(e) = app.emit("project-live", row) {
        log::debug!("the project page missed an update: {e}");
    }
}
