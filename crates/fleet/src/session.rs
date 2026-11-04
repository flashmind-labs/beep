//! What the desktop holds for the whole session: one HTTP client, and the last lists, runs and
//! spend of each project. A call for something already read gives it at once, and reads again in
//! the background when it is stale, then tells the page it changed. A project checked out on this
//! Mac is read from this Mac alone, so nothing waits on another machine.

use crate::{
    desk::{Desk, DesktopHosts},
    error::{FleetError, Result},
    peer::Peers,
};
use beep_proto::{HostedRuns, LabeledProjectLists, SpendReport};
use std::{
    collections::{HashMap, HashSet},
    hash::Hash,
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::{sync::Semaphore, task::JoinSet, time::timeout};

/// How long a read is fresh; a call after that reads again in the background.
const FRESH: Duration = Duration::from_secs(60);
/// How long a call waits for the mesh to open.
const MESH_WAIT: Duration = Duration::from_secs(5);
/// Background reads at once, over every project.
const READS_AT_ONCE: usize = 4;

/// What a background read changed, for the page that shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Changed {
    /// `org/repo`.
    pub project: String,
    pub lists: bool,
    pub runs: bool,
}

/// Something read, and when.
struct Held<T> {
    at: Instant,
    value: T,
}

/// The last reads, by key, and the keys being read now.
struct Cache<K, T> {
    held: Mutex<HashMap<K, Held<T>>>,
    reading: Mutex<HashSet<K>>,
}

impl<K, T> Default for Cache<K, T> {
    fn default() -> Self {
        Self {
            held: Mutex::default(),
            reading: Mutex::default(),
        }
    }
}

impl<K: Hash + Eq + Clone, T: Clone> Cache<K, T> {
    /// The last read and whether it is older than `fresh`.
    fn get(&self, key: &K, fresh: Duration) -> Option<(T, bool)> {
        let held = self.held.lock().unwrap_or_else(PoisonError::into_inner);
        let held = held.get(key)?;
        Some((held.value.clone(), held.at.elapsed() >= fresh))
    }

    /// Dates the last read now, as a read that failed does, so the next try waits for its age.
    fn touch(&self, key: &K) {
        if let Some(held) = self
            .held
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get_mut(key)
        {
            held.at = Instant::now();
        }
    }

    fn clear(&self) {
        self.held
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clear();
    }

    fn put(&self, key: K, value: T) {
        let held = Held {
            at: Instant::now(),
            value,
        };
        self.held
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(key, held);
    }

    /// Marks `key` as being read; false when it already is, so one read runs at a time.
    fn begin(&self, key: &K) -> bool {
        self.reading
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(key.clone())
    }

    fn end(&self, key: &K) {
        self.reading
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(key);
    }
}

/// What a session is set up with; the default is what the app uses.
#[derive(Debug, Clone)]
pub struct Options {
    /// How long a node may be quiet before a usage read takes what it said as all of it.
    pub settle: Duration,
    /// Where GitHub's API is, when it is not github.com.
    pub github_api: Option<String>,
    /// How long a read stays fresh.
    pub fresh: Duration,
    /// How long a call waits for the mesh to open.
    pub mesh_wait: Duration,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            settle: crate::usage::SETTLE,
            github_api: None,
            fresh: FRESH,
            mesh_wait: MESH_WAIT,
        }
    }
}

pub struct Session {
    options: Options,
    peers: Arc<dyn Peers>,
    hosts: DesktopHosts,
    changed: Arc<dyn Fn(Changed) + Send + Sync>,
    lists: Cache<String, LabeledProjectLists>,
    runs: Cache<String, HostedRuns>,
    spend: Cache<(), SpendReport>,
    /// Rises when what was read may no longer be what a read would give, so a read that started
    /// before is not kept.
    epoch: AtomicU64,
    /// Bounds the background reads.
    limit: Arc<Semaphore>,
    /// The background reads, owned here: they stop with the session.
    tasks: Mutex<JoinSet<()>>,
}

impl Session {
    /// A session over `peers`. `changed` hears of each background read that finished.
    pub fn new(
        peers: Arc<dyn Peers>,
        options: Options,
        changed: impl Fn(Changed) + Send + Sync + 'static,
    ) -> Result<Arc<Self>> {
        Ok(Arc::new(Self {
            options,
            peers,
            hosts: DesktopHosts::new()?,
            changed: Arc::new(changed),
            lists: Cache::default(),
            runs: Cache::default(),
            spend: Cache::default(),
            epoch: AtomicU64::new(0),
            limit: Arc::new(Semaphore::new(READS_AT_ONCE)),
            tasks: Mutex::default(),
        }))
    }

    /// A desk over the mesh, waiting a few seconds for it to open.
    pub async fn desk(&self) -> Result<Desk> {
        let opened = Desk::with(self.peers.clone(), self.hosts.clone());
        let mut desk = timeout(self.options.mesh_wait, opened)
            .await
            .map_err(|_| FleetError::NoMesh("still opening".into()))??;
        desk.settle = self.options.settle;
        desk.github_api.clone_from(&self.options.github_api);
        Ok(desk)
    }

    fn spawn(&self, read: impl Future<Output = ()> + Send + 'static) {
        let mut tasks = self.tasks.lock().unwrap_or_else(PoisonError::into_inner);
        // Reads that ended are collected here.
        while tasks.try_join_next().is_some() {}
        tasks.spawn(read);
    }

