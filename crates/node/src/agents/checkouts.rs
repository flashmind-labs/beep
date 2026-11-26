//! One task per checkout keeps what git says of it, so the sessions and
//! peers that read a folder share one git run for each change to it.

use super::{work_diff, work_folder::{GitStatus, git_status}};
use crate::{actor, events::paths::Paths};
use anyhow::{Result, anyhow};
use beep_proto::{
    WorkDiff,
    node::agent::{DiffAsk, LineChanges},
};
use std::{
    collections::{HashMap, VecDeque},
    path::{Path, PathBuf},
    sync::{Arc, OnceLock},
    time::{Duration, Instant},
};
use tokio::{
    sync::{Notify, oneshot},
    task::{JoinSet, spawn_blocking},
    time::interval,
};
use tokio_util::task::AbortOnDropHandle;

/// A checkout nobody read between two sweeps ends its task and lets its
/// watch go, so it lingers this long at least and twice it at most.
const SWEEP: Duration = Duration::from_secs(150);
/// The diffs one checkout keeps, a few megabytes each at most.
const KEPT: usize = 8;
/// A kept diff is read again once it is this old, should the watch have
/// missed a change.
const KEPT_FOR: Duration = Duration::from_secs(30);
/// How often a folder without Git metadata asks Git for it again.
const GIT_RETRY: Duration = Duration::from_secs(30);
const WAITING: usize = 64;
const STOPPED: &str = "the checkout's reader stopped";

/// What git gave for a read, as its readers share it.
type Read<V> = Result<Arc<V>, Arc<anyhow::Error>>;
type Reply<V> = oneshot::Sender<Read<V>>;

/// The node's checkouts. The task that owns them starts with the first read
/// and ends with this.
#[derive(Default)]
pub(crate) struct Checkouts {
    started: OnceLock<Started>,
}

struct Started {
    opens: actor::Sender<Open>,
    _task: AbortOnDropHandle<()>,
}

/// Asks for the task of the checkout at `root`, started when it has none.
struct Open {
    root: PathBuf,
    reply: oneshot::Sender<actor::Sender<Ask>>,
}

impl From<&Open> for &'static str {
    fn from(_: &Open) -> Self {
        "Open"
    }
}

/// Asks one checkout for what git says of it.
#[derive(strum::IntoStaticStr)]
enum Ask {
    Diff {
        ask: DiffAsk,
        reply: Reply<WorkDiff>,
    },
    Status {
        reply: Reply<GitStatus>,
    },
}

impl Checkouts {
    /// The diff `ask` names in `root`, a real folder in reach: the kept one
    /// while the checkout has not changed since it was read.
    pub(crate) async fn diff(&self, root: PathBuf, ask: DiffAsk) -> Result<Arc<WorkDiff>> {
        let (reply, read) = oneshot::channel();
        // Held until the answer comes, so the sweep leaves the task be.
        let asks = self.open(root).await?;
        asks.send(Ask::Diff { ask, reply })
            .await
            .map_err(|_| anyhow!(STOPPED))?;

        read.await
            .map_err(|_| anyhow!(STOPPED))?
            .map_err(|e| anyhow!("{e:#}"))
    }

    /// What `git status` says of the checkout at `root`, a real folder in
    /// reach: the kept one while the checkout has not changed since it was
    /// read. `None` outside a git checkout, when it cannot be read, and when
    /// nothing is kept because no watch holds: the caller asks git itself.
    pub(in crate::agents) async fn status(&self, root: PathBuf) -> Option<Arc<GitStatus>> {
        let (reply, read) = oneshot::channel();
        let asks = self.open(root).await.ok()?;
        asks.send(Ask::Status { reply }).await.ok()?;
        let read = read.await.ok()?.ok();
        drop(asks);

        read
    }

    /// The lines added and removed in the checkout at `folder`, as the diff
    /// viewer counts them against the default branch with the working tree.
    /// `None` outside a git checkout.
    pub(crate) async fn changes(&self, folder: PathBuf) -> Option<LineChanges> {
        let root = spawn_blocking(move || crate::fs::paths::real(&folder))
            .await
            .ok()?
            .ok()?;
        let ask = DiffAsk {
            root: String::new(),
            base: String::new(),
            head: String::new(),
            only: Vec::new(),
            lockfiles: false,
            context: 0,
        };
        let diff = self.diff(root, ask).await.ok()?;

        Some(LineChanges {
            additions: diff.additions,
            deletions: diff.deletions,
        })
    }

