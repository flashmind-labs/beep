//! The copies of each project's host lists, by user and project, and the
//! background sync that keeps the ones someone looks at fresh.

use super::{
    http::HostCalls,
    lists::{self, ListsCopy},
    live::{self, RunsCopy},
    timing::Timing,
};
use crate::{AsyncMutex, app::App, projects::model::ProjectId};
use futures::{StreamExt, stream};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, MutexGuard, PoisonError, Weak},
    time::{Duration, Instant},
};
use tokio::time::{MissedTickBehavior, interval};

/// How often the background sync looks for copies to read again.
const TICK: Duration = Duration::from_secs(30);
/// Projects the background sync reads at once.
const MAX_SYNCS: usize = 4;

/// Whose copy it is.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) struct Key {
    pub(super) user: String,
    pub(super) project: ProjectId,
}

/// One copy and when it was asked for and read. The copy's lock is held
/// across a read on purpose: a second ask waits for the first and takes
/// its copy.
#[derive(Default)]
pub(super) struct Watched<T> {
    timing: Mutex<Timing>,
    pub(super) copy: AsyncMutex<T>,
}

impl<T> Watched<T> {
    pub(super) fn timing(&self) -> MutexGuard<'_, Timing> {
        self.timing.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// What the page reads first of an ask: whether it must read the host.
pub(super) struct Ask {
    /// Refresh may read now.
    pub(super) forced: bool,
}

impl<T> Watched<T> {
    /// Notes an ask, and whether Refresh may read the host now.
    pub(super) fn ask(&self, now: Instant, refresh: bool) -> Ask {
        let mut timing = self.timing();
        timing.view(now);
        Ask {
            forced: refresh && timing.force(now),
        }
    }

    /// Whether the copy is old enough to read again, checked once its lock
    /// is held, so an ask that waited on another's read takes that one.
    pub(super) fn stale(&self) -> bool {
        self.timing().stale(Instant::now())
    }
}

/// An open pull request's number and head branch.
#[derive(Debug, Clone)]
pub(super) struct OpenHead {
    pub(super) number: u64,
    /// Empty when the host's CLI did not say.
    pub(super) branch: String,
}

/// One project's copies.
#[derive(Default)]
pub(super) struct Entry {
    pub(super) lists: Watched<ListsCopy>,
    pub(super) runs: Watched<RunsCopy>,
    /// The open pull requests' numbers and head branches, from the last
    /// list read, which the checks match runs with. Apart from `lists` so
    /// reading runs never waits on a list read.
    heads: Mutex<Vec<OpenHead>>,
}

impl Entry {
    pub(super) fn heads(&self) -> Vec<OpenHead> {
        self.heads
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    pub(super) fn set_heads(&self, heads: Vec<OpenHead>) {
        *self.heads.lock().unwrap_or_else(PoisonError::into_inner) = heads;
    }
}

/// Every user's copies of their projects' pull requests, issues and
/// workflow runs, and the calls that read the runs.
#[derive(Default)]
pub struct RepoWatch {
    entries: Mutex<HashMap<Key, Arc<Entry>>>,
    pub(super) calls: HostCalls,
}

impl RepoWatch {
    pub(super) fn entry(&self, user: &str, project: ProjectId) -> Arc<Entry> {
        self.entries
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entry(Key {
                user: user.to_owned(),
                project,
            })
            .or_default()
            .clone()
    }

    /// The copies the background sync reads now, dropping the ones nobody
    /// asked about for long.
    fn due(&self, now: Instant) -> Vec<Due> {
        let mut entries = self.entries.lock().unwrap_or_else(PoisonError::into_inner);
        entries.retain(|_, entry| {
            !(entry.lists.timing().forgotten(now) && entry.runs.timing().forgotten(now))
        });
        entries
            .iter()
            .filter_map(|(key, entry)| {
                let lists = entry.lists.timing().background_due(now);
                let runs = entry.runs.timing().background_due(now);
                (lists || runs).then(|| Due {
                    key: key.clone(),
                    entry: entry.clone(),
                    lists,
                    runs,
                })
            })
            .collect()
    }

    /// Reads again every copy someone looked at in the last half hour that
    /// is a few minutes old, a few projects at a time.
    async fn sync_due(&self, app: &Arc<App>) {
        let due = self.due(Instant::now());
        stream::iter(due)
            .for_each_concurrent(MAX_SYNCS, |due| async move {
                let Some(user) = app.user(&due.key.user) else {
                    return;
                };
                let project = match user.project(due.key.project).await {
                    Ok(Some(project)) => project,
                    Ok(None) => return,
                    Err(e) => {
                        log::warn!("repo watch: the project did not read: {e:#}");
                        return;
                    }
                };
                if due.lists {
                    lists::sync_in_background(app, &user, &project, &due.entry).await;
                }
                if due.runs {
                    live::sync_in_background(app, &user, &project, &due.entry).await;
                }
            })
            .await;
    }
}

/// A copy the background sync reads.
struct Due {
    key: Key,
    entry: Arc<Entry>,
    lists: bool,
    runs: bool,
}

/// Runs the background sync until the app goes. Holds it weakly: the app
/// owns the loop.
pub(super) async fn run(app: Weak<App>) {
    let mut tick = interval(TICK);
    tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
    loop {
        tick.tick().await;
        let Some(app) = app.upgrade() else { return };
        app.repo_watch.sync_due(&app).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn only_viewed_stale_copies_are_due_and_old_ones_are_dropped() {
        let watch = RepoWatch::default();
        let project = ProjectId(Uuid::from_u128(1));
        let start = Instant::now();
        let entry = watch.entry("alice", project);
        entry.lists.ask(start, false);
        entry.lists.timing().synced(start, true);
        assert!(watch.due(start + Duration::from_secs(60)).is_empty());
        let due = watch.due(start + Duration::from_secs(200));
        assert_eq!(due.len(), 1);
        assert!(due[0].lists && !due[0].runs, "runs were never asked for");
        // Nobody asked for two hours: the copy goes.
        assert!(
            watch
                .due(start + Duration::from_secs(3 * 60 * 60))
                .is_empty()
        );
        assert!(
            watch
                .entries
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .is_empty()
        );
    }
}
