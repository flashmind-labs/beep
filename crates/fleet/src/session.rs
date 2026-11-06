//! What the desktop holds for the whole session: one HTTP client, and the last lists, runs and
//! spend of each project. A call for something already read gives it at once, and reads again in
//! the background when it is stale, then tells the page it changed. A project checked out on this
//! Mac is read from this Mac alone, so nothing waits on another machine.

use crate::usage::{PlanFollow, freshest};
use crate::{
    desk::{Desk, DesktopHosts},
    error::{FleetError, Result},
    peer::Peers,
};
use beep_proto::{HostedRuns, LabeledProjectLists, PlanLimits, SpendReport};
use std::{
    collections::{HashMap, HashSet},
    fs,
    hash::Hash,
    io,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::{
    sync::{Semaphore, watch},
    task::JoinSet,
    time::timeout,
};

/// How long a read is fresh; a call after that reads again in the background.
const FRESH: Duration = Duration::from_secs(60);
/// How long a call waits for the mesh to open.
const MESH_WAIT: Duration = Duration::from_secs(5);
/// Background reads at once, over every project.
const READS_AT_ONCE: usize = 4;
/// Machines' streams kept open at once for the plans they owe.
const PLAN_FOLLOWS: usize = 16;

/// What a background read changed, for the page that shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Changed {
    Project {
        /// `org/repo`.
        project: String,
        lists: bool,
        runs: bool,
    },
    /// The plans and spending: `Session::spend` gives more than before.
    Usage,
}

/// Something read, and when; `None` for a read kept from an earlier run, which is stale.
struct Held<T> {
    at: Option<Instant>,
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
        let stale = held.at.is_none_or(|at| at.elapsed() >= fresh);
        Some((held.value.clone(), stale))
    }

    /// Dates the last read now, as a read that failed does, so the next try waits for its age.
    fn touch(&self, key: &K) {
        if let Some(held) = self
            .held
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get_mut(key)
        {
            held.at = Some(Instant::now());
        }
    }

    fn clear(&self) {
        self.held
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clear();
    }

    fn put(&self, key: K, value: T) {
        self.hold(key, value, Some(Instant::now()));
    }

    fn hold(&self, key: K, value: T, at: Option<Instant>) {
        let held = Held { at, value };
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
    /// Where the last usage report is kept, so the next run shows it at once.
    pub usage_file: Option<PathBuf>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            settle: crate::usage::SETTLE,
            github_api: None,
            fresh: FRESH,
            mesh_wait: MESH_WAIT,
            usage_file: None,
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
    /// The freshest plan of each agent any read heard, which `spend` gives: a machine's stream
    /// that still owes a plan adds it when the node sends it.
    plans: watch::Sender<Vec<PlanLimits>>,
    /// The machines' streams kept for the plans they owe.
    following: AtomicUsize,
    /// Rises when what was read may no longer be what a read would give, so a read that started
    /// before is not kept.
    epoch: AtomicU64,
    /// Bounds the background reads.
    limit: Arc<Semaphore>,
    /// One write of `usage_file` at a time.
    saving: Mutex<()>,
    /// The background reads, owned here: they stop with the session.
    tasks: Mutex<JoinSet<()>>,
    /// Whether the first desk put the projects already imported in sync.
    checkouts_synced: AtomicBool,
}

