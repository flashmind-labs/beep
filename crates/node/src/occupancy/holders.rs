//! Whether a session started outside Beep still runs: a Codex process
//! keeps its session's rollout open while it has the session loaded, so a
//! process holding the file is the sign, not when the file was written.

use crate::{events::exits::Exit, fs::paths};
use beep_proto::node::agent::Presence;
use futures::future::BoxFuture;
#[cfg(target_os = "linux")]
use std::collections::HashSet;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, OnceLock},
    time::{Duration, SystemTime},
};
use tokio::{
    sync::{mpsc, oneshot},
    task::spawn_blocking,
    time::Instant,
};

/// How long an answer stands while its recently written file is not written
/// again: a process that opens a session without writing to it shows within
/// it. Checks of every file due within half its time run together.
const KEPT_FOR: Duration = Duration::from_secs(10);
/// How long an answer stands for a file its holder keeps open: its writes
/// need no check, and its exit ends the answer sooner.
const HELD_KEPT_FOR: Duration = Duration::from_secs(60);
/// How long a file must go unwritten to count as quiet.
const QUIET: Duration = Duration::from_secs(10 * 60);
/// How long an answer stands for a quiet file nothing holds: a session
/// left that long is rarely opened again before it is written.
const QUIET_KEPT_FOR: Duration = Duration::from_secs(2 * 60);
/// How long a file nobody asked about stays known.
const FORGET_AFTER: Duration = Duration::from_secs(5 * 60);
/// How long after a check failed the next one may run.
const RETRY_AFTER: Duration = Duration::from_secs(5);
/// The most files known at once.
const MAX_KNOWN: usize = 1024;
/// The most asks waiting for a pass.
const MAX_WAITING: usize = 64;

/// What holds a session's file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Holding {
    /// The process with this pid has it open.
    Open(u32),
    /// No process has it open.
    Closed,
    /// The check failed, timed out or cannot run here.
    Unknown,
}

impl Holding {
    pub(crate) fn pid(self) -> Option<u32> {
        match self {
            Self::Open(pid) => Some(pid),
            Self::Closed | Self::Unknown => None,
        }
    }
}

/// Looks up which processes hold the files given, by the paths the system
/// reports; `None` when that cannot be told.
pub(super) type Scan =
    Arc<dyn Fn(Vec<PathBuf>) -> BoxFuture<'static, Option<HashMap<PathBuf, u32>>> + Send + Sync>;

/// The machine's holder checks, shared by every listing, follow and close:
/// one task answers them in passes, from what it knows while a file is not
/// written and its holder runs, and with one scan for every file a pass
/// must look up.
pub(super) struct Holders {
    scan: Scan,
    asks: OnceLock<mpsc::Sender<Ask>>,
}

struct Ask {
    files: Vec<PathBuf>,
    reply: oneshot::Sender<HashMap<PathBuf, Holding>>,
}

impl Default for Holders {
    fn default() -> Self {
        Self::with_scan(Arc::new(|real| Box::pin(open_among(real))))
    }
}

impl Holders {
    pub(super) fn with_scan(scan: Scan) -> Self {
        Self {
            scan,
            asks: OnceLock::new(),
        }
    }

    /// What holds each of `files`. Concurrent asks share one pass.
    pub(super) async fn held_open(&self, files: Vec<PathBuf>) -> HashMap<PathBuf, Holding> {
        if files.is_empty() {
            return HashMap::new();
        }

        let asks = self.asks.get_or_init(|| {
            let (asks, waiting) = mpsc::channel(MAX_WAITING);
            tokio::spawn(answer(waiting, self.scan.clone()));
            asks
        });
        let (reply, answer) = oneshot::channel();
        let unknown = |files: &[PathBuf]| {
            files
                .iter()
                .map(|file| (file.clone(), Holding::Unknown))
                .collect()
        };

        if asks
            .send(Ask {
                files: files.clone(),
                reply,
            })
            .await
            .is_err()
        {
            return unknown(&files);
        }

        answer.await.unwrap_or_else(|_| unknown(&files))
    }

    /// How many asks wait for a pass.
    #[cfg(test)]
    pub(super) fn waiting(&self) -> usize {
        self.asks
            .get()
            .map_or(0, |asks| asks.max_capacity() - asks.capacity())
    }
}

/// A file as it was when last looked at.
#[derive(Clone, PartialEq, Eq)]
struct Seen {
    /// The path the system reports for it.
    real: PathBuf,
    len: u64,
    modified: Option<SystemTime>,
}

