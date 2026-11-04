//! What the desktop holds for the whole session: one HTTP client, and the last lists, runs and
//! spend of each project. A call for something already read gives it at once, and reads again in
//! the background when it is stale, then tells the page it changed. A project checked out on this
//! Mac is read from this Mac alone, so nothing waits on another machine.

use crate::{
    desk::{Desk, DesktopHosts},
    error::Result,
    peer::Peers,
};
use beep_proto::{HostedRuns, LabeledProjectLists, SpendReport};
use std::{
    collections::{HashMap, HashSet},
    hash::Hash,
    sync::{Arc, Mutex, PoisonError},
    time::{Duration, Instant},
};
use tokio::{sync::Semaphore, task::JoinSet};

/// How long a read is fresh; a call after that reads again in the background.
const FRESH: Duration = Duration::from_secs(60);
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
    fn get(&self, key: &K) -> Option<(T, bool)> {
        let held = self.held.lock().unwrap_or_else(PoisonError::into_inner);
        let held = held.get(key)?;
        Some((held.value.clone(), held.at.elapsed() >= FRESH))
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
}

impl Default for Options {
    fn default() -> Self {
        Self {
            settle: crate::usage::SETTLE,
            github_api: None,
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
            limit: Arc::new(Semaphore::new(READS_AT_ONCE)),
            tasks: Mutex::default(),
        }))
    }

    /// A desk over the mesh, waiting for it to open.
    pub async fn desk(&self) -> Result<Desk> {
        let mut desk = Desk::with(self.peers.clone(), self.hosts.clone()).await?;
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
        if !refresh && let Some((held, stale)) = self.lists.get(&key) {
            if stale {
                self.read_lists_later(key);
            }
            return Ok(held);
        }
        let read = self.desk().await?.lists(project).await?;
        self.lists.put(key, read.clone());
        Ok(read)
    }

    fn read_lists_later(self: &Arc<Self>, key: String) {
        if !self.lists.begin(&key) {
            return;
        }
        let this = self.clone();
        self.spawn(async move {
            let _slot = this.limit.acquire().await;
            let read = async { this.desk().await?.lists(&key).await }.await;
            match read {
                Ok(read) => this.lists.put(key.clone(), read),
                Err(e) => log::info!("a project's lists did not read again: {e}"),
            }
            this.lists.end(&key);
            (this.changed)(Changed {
                project: key,
                lists: true,
                runs: false,
            });
        });
    }

    /// The project's CI runs, kept as [`Session::lists`] keeps its lists.
    pub async fn runs(self: &Arc<Self>, project: &str, refresh: bool) -> Result<HostedRuns> {
        let key = project.to_owned();
        if !refresh && let Some((held, stale)) = self.runs.get(&key) {
            if stale {
                self.read_runs_later(key);
            }
            return Ok(held);
        }
        let read = self.desk().await?.runs(project).await?;
        self.runs.put(key, read.clone());
        Ok(read)
    }

    fn read_runs_later(self: &Arc<Self>, key: String) {
        if !self.runs.begin(&key) {
            return;
        }
        let this = self.clone();
        self.spawn(async move {
            let _slot = this.limit.acquire().await;
            let read = async { this.desk().await?.runs(&key).await }.await;
            match read {
                Ok(read) => this.runs.put(key.clone(), read),
                Err(e) => log::info!("a project's runs did not read again: {e}"),
            }
            this.runs.end(&key);
            (this.changed)(Changed {
                project: key,
                lists: false,
                runs: true,
            });
        });
    }

    /// What the agents cost and how much of their plans is left: the last report at once, and a
    /// new one collected behind it (it takes the machines a moment to say). `refresh`, or no
    /// report yet, waits for one.
    pub async fn spend(self: &Arc<Self>, refresh: bool) -> Result<SpendReport> {
        if !refresh && let Some((held, stale)) = self.spend.get(&()) {
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
                Err(e) => log::info!("the usage did not read again: {e}"),
            }
            this.spend.end(&());
        });
    }
}