    /// The project's pull requests and issues: the last read at once, and read again behind it
    /// when it is stale. `refresh` reads the host now. A project never read waits for its first
    /// read, which asks this Mac alone when the project is checked out here.
    pub async fn lists(
        self: &Arc<Self>,
        project: &str,
        refresh: bool,
    ) -> Result<LabeledProjectLists> {
        let key = project.to_owned();
        if !refresh && let Some((held, stale)) = self.lists.get(&key, self.options.fresh) {
            if stale {
                self.read_lists_later(key);
            }
            return Ok(held);
        }
        let started = self.epoch.load(Ordering::Relaxed);
        let read = self.desk().await?.lists(project).await?;
        if started == self.epoch.load(Ordering::Relaxed) {
            self.lists.put(key, read.clone());
        }
        Ok(read)
    }

    fn read_lists_later(self: &Arc<Self>, key: String) {
        if !self.lists.begin(&key) {
            return;
        }
        let this = self.clone();
        self.spawn(async move {
            let _slot = this.limit.acquire().await;
            let started = this.epoch.load(Ordering::Relaxed);
            let read = async { this.desk().await?.lists(&key).await }.await;
            let kept = started == this.epoch.load(Ordering::Relaxed);
            let read = match read {
                Ok(read) => Some(read),
                // The page would read again at once and ask again: it waits for the age.
                Err(e) => {
                    log::info!("a project's lists did not read again: {e}");
                    this.lists.touch(&key);
                    None
                }
            };
            this.lists.end(&key);
            if let Some(read) = read
                && kept
            {
                this.lists.put(key.clone(), read);
                (this.changed)(Changed {
                    project: key,
                    lists: true,
                    runs: false,
                });
            }
        });
    }

    /// The project's CI runs, kept as [`Session::lists`] keeps its lists.
    pub async fn runs(self: &Arc<Self>, project: &str, refresh: bool) -> Result<HostedRuns> {
        let key = project.to_owned();
        if !refresh && let Some((held, stale)) = self.runs.get(&key, self.options.fresh) {
            if stale {
                self.read_runs_later(key);
            }
            return Ok(held);
        }
        let started = self.epoch.load(Ordering::Relaxed);
        let read = self.desk().await?.runs(project).await?;
        if started == self.epoch.load(Ordering::Relaxed) {
            self.runs.put(key, read.clone());
        }
        Ok(read)
    }

    fn read_runs_later(self: &Arc<Self>, key: String) {
        if !self.runs.begin(&key) {
            return;
        }
        let this = self.clone();
        self.spawn(async move {
            let _slot = this.limit.acquire().await;
            let started = this.epoch.load(Ordering::Relaxed);
            let read = async { this.desk().await?.runs(&key).await }.await;
            let kept = started == this.epoch.load(Ordering::Relaxed);
            let read = match read {
                Ok(read) => Some(read),
                Err(e) => {
                    log::info!("a project's runs did not read again: {e}");
                    this.runs.touch(&key);
                    None
                }
            };
            this.runs.end(&key);
            if let Some(read) = read
                && kept
            {
                this.runs.put(key.clone(), read);
                (this.changed)(Changed {
                    project: key,
                    lists: false,
                    runs: true,
                });
            }
        });
    }

    /// What the agents cost and how much of their plans is left: the last report at once, and a
    /// new one collected behind it (it takes the machines a moment to say). `refresh`, or no
    /// report yet, waits for one.
    pub async fn spend(self: &Arc<Self>, refresh: bool) -> Result<SpendReport> {
        if !refresh && let Some((held, stale)) = self.spend.get(&(), self.options.fresh) {
            if stale {
                self.read_spend_later();
            }
            return Ok(held);
        }
        let read = self.desk().await?.spend().await?;
        self.spend.put((), read.clone());
        Ok(read)
    }

    fn read_spend_later(self: &Arc<Self>) {
        if !self.spend.begin(&()) {
            return;
        }
        let this = self.clone();
        self.spawn(async move {
            let _slot = this.limit.acquire().await;
            match async { this.desk().await?.spend().await }.await {
                Ok(read) => this.spend.put((), read),
                Err(e) => {
                    log::info!("the usage did not read again: {e}");
                    this.spend.touch(&());
                }
            }
            this.spend.end(&());
        });
    }

    /// Drops what was read, for a project that was removed or a credential that changed, so the
    /// next call reads again and no one is given another account's data. A read in flight is not
    /// kept.
    pub fn forget(&self) {
        self.epoch.fetch_add(1, Ordering::Relaxed);
        self.lists.clear();
        self.runs.clear();
    }

    /// Waits for the background reads that are running.
    pub async fn settled(&self) {
        let mut reads =
            std::mem::take(&mut *self.tasks.lock().unwrap_or_else(PoisonError::into_inner));
        while reads.join_next().await.is_some() {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failed_read_dates_the_last_one_so_the_next_try_waits_for_its_age() {
        let cache: Cache<String, u8> = Cache::default();
        cache.put("a".into(), 1);
        let hour = Duration::from_secs(3600);
        cache
            .held
            .lock()
            .expect("not poisoned")
            .get_mut("a")
            .expect("held")
            .at = Instant::now() - 2 * hour;
        assert_eq!(cache.get(&"a".to_owned(), hour), Some((1, true)));
        cache.touch(&"a".to_owned());
        assert_eq!(cache.get(&"a".to_owned(), hour), Some((1, false)));
    }
}
