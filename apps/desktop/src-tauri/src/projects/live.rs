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
    collections::HashSet,
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
    windows: HashSet<String>,
    task: JoinHandle<()>,
}

impl Drop for Following {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl Followed {
    pub(crate) fn clear(&self, window: &str) {
        self.remove(window, None);
    }

    fn remove(&self, window: &str, project_id: Option<Uuid>) {
        let mut tasks = self.tasks.lock().unwrap_or_else(PoisonError::into_inner);
        for following in tasks.iter_mut() {
            if project_id.is_none_or(|id| following.project_id == id) {
                following.windows.remove(window);
            }
        }
        tasks.retain(|following| !following.windows.is_empty());
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
                        task: spawn(),
                    });
                }
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
            },
        ),
        Changed::Usage => {
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
        if lists_changed || runs_changed {
            emit(
                &app,
                &LiveRow {
                    project_id,
                    live: Some(false),
                    polling_why: Some(POLLING_WHY.into()),
                    lists: lists_changed,
                    runs: runs_changed,
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::sync::oneshot;

    fn source(stopped: oneshot::Sender<()>) -> JoinHandle<()> {
        tauri::async_runtime::spawn(async move {
            let _stopped = stopped;
            std::future::pending::<()>().await;
        })
    }

    #[tokio::test]
    async fn windows_share_a_source_until_its_last_owner_leaves() {
        let followed = Followed::default();
        let project = Uuid::new_v4();
        let (stop, stopped) = oneshot::channel();
        followed.add("one", project, || true, || source(stop));
        followed.add("two", project, || true, || panic!("duplicate source"));
        followed.remove("one", Some(project));
        {
            let tasks = followed.tasks.lock().unwrap();
            assert_eq!(tasks.len(), 1);
            assert_eq!(tasks[0].windows, HashSet::from(["two".to_owned()]));
        }
        followed.clear("two");
        assert!(followed.tasks.lock().unwrap().is_empty());
        assert!(
            tokio::time::timeout(Duration::from_secs(1), stopped)
                .await
                .unwrap()
                .is_err()
        );
    }

    #[tokio::test]
    async fn clearing_one_window_keeps_other_windows_project_sources() {
        let followed = Followed::default();
        let shared = Uuid::new_v4();
        let other = Uuid::new_v4();
        let (shared_stop, _shared_stopped) = oneshot::channel();
        let (other_stop, other_stopped) = oneshot::channel();
        followed.add("one", shared, || true, || source(shared_stop));
        followed.add("two", shared, || true, || panic!("duplicate source"));
        followed.add("one", other, || true, || source(other_stop));
        followed.clear("one");
        {
            let tasks = followed.tasks.lock().unwrap();
            assert_eq!(tasks.len(), 1);
            assert_eq!(tasks[0].project_id, shared);
            assert!(tasks[0].windows.contains("two"));
        }
        assert!(
            tokio::time::timeout(Duration::from_secs(1), other_stopped)
                .await
                .unwrap()
                .is_err()
        );
    }

    #[tokio::test]
    async fn a_destroyed_window_cannot_leave_a_late_source_registration() {
        let followed = Followed::default();
        followed.add("gone", Uuid::new_v4(), || false, || panic!("closed window"));
        let checks = AtomicUsize::new(0);
        let (stop, stopped) = oneshot::channel();
        followed.add(
            "closing",
            Uuid::new_v4(),
            || checks.fetch_add(1, Ordering::SeqCst) == 0,
            || source(stop),
        );
        assert!(followed.tasks.lock().unwrap().is_empty());
        assert!(
            tokio::time::timeout(Duration::from_secs(1), stopped)
                .await
                .unwrap()
                .is_err()
        );
    }

    #[tokio::test]
    async fn one_windows_limit_cannot_stop_another_windows_source() {
        let followed = Followed::default();
        let shared = Uuid::new_v4();
        let (stop, _stopped) = oneshot::channel();
        followed.add("one", shared, || true, || source(stop));
        followed.add("two", shared, || true, || panic!("duplicate source"));
        for _ in 0..MAX_FOLLOWED {
            let (stop, _stopped) = oneshot::channel();
            followed.add("one", Uuid::new_v4(), || true, || source(stop));
        }
        let tasks = followed.tasks.lock().unwrap();
        let shared = tasks.iter().find(|task| task.project_id == shared).unwrap();
        assert_eq!(shared.windows, HashSet::from(["two".to_owned()]));
        assert_eq!(
            tasks
                .iter()
                .filter(|task| task.windows.contains("one"))
                .count(),
            MAX_FOLLOWED
        );
    }
}
