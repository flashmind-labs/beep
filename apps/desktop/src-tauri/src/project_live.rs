//! Follows the project pages that show (`WatchProject`): the server says
//! whether a project is live through a webhook or polling, and when its
//! pull requests, issues or runs were read again, and the page redraws
//! them as `project-live` events.

use crate::{beep::Beep, projects::unknown_op};
use beep_node::reconnect::Backoff;
use beep_proto::{ProjectChange, WatchProject};
use serde::Serialize;
use std::sync::{Arc, Mutex, PoisonError};
use tauri::{AppHandle, Emitter, State, async_runtime::JoinHandle};
use uuid::Uuid;

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
pub fn follow_project(
    app: AppHandle,
    beep: State<'_, Arc<Beep>>,
    followed: State<'_, Followed>,
    project_id: Uuid,
) {
    let mut tasks = followed
        .tasks
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    if tasks.iter().any(|f| f.project_id == project_id) {
        return;
    }
    if tasks.len() >= MAX_FOLLOWED {
        tasks.remove(0);
    }
    let task = tauri::async_runtime::spawn(follow(app, beep.inner().clone(), project_id));
    tasks.push(Following { project_id, task });
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