    async fn open(&self, root: PathBuf) -> Result<actor::Sender<Ask>> {
        let started = self.started.get_or_init(|| {
            let (opens, opened) = actor::channel("checkouts", WAITING);
            Started {
                opens,
                _task: AbortOnDropHandle::new(tokio::spawn(supervise(opened))),
            }
        });
        let (reply, asks) = oneshot::channel();
        started
            .opens
            .send(Open { root, reply })
            .await
            .map_err(|_| anyhow!(STOPPED))?;

        asks.await.map_err(|_| anyhow!(STOPPED))
    }
}

/// A checkout's task as the supervisor holds it.
struct Held {
    asks: actor::Sender<Ask>,
    /// Opened since the last sweep.
    used: bool,
}

/// Owns the checkouts' tasks: hands each reader the one for its folder and
/// ends those nobody reads.
async fn supervise(mut opens: actor::Receiver<Open>) {
    let mut held = HashMap::<PathBuf, Held>::new();
    let mut tasks = JoinSet::new();
    let mut sweep = interval(SWEEP);

    loop {
        tokio::select! {
            _ = sweep.tick() => {
                // Dropping the last sender ends the checkout's task. A reader
                // waiting for its answer holds another, and keeps it.
                held.retain(|_, held| {
                    std::mem::take(&mut held.used) || held.asks.strong_count() > 1
                });
            }
            Some(_) = tasks.join_next() => {}
            open = opens.recv() => {
                let Some((Open { root, reply }, _timing)) = open else {
                    return;
                };
                let asks = match held.get_mut(&root) {
                    Some(held) if !held.asks.is_closed() => {
                        held.used = true;
                        held.asks.clone()
                    }
                    _ => {
                        let (asks, asked) = actor::channel("checkout", WAITING);
                        tasks.spawn(Checkout::new(root.clone()).run(asked));
                        held.insert(root, Held { asks: asks.clone(), used: true });
                        asks
                    }
                };
                let _ = reply.send(asks);
            }
        }
    }
}

/// One checkout: its watch, the diffs kept of it and those being read.
struct Checkout {
    root: PathBuf,
    /// Counts the changes heard, so a diff knows whether it is still current.
    revision: u64,
    changed: Arc<Notify>,
    /// The watch on the folder and its Git metadata. Without it nothing is
    /// kept: a change would go unheard.
    watch: Option<Paths>,
    /// When Git was last asked for the metadata to watch; `None` while it
    /// is being asked.
    tried: Option<Instant>,
    diffs: Reads<DiffAsk, WorkDiff>,
    /// The status of the whole checkout: one read, whatever folder asks.
    statuses: Reads<(), GitStatus>,
    jobs: JoinSet<Job>,
}

/// What git gave for each `K` asked of one checkout: kept until the
/// checkout changes, and shared by the readers that ask while it is read.
struct Reads<K, V> {
    keep: usize,
    kept: VecDeque<Kept<K, V>>,
    reading: Vec<Reading<K, V>>,
}

struct Kept<K, V> {
    key: K,
    value: Arc<V>,
    read: Instant,
}

/// A read git is working on, and who waits for it.
struct Reading<K, V> {
    key: K,
    revision: u64,
    waiting: Vec<Reply<V>>,
    /// Those who asked after the checkout changed again: this read may have
    /// missed the change, so they get the next one.
    late: Vec<Reply<V>>,
}

enum Job {
    Watched(Option<Paths>),
    Diffed { ask: DiffAsk, read: Result<WorkDiff> },
    Statused(Result<GitStatus>),
}

impl<K: PartialEq + Clone, V> Reads<K, V> {
    fn new(keep: usize) -> Self {
        Self {
            keep,
            kept: VecDeque::new(),
            reading: Vec::new(),
        }
    }