/// What was found holding a file, and when.
struct Known {
    seen: Seen,
    holding: Holding,
    checked: Instant,
    asked: Instant,
    /// The holder's exit, which ends the answer.
    exit: Option<Arc<Exit>>,
}

impl Known {
    /// Whether the answer still stands for the file as `now` sees it.
    fn stands(&self, seen: &Seen) -> bool {
        if self.checked.elapsed() >= self.kept_for() || self.seen.real != seen.real {
            return false;
        }

        match self.holding {
            // The holder writes it; only its exit ends the answer.
            Holding::Open(_) => !self.exit.as_ref().is_some_and(|exit| exit.exited()),
            // A write means a process may have opened it.
            Holding::Closed => self.seen == *seen,
            Holding::Unknown => false,
        }
    }

    /// How long the answer stands once checked.
    fn kept_for(&self) -> Duration {
        let quiet = self
            .seen
            .modified
            .and_then(|modified| modified.elapsed().ok())
            .is_some_and(|age| age >= QUIET);

        match self.holding {
            Holding::Open(_) => HELD_KEPT_FOR,
            Holding::Closed if quiet => QUIET_KEPT_FOR,
            Holding::Closed | Holding::Unknown => KEPT_FOR,
        }
    }
}

/// Answers asks until every sender is gone. Asks that came during a pass
/// are answered together by the next.
async fn answer(mut waiting: mpsc::Receiver<Ask>, scan: Scan) {
    let mut known: HashMap<PathBuf, Known> = HashMap::new();
    let mut failed: Option<Instant> = None;

    while let Some(first) = waiting.recv().await {
        let mut asks = vec![first];

        while let Ok(ask) = waiting.try_recv() {
            asks.push(ask);
        }

        let mut files: Vec<PathBuf> = asks.iter().flat_map(|ask| ask.files.clone()).collect();
        files.sort();
        files.dedup();
        let seen = spawn_blocking(move || {
            files
                .into_iter()
                .map(|file| {
                    let seen = look(&file);
                    (file, seen)
                })
                .collect::<HashMap<_, _>>()
        })
        .await
        .unwrap_or_default();
        let answers = pass(&mut known, &mut failed, &seen, &scan).await;

        for ask in asks {
            let answer = ask
                .files
                .into_iter()
                .map(|file| {
                    let holding = answers.get(&file).copied().unwrap_or(Holding::Unknown);
                    (file, holding)
                })
                .collect();
            let _ = ask.reply.send(answer);
        }
    }
}

/// The file at `file` as the system sees it now; `None` when it cannot.
fn look(file: &PathBuf) -> Option<Seen> {
    let real = paths::canonicalize(file).ok()?;
    let meta = std::fs::metadata(&real).ok()?;
    Some(Seen {
        real,
        len: meta.len(),
        modified: meta.modified().ok(),
    })
}

/// One pass over `looked`, the files asked about as they are now: what
/// still stands is answered from `known`, the rest, with every known file
/// due soon, looked up in one scan unless one failed within `RETRY_AFTER`.
async fn pass(
    known: &mut HashMap<PathBuf, Known>,
    failed: &mut Option<Instant>,
    looked: &HashMap<PathBuf, Option<Seen>>,
    scan: &Scan,
) -> HashMap<PathBuf, Holding> {
    let now = Instant::now();
    known.retain(|_, file| now.duration_since(file.asked) < FORGET_AFTER);
    let mut answers = HashMap::new();
    let mut due: HashMap<PathBuf, Seen> = HashMap::new();

    for (file, seen) in looked {
        let Some(seen) = seen else {
            answers.insert(file.clone(), Holding::Unknown);
            continue;
        };

        match known.get_mut(file) {
            Some(old) if old.stands(seen) => {
                old.asked = now;
                answers.insert(file.clone(), old.holding);
            }
            _ => {
                due.insert(file.clone(), seen.clone());
            }
        }
    }

    if due.is_empty() {
        return answers;
    }

    if failed.is_some_and(|at| at.elapsed() < RETRY_AFTER) {
        answers.extend(due.into_keys().map(|file| (file, Holding::Unknown)));
        return answers;
    }

    // Known files due within half the time go along: one scan costs about
    // the same for one file as for many.
    for (file, old) in known.iter() {
        if !due.contains_key(file) && old.checked.elapsed() >= old.kept_for() / 2 {
            due.insert(file.clone(), old.seen.clone());
        }
    }

    let real: Vec<PathBuf> = due.values().map(|seen| seen.real.clone()).collect();
    let Some(open) = scan(real).await else {
        log::debug!("could not tell which processes hold the sessions' files");
        *failed = Some(Instant::now());

        for file in due.into_keys() {
            if looked.contains_key(&file) {
                known.remove(&file);
                answers.insert(file, Holding::Unknown);
            }
        }

        return answers;
    };
    *failed = None;
    let checked = Instant::now();

    for (file, seen) in due {
        let holding = open
            .get(&seen.real)
            .map_or(Holding::Closed, |pid| Holding::Open(*pid));
        let exit = holding.pid().and_then(|pid| match Exit::watch(pid) {
            Ok(exit) => Some(exit),
            Err(e) => {
                log::debug!("a session file's holder cannot be watched: {e}");
                None
            }
        });
        // A file only refreshed ahead keeps when it was last asked about.
        let asked = match known.get(&file) {
            Some(old) if !looked.contains_key(&file) => old.asked,
            _ => checked,
        };

        if looked.contains_key(&file) {
            answers.insert(file.clone(), holding);
        }

        if known.len() < MAX_KNOWN || known.contains_key(&file) {
            known.insert(
                file,
                Known {
                    seen,
                    holding,
                    checked,
                    asked,
                    exit,
                },
            );
        }
    }

    answers
}