impl Session {
    /// A session over `peers`. `changed` hears of each background read that finished.
    pub fn new(
        peers: Arc<dyn Peers>,
        options: Options,
        changed: impl Fn(Changed) + Send + Sync + 'static,
    ) -> Result<Arc<Self>> {
        let spend = Cache::default();
        let saved = options.usage_file.as_deref().and_then(load_spend);
        let plans = saved.as_ref().map(|report| report.limits.clone());
        if let Some(report) = saved {
            spend.hold((), report, None);
        }
        Ok(Arc::new(Self {
            options,
            peers,
            hosts: DesktopHosts::new()?,
            changed: Arc::new(changed),
            lists: Cache::default(),
            runs: Cache::default(),
            spend,
            plans: watch::Sender::new(plans.unwrap_or_default()),
            following: AtomicUsize::new(0),
            epoch: AtomicU64::new(0),
            limit: Arc::new(Semaphore::new(READS_AT_ONCE)),
            saving: Mutex::new(()),
            tasks: Mutex::default(),
            checkouts_synced: AtomicBool::new(false),
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
        if !self.checkouts_synced.swap(true, Ordering::Relaxed)
            && let Err(e) = desk.sync_all_checkouts().await
        {
            // The next session writes them; the desk is good without.
            log::warn!("the imported projects are not all synced: {e}");
        }
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
                (this.changed)(Changed::Project {
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
                (this.changed)(Changed::Project {
                    project: key,
                    lists: false,
                    runs: true,
                });
            }
        });
    }

    /// What the agents cost and how much of their plans is left: the last report at once, the
    /// one kept from the last run included, and a new one collected behind it (it takes the
    /// machines a moment to say). `refresh`, or no report yet, waits for one. A plan a machine
    /// sends after the report is added when it comes, and `changed` hears [`Changed::Usage`].
    /// The pill polls once per `fresh`, so a report is stale at half that: each poll reads
    /// again, and a machine that came online since is heard within a poll.
    pub async fn spend(self: &Arc<Self>, refresh: bool) -> Result<SpendReport> {
        let mut report = match self.spend.get(&(), self.options.fresh / 2) {
            Some((held, stale)) if !refresh => {
                if stale {
                    self.read_spend_later();
                }
                held
            }
            _ => {
                let (read, follows) = self.desk().await?.spend_and_follow().await?;
                self.keep_spend(read.clone(), follows);
                read
            }
        };
        report.limits = self.plans.borrow().clone();
        Ok(report)
    }

    fn read_spend_later(self: &Arc<Self>) {
        if !self.spend.begin(&()) {
            return;
        }
        let this = self.clone();
        self.spawn(async move {
            let _slot = this.limit.acquire().await;
            match async { this.desk().await?.spend_and_follow().await }.await {
                Ok((read, follows)) => {
                    this.keep_spend(read, follows);
                    (this.changed)(Changed::Usage);
                }
                Err(e) => {
                    log::info!("the usage did not read again: {e}");
                    this.spend.touch(&());
                }
            }
            this.spend.end(&());
        });
    }

    /// Keeps `read` and its plans, and listens on for the plans `follows` owe.
    fn keep_spend(self: &Arc<Self>, read: SpendReport, follows: Vec<PlanFollow>) {
        self.hear(read.limits.clone());
        self.spend.put((), read);
        self.save_spend();
        for mut follow in follows {
            // A plan of the agent already shown is not waited for: the next read brings it.
            let shown = self.plans.borrow().clone();
            follow
                .waiting
                .retain(|agent| !shown.iter().any(|plan| plan.agent == *agent));
            if follow.waiting.is_empty() {
                continue;
            }
            if self.following.fetch_add(1, Ordering::Relaxed) >= PLAN_FOLLOWS {
                self.following.fetch_sub(1, Ordering::Relaxed);
                continue;
            }
            let this = self.clone();
            self.spawn(async move {
                follow
                    .run(|plan| {
                        if this.hear(vec![plan]) {
                            this.save_spend();
                            (this.changed)(Changed::Usage);
                        }
                    })
                    .await;
                this.following.fetch_sub(1, Ordering::Relaxed);
            });
        }
    }

    /// Writes the report held and its plans to `usage_file`, off the async workers.
    fn save_spend(self: &Arc<Self>) {
        let Some(path) = self.options.usage_file.clone() else {
            return;
        };
        let this = self.clone();
        self.spawn(async move {
            let saved = tokio::task::spawn_blocking(move || this.write_spend(&path)).await;
            match saved {
                Ok(Ok(())) => {}
                Ok(Err(e)) => log::warn!("the usage was not kept for the next run: {e}"),
                Err(e) => log::warn!("keeping the usage stopped: {e}"),
            }
        });
    }

    /// Writes what is held when the write starts, one write at a time, so the last one written
    /// is the newest.
    fn write_spend(&self, path: &Path) -> io::Result<()> {
        let _one = self.saving.lock().unwrap_or_else(PoisonError::into_inner);
        let Some((mut report, _)) = self.spend.get(&(), Duration::ZERO) else {
            return Ok(());
        };
        report.limits = self.plans.borrow().clone();
        let bytes = rmp_serde::to_vec_named(&report).map_err(io::Error::other)?;
        let part = path.with_extension("part");
        fs::write(&part, bytes)?;
        fs::rename(&part, path)
    }

    /// Takes the freshest of `plans` and those held; true when that changed anything.
    fn hear(&self, plans: Vec<PlanLimits>) -> bool {
        self.plans.send_if_modified(|held| {
            let merged = freshest(held.iter().cloned().chain(plans));
            let changed = merged != *held;
            *held = merged;
            changed
        })
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

/// The report the last run kept at `path`; `None` without one.
fn load_spend(path: &Path) -> Option<SpendReport> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return None,
        Err(e) => {
            log::warn!("the usage kept by the last run was not read: {e}");
            return None;
        }
    };
    rmp_serde::from_slice(&bytes)
        .inspect_err(|e| log::warn!("the usage kept by the last run did not decode: {e}"))
        .ok()
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
            .at = None;
        assert_eq!(cache.get(&"a".to_owned(), hour), Some((1, true)));
        cache.touch(&"a".to_owned());
        assert_eq!(cache.get(&"a".to_owned(), hour), Some((1, false)));
    }
}
