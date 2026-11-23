//! One task per checkout keeps what git says of it, so the sessions and
//! peers that read a folder share one git run for each change to it.

use super::work_diff;
use crate::events::paths::Paths;
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
    sync::{Notify, mpsc, oneshot},
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

/// A diff as its readers share it.
type Read = Result<Arc<WorkDiff>, Arc<anyhow::Error>>;

/// The node's checkouts. The task that owns them starts with the first read
/// and ends with this.
#[derive(Default)]
pub(crate) struct Checkouts {
    started: OnceLock<Started>,
}

struct Started {
    opens: mpsc::Sender<Open>,
    _task: AbortOnDropHandle<()>,
}

/// Asks for the task of the checkout at `root`, started when it has none.
struct Open {
    root: PathBuf,
    reply: oneshot::Sender<mpsc::Sender<Ask>>,
}

/// Asks one checkout for a diff.
struct Ask {
    ask: DiffAsk,
    reply: oneshot::Sender<Read>,
}

impl Checkouts {
    /// The diff `ask` names in `root`, a real folder in reach: the kept one
    /// while the checkout has not changed since it was read.
    pub(crate) async fn diff(&self, root: PathBuf, ask: DiffAsk) -> Result<Arc<WorkDiff>> {
        let (reply, read) = oneshot::channel();
        self.open(root)
            .await?
            .send(Ask { ask, reply })
            .await
            .map_err(|_| anyhow!(STOPPED))?;

        read.await
            .map_err(|_| anyhow!(STOPPED))?
            .map_err(|e| anyhow!("{e:#}"))
    }

    /// The lines added and removed in the checkout at `folder`, as the diff
    /// viewer counts them against the default branch with the working tree.
    /// `None` outside a git checkout.
    pub(crate) async fn changes(&self, folder: PathBuf) -> Option<LineChanges> {
        let root = spawn_blocking(move || super::reach::real(&folder))
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

    async fn open(&self, root: PathBuf) -> Result<mpsc::Sender<Ask>> {
        let started = self.started.get_or_init(|| {
            let (opens, opened) = mpsc::channel(WAITING);
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
    asks: mpsc::Sender<Ask>,
    /// Opened since the last sweep.
    used: bool,
}

/// Owns the checkouts' tasks: hands each reader the one for its folder and
/// ends those nobody reads.
async fn supervise(mut opens: mpsc::Receiver<Open>) {
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
                let Some(Open { root, reply }) = open else {
                    return;
                };
                let asks = match held.get_mut(&root) {
                    Some(held) if !held.asks.is_closed() => {
                        held.used = true;
                        held.asks.clone()
                    }
                    _ => {
                        let (asks, asked) = mpsc::channel(WAITING);
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
    kept: VecDeque<Kept>,
    reading: Vec<Reading>,
    jobs: JoinSet<Job>,
}

struct Kept {
    ask: DiffAsk,
    diff: Arc<WorkDiff>,
    read: Instant,
}

/// A diff git is working on, and who waits for it.
struct Reading {
    ask: DiffAsk,
    revision: u64,
    waiting: Vec<oneshot::Sender<Read>>,
    /// Those who asked after the checkout changed again: this read may have
    /// missed the change, so they get the next one.
    late: Vec<oneshot::Sender<Read>>,
}

enum Job {
    Watched(Option<Paths>),
    Diffed { ask: DiffAsk, read: Result<WorkDiff> },
}

impl Checkout {
    fn new(root: PathBuf) -> Self {
        Self {
            root,
            revision: 0,
            changed: Arc::default(),
            watch: None,
            tried: None,
            kept: VecDeque::new(),
            reading: Vec::new(),
            jobs: JoinSet::new(),
        }
    }

    async fn run(mut self, mut asks: mpsc::Receiver<Ask>) {
        let changed = self.changed.clone();
        self.start_watch();

        loop {
            tokio::select! {
                ask = asks.recv() => match ask {
                    Some(ask) => self.asked(ask),
                    None => return,
                },
                () = changed.notified() => self.forget(),
                Some(Ok(job)) = self.jobs.join_next() => self.done(job),
            }
        }
    }

    /// Answers with the kept diff, joins the read of it under way, or
    /// starts one.
    fn asked(&mut self, Ask { ask, reply }: Ask) {
        // The folder is this task's, and git never takes fewer lines.
        let ask = DiffAsk {
            root: String::new(),
            context: work_diff::context(&ask),
            ..ask
        };

        if let Some(kept) = self
            .kept
            .iter()
            .find(|kept| kept.ask == ask && kept.read.elapsed() < KEPT_FOR)
        {
            let _ = reply.send(Ok(kept.diff.clone()));
            return;
        }

        if self.watch.is_none() && self.tried.is_some_and(|tried| tried.elapsed() >= GIT_RETRY) {
            self.start_watch();
        }

        let current = self.watch.is_some();
        let revision = self.revision;

        match self.reading.iter_mut().find(|reading| reading.ask == ask) {
            Some(reading) if current && reading.revision == revision => reading.waiting.push(reply),
            Some(reading) => reading.late.push(reply),
            None => self.read(ask, vec![reply]),
        }
    }

    fn read(&mut self, ask: DiffAsk, waiting: Vec<oneshot::Sender<Read>>) {
        let (root, asked) = (self.root.clone(), ask.clone());
        self.jobs.spawn(async move {
            let read = spawn_blocking({
                let asked = asked.clone();
                move || work_diff::read(&root, &asked)
            })
            .await
            .unwrap_or_else(|e| Err(anyhow!("reading the diff stopped: {e}")));

            Job::Diffed { ask: asked, read }
        });
        self.reading.push(Reading {
            ask,
            revision: self.revision,
            waiting,
            late: Vec::new(),
        });
    }

    fn done(&mut self, job: Job) {
        let (ask, read) = match job {
            Job::Watched(watch) => {
                self.tried = Some(Instant::now());
                self.watch = watch;
                // What was read before the watch held may have missed a change.
                self.forget();
                return;
            }
            Job::Diffed { ask, read } => (ask, read.map(Arc::new).map_err(Arc::new)),
        };
        let Some(at) = self.reading.iter().position(|reading| reading.ask == ask) else {
            return;
        };
        let reading = self.reading.swap_remove(at);
        // A branch may be checked out in another worktree, which this
        // checkout's watch does not cover.
        let watched = self.watch.is_some() && ask.head.trim().is_empty();

        if let Ok(diff) = &read
            && watched
            && reading.revision == self.revision
        {
            self.kept.retain(|kept| kept.ask != ask);

            if self.kept.len() == KEPT {
                self.kept.pop_front();
            }

            self.kept.push_back(Kept {
                ask: ask.clone(),
                diff: diff.clone(),
                read: Instant::now(),
            });
        }

        for reply in reading.waiting {
            let _ = reply.send(read.clone());
        }

        if !reading.late.is_empty() {
            self.read(ask, reading.late);
        }
    }

    /// The checkout changed: nothing kept of it holds any more.
    fn forget(&mut self) {
        self.revision += 1;
        self.kept.clear();
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
                let watch = Paths::new(move |_| changed.notify_one())?;
                watch.replace(paths).await?;
                anyhow::Ok(Some(watch))
            };

            Job::Watched(watch.await.unwrap_or_else(|e| {
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
