//! What the desktop holds for the whole session: one HTTP client, and the last lists, runs and
//! spend of each project. A call for something already read gives it at once, and reads again in
//! the background when it is stale, then tells the page it changed. A project checked out on this
//! Mac is read from this Mac alone, so nothing waits on another machine.

use crate::{
    AsyncMutex,
    changed::{Changed, ProjectRead},
    desk::{Desk, DesktopHosts},
    error::{FleetError, Result},
    machines::Machines,
};
use crate::{usage::freshest, usage_cache::PlanFollow};
use beep_proto::watched;
use beep_proto::{
    HostedRuns, LabeledProjectLists, PlanLimits, ProjectRunDetail, SpendPeriod, SpendReport,
};
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
    time::{Duration, Instant, SystemTime},
};
use tokio::{
    sync::{
        Semaphore,
        mpsc::{self, UnboundedReceiver, UnboundedSender},
        watch,
    },
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
/// How long a run list with a run still going is fresh, so its progress moves on the page's
/// poll; while the host's rate limit is low it keeps `Options::fresh`.
const GOING_FRESH: Duration = Duration::from_secs(5);
/// How long a running run's details are fresh: under the page's five-second poll, so each poll
/// reads the host.
const RUN_GOING_FRESH: Duration = Duration::from_secs(4);
/// How long a finished run's details are fresh. A run started again shows changed in the list,
/// and the page then asks past this copy.
const RUN_DONE_FRESH: Duration = Duration::from_secs(3600);
/// Runs' details kept, over every project.
const MAX_RUN_DETAILS: usize = 64;

fn empty_period() -> SpendPeriod {
    SpendPeriod {
        cost_usd: 0.0,
        metered_tokens: 0,
        subscription_tokens: 0,
        projects: Vec::new(),
        harnesses: Vec::new(),
        daily: Vec::new(),
    }
}

fn empty_spend() -> SpendReport {
    SpendReport {
        limits: Vec::new(),
        today: empty_period(),
        week: empty_period(),
        month: empty_period(),
        historical: Vec::new(),
    }
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
        self.get_by(key, |_| fresh)
    }

    /// The last read and whether it is older than `fresh` says that read stays fresh.
    fn get_by(&self, key: &K, fresh: impl Fn(&T) -> Duration) -> Option<(T, bool)> {
        let held = self.held.lock().unwrap_or_else(PoisonError::into_inner);
        let held = held.get(key)?;
        let stale = held.at.is_none_or(|at| at.elapsed() >= fresh(&held.value));
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

    /// Puts `value`, and drops the oldest reads past `max`.
    fn put_within(&self, key: K, value: T, max: usize) {
        let mut held = self.held.lock().unwrap_or_else(PoisonError::into_inner);
        held.insert(
            key,
            Held {
                at: Some(Instant::now()),
                value,
            },
        );

        while held.len() > max {
            let Some(oldest) = held
                .iter()
                .min_by_key(|(_, read)| read.at)
                .map(|(key, _)| key.clone())
            else {
                break;
            };
            held.remove(&oldest);
        }
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

    fn is_reading(&self, key: &K) -> bool {
        self.reading
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .contains(key)
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
            settle: crate::usage_cache::SETTLE,
            github_api: None,
            fresh: FRESH,
            mesh_wait: MESH_WAIT,
            usage_file: None,
        }
    }
}

/// The last spend report and the plans heard since, which are written to disk for the next run.
struct Spent {
    report: Cache<(), SpendReport>,
    /// The freshest plan of each agent any read heard, which `spend` gives: a machine's stream
    /// that still owes a plan adds it when the node sends it.
    plans: watch::Sender<Vec<PlanLimits>>,
    /// One write of `usage_file` at a time.
    saving: Mutex<()>,
}

impl Spent {
    /// Holds the report kept from the last run, if any.
    fn new(saved: Option<SpendReport>) -> Self {
        let report = Cache::default();
        let plans = saved.as_ref().map(|saved| saved.limits.clone());

        if let Some(saved) = saved {
            report.hold((), saved, None);
        }

        Self {
            report,
            plans: watch::Sender::new(plans.unwrap_or_default()),
            saving: Mutex::new(()),
        }
    }

    /// The plans heard so far.
    fn limits(&self) -> Vec<PlanLimits> {
        watched::read_sent(&self.plans, Clone::clone)
    }

    /// Takes the freshest of `plans` and those held; true when any visible plan field changed.
    fn hear(&self, plans: Vec<PlanLimits>) -> bool {
        self.plans.send_if_modified(|held| {
            let merged = freshest(held.iter().cloned().chain(plans));
            let changed = !same_limits(held, &merged);
            *held = merged;
            changed
        })
    }

    /// Writes what is held when the write starts, one write at a time, so the last one written
    /// is the newest.
    fn write(&self, path: &Path) -> io::Result<()> {
        let _one = self.saving.lock().unwrap_or_else(PoisonError::into_inner);
        let Some((mut report, _)) = self.report.get(&(), Duration::ZERO) else {
            return Ok(());
        };
        report.limits = self.limits();
        let bytes = rmp_serde::to_vec_named(&report).map_err(io::Error::other)?;
        let part = path.with_extension("part");
        fs::write(&part, bytes)?;
        fs::rename(&part, path)
    }
}

pub struct Session {
    options: Options,
    machines: Arc<Machines>,
    hosts: DesktopHosts,
    changes: UnboundedSender<Changed>,
    /// The feed's one reader, until the follower takes it.
    follower: Mutex<Option<UnboundedReceiver<Changed>>>,
    lists: Cache<String, LabeledProjectLists>,
    runs: Cache<String, HostedRuns>,
    run_reads: Mutex<HashMap<String, Arc<AsyncMutex<()>>>>,
    /// Runs opened, by project, host and run id.
    run_details: Cache<(String, Option<String>, u64), ProjectRunDetail>,
    spent: Spent,
    /// True once a cold `spend(false)` already waited its initial fast window on a slow machine.
    spend_waited: AtomicBool,
    /// The machines' streams kept for the plans they owe.
    following: AtomicUsize,
    /// Rises when what was read may no longer be what a read would give, so a read that started
    /// before is not kept.
    epoch: AtomicU64,
    /// Bounds the background reads.
    limit: Arc<Semaphore>,
    /// The background reads, owned here: they stop with the session.
    tasks: Mutex<JoinSet<()>>,
    /// Whether the first desk put the projects already imported in sync.
    checkouts_synced: AtomicBool,
}

impl Session {
    /// The feed of background reads that finished, for the one follower: `None` once taken.
    pub fn changes(&self) -> Option<UnboundedReceiver<Changed>> {
        self.follower
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
    }

    /// A session over `machines`. [`Session::changes`] tells of each background read that finished.
    pub fn new(machines: Arc<Machines>, options: Options) -> Result<Arc<Self>> {
        let saved = options.usage_file.as_deref().and_then(load_spend);
        let (changes, follower) = mpsc::unbounded_channel();
        Ok(Arc::new(Self {
            options,
            machines,
            hosts: DesktopHosts::new(changes.clone())?,
            changes,
            follower: Mutex::new(Some(follower)),
            lists: Cache::default(),
            runs: Cache::default(),
            run_reads: Mutex::default(),
            run_details: Cache::default(),
            spent: Spent::new(saved),
            spend_waited: AtomicBool::new(false),
            following: AtomicUsize::new(0),
            epoch: AtomicU64::new(0),
            limit: Arc::new(Semaphore::new(READS_AT_ONCE)),
            tasks: Mutex::default(),
            checkouts_synced: AtomicBool::new(false),
        }))
    }

    /// A desk over the mesh, waiting a few seconds for it to open.
    pub async fn desk(&self) -> Result<Desk> {
        let opened = Desk::with(self.machines.clone(), self.hosts.clone());
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
                this.lists.put(key.clone(), read.clone());
                let _ = this.changes.send(Changed::Project {
                    project: key,
                    read: ProjectRead::Lists(read),
                });
            }
        });
    }

    /// The project's CI runs, kept as [`Session::lists`] keeps its lists, but stale after
    /// [`GOING_FRESH`] while a run goes, so its progress moves on.
    pub async fn runs(self: &Arc<Self>, project: &str, refresh: bool) -> Result<HostedRuns> {
        let key = project.to_owned();
        let fresh = |runs: &HostedRuns| runs_fresh(runs, self.options.fresh, SystemTime::now());

        if !refresh && let Some((held, stale)) = self.runs.get_by(&key, fresh) {
            if stale {
                self.read_runs_later(key);
            }

            return Ok(held);
        }

        let gate = self.run_gate(project);
        let (_reading, waited) = match gate.try_lock() {
            Ok(guard) => (guard, false),
            Err(_) => (gate.lock().await, true),
        };

        if (!refresh || waited)
            && let Some((held, false)) = self.runs.get_by(&key, fresh)
        {
            return Ok(held);
        }

        let started = self.epoch.load(Ordering::Relaxed);
        let mut read = self.desk().await?.runs(project).await?;
        self.preserve_runs(&key, &mut read);

        if started == self.epoch.load(Ordering::Relaxed) {
            self.runs.put(key, read.clone());
        }

        Ok(read)
    }

    /// Run `id` of the project on `host` (its own host for `None`) with its jobs and the end of
    /// a failed job's log. A finished run is read once and then given from here; a running one
    /// is read again after [`RUN_GOING_FRESH`], or after `Options::fresh` while the host's rate
    /// limit is low. `refresh` reads the host now, for a run the list shows changed.
    pub async fn run(
        &self,
        project: &str,
        host: Option<&str>,
        id: u64,
        refresh: bool,
    ) -> Result<ProjectRunDetail> {
        let key = (project.to_owned(), host.map(str::to_owned), id);
        let fresh =
            |detail: &ProjectRunDetail| run_fresh(detail, self.options.fresh, SystemTime::now());
        if !refresh && let Some((held, false)) = self.run_details.get_by(&key, fresh) {
            return Ok(held);
        }

        let started = self.epoch.load(Ordering::Relaxed);
        let read = self.desk().await?.run(project, host, id).await?;

        if started == self.epoch.load(Ordering::Relaxed) {
            self.run_details
                .put_within(key, read.clone(), MAX_RUN_DETAILS);
        }

        Ok(read)
    }

    fn run_gate(&self, project: &str) -> Arc<AsyncMutex<()>> {
        self.run_reads
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entry(project.to_owned())
            .or_default()
            .clone()
    }

    fn preserve_runs(&self, key: &String, read: &mut HostedRuns) {
        if let Some((previous, _)) = self.runs.get(key, FRESH) {
            for host in &mut read.hosts {
                if host.error.is_some() {
                    host.synced_at = previous
                        .hosts
                        .iter()
                        .find(|old| old.host == host.host)
                        .and_then(|old| old.synced_at);
                    read.runs.extend(
                        previous
                            .runs
                            .iter()
                            .filter(|run| run.host == host.host)
                            .cloned(),
                    );
                    read.checks.extend(
                        previous
                            .checks
                            .iter()
                            .filter(|checks| checks.host == host.host)
                            .cloned(),
                    );
                }
            }
        }
    }

    fn read_runs_later(self: &Arc<Self>, key: String) {
        if !self.runs.begin(&key) {
            return;
        }

        let this = self.clone();
        self.spawn(async move {
            let _slot = this.limit.acquire().await;
            let started = this.epoch.load(Ordering::Relaxed);
            let gate = this.run_gate(&key);
            let _reading = gate.lock().await;
            if let Some((_, false)) = this.runs.get_by(&key, |runs| {
                runs_fresh(runs, this.options.fresh, SystemTime::now())
            }) {
                this.runs.end(&key);
                return;
            }

            let read = async { this.desk().await?.runs(&key).await }.await;
            let kept = started == this.epoch.load(Ordering::Relaxed);
            let read = match read {
                Ok(mut read) => {
                    this.preserve_runs(&key, &mut read);
                    Some(read)
                }
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
                this.runs.put(key.clone(), read.clone());
                let _ = this.changes.send(Changed::Project {
                    project: key,
                    read: ProjectRead::Runs(read),
                });
            }
        });
    }

    /// What the agents cost and how much of their plans is left: the last report at once, the
    /// one kept from the last run included, and a new one collected behind it (it takes the
    /// machines a moment to say). `refresh`, or no report yet, waits for one. A plan a machine
    /// sends after the report is added when it comes, and the feed gets [`Changed::Usage`].
    /// The pill polls once per `fresh`, so a report is stale at half that: each poll reads
    /// again, and a machine that came online since is heard within a poll.
    pub async fn spend(self: &Arc<Self>, refresh: bool) -> Result<SpendReport> {
        let mut report = match self.spent.report.get(&(), self.options.fresh / 2) {
            Some((held, stale)) if !refresh => {
                if stale {
                    self.read_spend_later();
                }

                held
            }
            None if !refresh => {
                if self.spend_waited.load(Ordering::Acquire) && self.spent.report.is_reading(&()) {
                    empty_spend()
                } else {
                    self.read_cold_spend().await?
                }
            }
            _ => {
                let (read, follows) = self.desk().await?.spend_and_follow().await?;
                self.keep_spend(read.clone(), follows);
                read
            }
        };
        report.limits = self.spent.limits();
        Ok(report)
    }

    async fn read_cold_spend(self: &Arc<Self>) -> Result<SpendReport> {
        let cold_wait = self.options.settle + Duration::from_millis(800);
        if !self.spent.report.begin(&()) {
            return Ok(self
                .spent
                .report
                .get(&(), self.options.fresh)
                .map_or_else(empty_spend, |(held, _)| held));
        }

        let (tx, rx) = tokio::sync::oneshot::channel();
        let this = self.clone();
        self.spawn(async move {
            let result = async { this.desk().await?.spend_and_follow().await }.await;
            let waited = this.spend_waited.swap(false, Ordering::AcqRel);
            let out = match result {
                Ok((read, follows)) => {
                    this.keep_spend(read.clone(), follows);
                    if waited {
                        let _ = this.changes.send(Changed::Usage);
                    }
                    Ok(read)
                }
                Err(e) => {
                    log::info!("the usage did not read on start: {e}");
                    Err(e)
                }
            };
            this.spent.report.end(&());
            let _ = tx.send(out);
        });

        match timeout(cold_wait, rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Ok(empty_spend()),
            Err(_) => {
                self.spend_waited.store(true, Ordering::Release);
                Ok(empty_spend())
            }
        }
    }

    fn read_spend_later(self: &Arc<Self>) {
        if !self.spent.report.begin(&()) {
            return;
        }

        let this = self.clone();
        self.spawn(async move {
            let _slot = this.limit.acquire().await;

            match async { this.desk().await?.spend_and_follow().await }.await {
                Ok((read, follows)) => {
                    this.spend_waited.store(false, Ordering::Release);
                    if this.keep_spend(read, follows) {
                        let _ = this.changes.send(Changed::Usage);
                    }
                }
                Err(e) => {
                    log::info!("the usage did not read again: {e}");
                    this.spent.report.touch(&());
                }
            }

            this.spent.report.end(&());
        });
    }

    /// Keeps `read` and its plans, and listens on for the plans `follows` owe.
    /// Returns whether the cached spend or visible plan limits changed.
    fn keep_spend(self: &Arc<Self>, read: SpendReport, follows: Vec<PlanFollow>) -> bool {
        let previous = self
            .spent
            .report
            .get(&(), Duration::ZERO)
            .map(|(held, _)| held);
        let limits_changed = self.spent.hear(read.limits.clone());
        let report_changed = previous
            .as_ref()
            .is_none_or(|before| !same_spend(before, &read));
        self.spent.report.put((), read);
        if limits_changed || report_changed {
            self.save_spend();
        }

        // A plan shown already is waited for too: it may be one kept from the last run, and over a
        // slow link the newer reading comes after every read went quiet.
        for follow in follows {
            if self.following.fetch_add(1, Ordering::Relaxed) >= PLAN_FOLLOWS {
                self.following.fetch_sub(1, Ordering::Relaxed);
                continue;
            }

            let this = self.clone();
            self.spawn(async move {
                let late_spend = Arc::new(AtomicBool::new(false));
                let flag = late_spend.clone();
                follow
                    .run(
                        |plan| {
                            if this.spent.hear(vec![plan]) {
                                this.save_spend();
                                let _ = this.changes.send(Changed::Usage);
                            }
                        },
                        move || {
                            flag.store(true, Ordering::Release);
                        },
                    )
                    .await;

                if late_spend.load(Ordering::Acquire)
                    && let Ok(desk) = this.desk().await
                {
                    let updated = desk.spend_from_cache();
                    if this.keep_spend(updated, Vec::new()) {
                        let _ = this.changes.send(Changed::Usage);
                    }
                }

                this.following.fetch_sub(1, Ordering::Relaxed);
            });
        }

        limits_changed || report_changed
    }

    /// Writes the report held and its plans to `usage_file`, off the async workers.
    fn save_spend(self: &Arc<Self>) {
        let Some(path) = self.options.usage_file.clone() else {
            return;
        };
        let this = self.clone();
        self.spawn(async move {
            let saved = tokio::task::spawn_blocking(move || this.spent.write(&path)).await;

            match saved {
                Ok(Ok(())) => {}
                Ok(Err(e)) => log::warn!("the usage was not kept for the next run: {e}"),
                Err(e) => log::warn!("keeping the usage stopped: {e}"),
            }
        });
    }

    /// Drops what was read, for a project that was removed or a credential that changed, so the
    /// next call reads again and no one is given another account's data. A read in flight is not
    /// kept.
    pub fn forget(&self) {
        self.epoch.fetch_add(1, Ordering::Relaxed);
        self.lists.clear();
        self.runs.clear();
        self.run_details.clear();
    }

    /// Waits for the background reads that are running.
    pub async fn settled(&self) {
        let mut reads =
            std::mem::take(&mut *self.tasks.lock().unwrap_or_else(PoisonError::into_inner));
        while reads.join_next().await.is_some() {}
    }
}