    /// Answers with the kept value, or joins the read under way. Gives back
    /// the reply when no read is, for `begin`. `current` is whether a change
    /// since `revision` would be heard.
    fn ask(&mut self, key: &K, reply: Reply<V>, current: bool, revision: u64) -> Option<Reply<V>> {
        if let Some(kept) = self
            .kept
            .iter()
            .find(|kept| kept.key == *key && kept.read.elapsed() < KEPT_FOR)
        {
            let _ = reply.send(Ok(kept.value.clone()));
            return None;
        }

        match self.reading.iter_mut().find(|reading| reading.key == *key) {
            Some(reading) if current && reading.revision == revision => reading.waiting.push(reply),
            Some(reading) => reading.late.push(reply),
            None => return Some(reply),
        }

        None
    }

    /// Notes a read of `key` started for `waiting`.
    fn begin(&mut self, key: K, waiting: Vec<Reply<V>>, revision: u64) {
        self.reading.push(Reading {
            key,
            revision,
            waiting,
            late: Vec::new(),
        });
    }

    /// Hands out what was read, and keeps it when `keep` holds and the
    /// checkout has not changed since the read began. Gives back those who
    /// are owed a newer read.
    fn done(&mut self, key: &K, read: Result<V>, keep: bool, revision: u64) -> Vec<Reply<V>> {
        let Some(at) = self.reading.iter().position(|reading| reading.key == *key) else {
            return Vec::new();
        };
        let reading = self.reading.swap_remove(at);
        let read = read.map(Arc::new).map_err(Arc::new);

        if let Ok(value) = &read
            && keep
            && reading.revision == revision
        {
            self.kept.retain(|kept| kept.key != *key);

            if self.kept.len() == self.keep {
                self.kept.pop_front();
            }

            self.kept.push_back(Kept {
                key: key.clone(),
                value: value.clone(),
                read: Instant::now(),
            });
        }

        for reply in reading.waiting {
            let _ = reply.send(read.clone());
        }

        reading.late
    }
}

impl Checkout {
    fn new(root: PathBuf) -> Self {
        Self {
            root,
            revision: 0,
            changed: Arc::default(),
            watch: None,
            tried: None,
            diffs: Reads::new(KEPT),
            statuses: Reads::new(1),
            jobs: JoinSet::new(),
        }
    }

    async fn run(mut self, mut asks: actor::Receiver<Ask>) {
        let changed = self.changed.clone();
        self.start_watch();

        loop {
            // A change heard goes first, so no ask is answered from what it
            // made stale.
            tokio::select! {
                biased;
                () = changed.notified() => self.forget(),
                ask = asks.recv() => match ask {
                    Some((ask, _timing)) => self.asked(ask),
                    None => return,
                },
                Some(Ok(job)) = self.jobs.join_next() => self.done(job),
            }
        }
    }

    /// Answers with what is kept, joins the read under way, or starts one.
    fn asked(&mut self, ask: Ask) {
        if self.watch.is_none() && self.tried.is_some_and(|tried| tried.elapsed() >= GIT_RETRY) {
            self.start_watch();
        }

        let (current, revision) = (self.watch.is_some(), self.revision);

        match ask {
            Ask::Diff { ask, reply } => {
                // The folder is this task's, and git never takes fewer lines.
                let ask = DiffAsk {
                    root: String::new(),
                    context: work_diff::context(&ask),
                    ..ask
                };

                if let Some(reply) = self.diffs.ask(&ask, reply, current, revision) {
                    self.read_diff(ask, vec![reply]);
                }
            }
            Ask::Status { reply } => {
                // Nothing is kept without a watch, and a read for each reader
                // would only queue behind the others: they ask git themselves.
                if !current {
                    let _ = reply.send(Err(Arc::new(anyhow!("the checkout is not watched"))));
                    return;
                }

                if let Some(reply) = self.statuses.ask(&(), reply, current, revision) {
                    self.read_status(vec![reply]);
                }
            }
        }
    }

