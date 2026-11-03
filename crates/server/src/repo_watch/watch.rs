//! The copies of each project's host lists, by user and project, and the
//! background sync that keeps the ones someone looks at fresh.

use super::{
    forward::{Forwards, Refresh},
    hosts::RemotesRead,
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
use tokio::{
    sync::{Notify, broadcast},
    time::{MissedTickBehavior, interval},
};

/// How often the background sync looks for copies to read again. A
/// webhook wakes it at once too.
const TICK: Duration = Duration::from_secs(5);
/// Reads of copies kept for the pages following them before the slowest
/// misses some (and reads everything again).
const MAX_CHANGES: usize = 256;
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
    /// The remotes last read from one of the project's checkouts, which
    /// name the other hosts its runs are on.
    remotes: Mutex<Option<RemotesRead>>,
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

    pub(super) fn remotes(&self) -> Option<RemotesRead> {
        self.remotes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    pub(super) fn set_remotes(&self, remotes: RemotesRead) {
        *self.remotes.lock().unwrap_or_else(PoisonError::into_inner) = Some(remotes);
    }
}

/// A copy the background sync read again, for the pages following the
/// project (`WatchProject`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Changed {
    pub(crate) user: String,
    pub(crate) project: ProjectId,
    pub(crate) lists: bool,
    pub(crate) runs: bool,
}

/// Every user's copies of their projects' pull requests, issues and
/// workflow runs, and the calls that read the runs.
pub struct RepoWatch {
    entries: Mutex<HashMap<Key, Arc<Entry>>>,
    pub(super) calls: HostCalls,
    /// The webhooks forwarded for live updates.
    pub(crate) forwards: Forwards,
    changes: broadcast::Sender<Changed>,
    /// Wakes the background sync when a webhook marked a copy stale.
    wake: Arc<Notify>,
}

impl Default for RepoWatch {
    fn default() -> Self {
        Self {
            entries: Mutex::default(),
            calls: HostCalls::default(),
            forwards: Forwards::default(),
            changes: broadcast::channel(MAX_CHANGES).0,
            wake: Arc::new(Notify::new()),
        }
    }
}

impl RepoWatch {
    /// The reads from now on.
    pub(crate) fn changes(&self) -> broadcast::Receiver<Changed> {
        self.changes.subscribe()
    }

    /// A webhook says `project`'s host changed: what `refresh` names is
    /// read again within seconds, while someone looks at the project.
    pub(crate) fn poke(&self, user: &str, project: ProjectId, refresh: Refresh) {
        if !refresh.lists && !refresh.runs {
            return;
        }
        let entry = self.entry(user, project);
        if refresh.lists {
            entry.lists.timing().poke();
        }
        if refresh.runs {
            entry.runs.timing().poke();
        }
        self.wake.notify_one();
    }

    fn wake_handle(&self) -> Arc<Notify> {
        self.wake.clone()
    }

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
                let lists =
                    due.lists && lists::sync_in_background(app, &user, &project, &due.entry).await;
                let runs =
                    due.runs && live::sync_in_background(app, &user, &project, &due.entry).await;
                if lists || runs {
                    // Nobody following is fine: the page reads on its own.
                    let _ = self.changes.send(Changed {
                        user: due.key.user.clone(),
                        project: due.key.project,
                        lists,
                        runs,
                    });
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
    let Some(wake) = app.upgrade().map(|app| app.repo_watch.wake_handle()) else {
        return;
    };
    loop {
        tokio::select! {
            _ = tick.tick() => {}
            () = wake.notified() => {}
        }
        let Some(app) = app.upgrade() else { return };
        app.repo_watch.sync_due(&app).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn an_event_reads_again_only_the_copy_it_is_about() {
        let watch = RepoWatch::default();
        let widget = ProjectId(Uuid::from_u128(1));
        let gadget = ProjectId(Uuid::from_u128(2));
        let start = Instant::now();
        for project in [widget, gadget] {
            let entry = watch.entry("alice", project);
            entry.lists.ask(start, false);
            entry.lists.timing().synced(start, true);
            entry.runs.ask(start, false);
            entry.runs.timing().synced(start, true);
        }
        let later = start + Duration::from_secs(10);
        assert!(watch.due(later).is_empty(), "both fresh");

        watch.poke("alice", widget, Refresh::of_event("pull_request"));
        let due = watch.due(later);
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].key.project, widget);
        assert!(due[0].lists && !due[0].runs);

        watch.poke("alice", gadget, Refresh::of_event("workflow_run"));
        let mut due = watch.due(later);
        due.sort_by_key(|due| due.key.project.0);
        assert_eq!(due.len(), 2);
        assert_eq!(due[1].key.project, gadget);
        assert!(!due[1].lists && due[1].runs);

        // Someone else's project of the same id is not touched.
        watch.poke("bob", widget, Refresh::of_event("push"));
        assert!(watch.due(later).iter().all(|due| due.key.user == "alice"));
    }

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