/// Whether two plan lists carry the same visible values (ignoring read timestamps).
fn same_limits(a: &[PlanLimits], b: &[PlanLimits]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|(left, right)| {
            left.agent == right.agent
                && left.account == right.account
                && left.plan == right.plan
                && left.machine_name == right.machine_name
                && left.windows == right.windows
                && left.credits == right.credits
        })
}

/// Whether two spend reports carry the same spend totals and history (limits are checked via `same_limits`).
fn same_spend(a: &SpendReport, b: &SpendReport) -> bool {
    a.today == b.today && a.week == b.week && a.month == b.month && a.historical == b.historical
}

/// Whether the host's rate limit is low at `now`, by what a read said.
fn low(until: Option<SystemTime>, now: SystemTime) -> bool {
    until.is_some_and(|until| until > now)
}

/// How long a run list stays fresh: [`GOING_FRESH`] while a run goes and no host's rate limit is
/// low, else `fresh`.
fn runs_fresh(runs: &HostedRuns, fresh: Duration, now: SystemTime) -> Duration {
    let going = runs.runs.iter().any(|hosted| hosted.run.state.going());
    let limited = runs
        .hosts
        .iter()
        .any(|host| low(host.rate_limited_until, now));
    if going && !limited {
        GOING_FRESH.min(fresh)
    } else {
        fresh
    }
}

/// How long a run's details stay fresh: a finished run's for [`RUN_DONE_FRESH`], a running
/// one's for [`RUN_GOING_FRESH`], and `fresh` while the rate limit is low or for a read that
/// came back with a problem, such as a log that did not read.
fn run_fresh(detail: &ProjectRunDetail, fresh: Duration, now: SystemTime) -> Duration {
    if detail.error.is_some() {
        fresh
    } else if !detail.run.state.going() {
        RUN_DONE_FRESH
    } else if low(detail.rate_limited_until, now) {
        fresh
    } else {
        RUN_GOING_FRESH.min(fresh)
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
#[path = "tests/session.rs"]
mod tests;