    fn read_diff(&mut self, ask: DiffAsk, waiting: Vec<Reply<WorkDiff>>) {
        let (root, asked) = (self.root.clone(), ask.clone());
        self.jobs.spawn(async move {
            let read = actor::job("checkout", "diff", spawn_blocking({
                let asked = asked.clone();
                move || work_diff::read(&root, &asked)
            }))
            .await
            .unwrap_or_else(|e| Err(anyhow!("reading the diff stopped: {e}")));

            Job::Diffed { ask: asked, read }
        });
        self.diffs.begin(ask, waiting, self.revision);
    }

    fn read_status(&mut self, waiting: Vec<Reply<GitStatus>>) {
        let root = self.root.clone();
        self.jobs.spawn(async move {
            let read = actor::job("checkout", "status", spawn_blocking(move || {
                git_status(&root).ok_or_else(|| anyhow!("{} is not a git checkout", root.display()))
            }))
            .await
            .unwrap_or_else(|e| Err(anyhow!("reading the status stopped: {e}")));

            Job::Statused(read)
        });
        self.statuses.begin((), waiting, self.revision);
    }

    fn done(&mut self, job: Job) {
        match job {
            Job::Watched(watch) => {
                self.tried = Some(Instant::now());
                self.watch = watch;
                // What was read before the watch held may have missed a change.
                self.forget();
            }
            Job::Diffed { ask, read } => {
                // A branch may be checked out in another worktree, which this
                // checkout's watch does not cover.
                let watched = self.watch.is_some() && ask.head.trim().is_empty();
                let late = self.diffs.done(&ask, read, watched, self.revision);

                if !late.is_empty() {
                    self.read_diff(ask, late);
                }
            }
            Job::Statused(read) => {
                let late = self
                    .statuses
                    .done(&(), read, self.watch.is_some(), self.revision);

                if !late.is_empty() {
                    self.read_status(late);
                }
            }
        }
    }

    /// The checkout changed: nothing kept of it holds any more.
    fn forget(&mut self) {
        self.revision += 1;
        self.diffs.kept.clear();
        self.statuses.kept.clear();
    }

    /// Starts watching the folder and its Git metadata, off this task: the
    /// first walk of a large checkout takes longer than a reader waits.
    fn start_watch(&mut self) {
        let (root, changed) = (self.root.clone(), self.changed.clone());
        self.tried = None;
        self.jobs.spawn(async move {
            let watch = async {
                let mut paths = watch_paths(&root).await?;

                if paths.is_empty() {
                    return Ok(None);
                }

                paths.push(root.clone());
                let watch = Paths::new("checkouts", move |_| changed.notify_one())?;
                watch.replace(paths).await?;
                anyhow::Ok(Some(watch))
            };

            Job::Watched(actor::job("checkout", "watch", watch).await.unwrap_or_else(|e| {
                log::debug!("a checkout is not watched: {e:#}");
                None
            }))
        });
    }
}

/// What a checkout's git reads depend on besides its working tree. Worktree
/// metadata can live outside the checkout, and the object database is left
/// out. Empty outside a repository.
pub(crate) async fn watch_paths(root: &Path) -> Result<Vec<PathBuf>> {
    let mut repository = false;

    for folder in root.ancestors() {
        if tokio::fs::symlink_metadata(folder.join(".git"))
            .await
            .is_ok()
        {
            repository = true;
            break;
        }
    }

    if !repository {
        return Ok(Vec::new());
    }

    // Read in process: no git process for each folder the events watch.
    let root = root.to_owned();
    let found = spawn_blocking(move || {
        let repo = super::found::repository(&root)?;
        Some((repo.git_dir().to_owned(), repo.common_dir().to_owned()))
    })
    .await?;
    let Some((git, common)) = found else {
        return Ok(Vec::new());
    };

    let git = tokio::fs::canonicalize(git).await?;
    let common = tokio::fs::canonicalize(common).await?;
    // Everything the Git projections and diffs read besides the working tree:
    // the branch, index, refs, upstream settings, worktrees and exclusions.
    Ok(vec![
        git.join("HEAD"),
        git.join("index"),
        git.join("commondir"),
        common.join("refs"),
        common.join("packed-refs"),
        common.join("config"),
        common.join("worktrees"),
        common.join("info").join("exclude"),
    ])
}

#[cfg(test)]
#[path = "../tests/agents_checkouts.rs"]
mod tests;