/// The paths among `real` some process has open, read from `/proc` in one
/// pass.
#[cfg(target_os = "linux")]
async fn open_among(real: Vec<PathBuf>) -> Option<HashMap<PathBuf, u32>> {
    /// The most processes looked at.
    const MAX_PROCESSES: usize = 8192;
    let wanted: HashSet<PathBuf> = real.into_iter().collect();
    let scan = move || {
        let processes = std::fs::read_dir("/proc").ok()?;
        let mut open = HashMap::new();

        for process in processes
            .flatten()
            .filter(|entry| {
                entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| name.bytes().all(|b| b.is_ascii_digit()))
            })
            .take(MAX_PROCESSES)
        {
            // Another user's processes cannot be read, and do not count.
            let Ok(fds) = std::fs::read_dir(process.path().join("fd")) else {
                continue;
            };

            for fd in fds.flatten() {
                if let Ok(to) = std::fs::read_link(fd.path())
                    && wanted.contains(&to)
                    && let Some(pid) = process
                        .file_name()
                        .to_str()
                        .and_then(|pid| pid.parse().ok())
                {
                    open.entry(to).or_insert(pid);
                }
            }

            if open.len() == wanted.len() {
                break;
            }
        }

        Some(open)
    };
    spawn_blocking(scan).await.ok()?
}

/// The paths among `real` some process has open, through one `lsof` given
/// `LIMIT`.
#[cfg(all(unix, not(target_os = "linux")))]
async fn open_among(real: Vec<PathBuf>) -> Option<HashMap<PathBuf, u32>> {
    use std::process::Stdio;
    use tokio::{process::Command, time::timeout};
    /// How long `lsof` may take.
    const LIMIT: Duration = Duration::from_secs(10);
    crate::process::counted("lsof".as_ref());
    let mut lsof = Command::new("lsof");
    lsof.args(["-F", "pn", "-w", "--"])
        .args(&real)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let out = timeout(LIMIT, lsof.output()).await.ok()?.ok()?;
    // lsof exits 1 when any file is not open; what it printed still counts.
    let mut open = HashMap::new();
    let mut pid = None;

    for line in String::from_utf8_lossy(&out.stdout).lines() {
        if let Some(value) = line.strip_prefix('p') {
            pid = value.parse::<u32>().ok();
        } else if let Some(path) = line.strip_prefix('n')
            && let Some(pid) = pid
        {
            open.entry(PathBuf::from(path)).or_insert(pid);
        }
    }

    Some(open)
}

#[cfg(not(unix))]
async fn open_among(_real: Vec<PathBuf>) -> Option<HashMap<PathBuf, u32>> {
    None
}

/// What a session found in the agents' logs is doing, once what only its
/// log said is checked: `presence` as listed, `pid` the process seen
/// running it, and `held` what has its file open. A recent write alone
/// does not make a session live when nothing holds it; a failed check
/// leaves what the log said.
pub(crate) fn settle(presence: Presence, pid: Option<u32>, held: Holding) -> Presence {
    if pid.is_some() {
        return presence;
    }

    match held {
        // Quiet for a while, it may still be loaded in an app server.
        Holding::Open(_) if presence.is_live() => presence,
        Holding::Open(_) => Presence::Running,
        Holding::Closed if presence.is_live() => Presence::Stopped,
        _ => presence,
    }
}

#[cfg(test)]
#[path = "../tests/occupancy_holders.rs"]
mod tests;
