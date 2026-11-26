//! Processes a session started, including children that changed groups.

use crate::{actor, events::exits::Exit};
use futures::future::join_all;
use beep_proto::{
    SessionId,
    node::{
        agent::{RunningContainer, RunningProcess},
        events::Topic,
    },
};
use beep_proto::watched;
use std::{
    collections::{BTreeSet, HashMap},
    sync::{
        Arc, LazyLock, Mutex,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
    time::Duration,
};
use tokio::{
    sync::{oneshot, watch},
    time::{Instant, sleep},
};
use tokio_util::task::AbortOnDropHandle;

mod docker;

pub(super) use docker::Docker;
use process::Process;
mod pages;
mod process;
mod snapshot;

#[cfg(any(target_os = "linux", target_os = "android"))]
mod linux;
#[cfg(any(target_os = "macos", target_os = "ios"))]
mod macos;
#[cfg(windows)]
mod windows;
#[cfg(any(target_os = "linux", target_os = "android"))]
use linux as os;
#[cfg(any(target_os = "macos", target_os = "ios"))]
use macos as os;
#[cfg(windows)]
use windows as os;

/// Sends SIGTERM, or SIGKILL when `kill`, to `p` unless it is gone or its pid
/// now belongs to another process.
#[cfg(unix)]
fn signal(p: &Process, kill: bool) -> std::io::Result<()> {
    if !os::alive(p) {
        return Ok(());
    }

    let pid = libc::pid_t::try_from(p.pid).map_err(std::io::Error::other)?;
    // SAFETY: the positive pid and signal are plain integers. Its birth was
    // checked above, so a stale observation is not sent to a reused pid.
    if unsafe { libc::kill(pid, if kill { libc::SIGKILL } else { libc::SIGTERM }) } != 0 {
        let e = std::io::Error::last_os_error();

        if e.raw_os_error() != Some(libc::ESRCH) {
            return Err(e);
        }
    }

    Ok(())
}
#[cfg(windows)]
use windows::signal;

/// How often a session reads the process table while it works or a client
/// follows `processes`: a wrapper that daemonizes and exits between reads
/// would lose its child, and closing the session would leave it running.
const WATCHED: Duration = Duration::from_secs(1);
/// How often an idle session reads it, only to keep track of children that
/// leave the session's group, so closing the session can stop them. Idle
/// sessions share a table up to this old, so however many there are, the
/// OS is read about once per interval.
const UNWATCHED: Duration = Duration::from_secs(60);

/// How often a standalone node checks the ports its sessions' processes
/// listen on, so a server that binds after it started still shows up.
const PORTS: Duration = Duration::from_secs(60);
/// How long after a session's processes change its ports are checked on
/// every read while a client follows `processes`: a server binds a moment
/// after it starts, after the list that showed it was sent.
const STARTING: Duration = Duration::from_secs(30);
/// Set by a standalone node: an embedded one leaves the desktop's battery be.
static WATCH_PORTS: AtomicBool = AtomicBool::new(false);

/// Has every session check its ports each `PORTS`.
pub(crate) fn watch_ports() {
    WATCH_PORTS.store(true, Ordering::Relaxed);
}

/// How many topic sources follow `processes` right now.
static DEMAND: LazyLock<watch::Sender<usize>> = LazyLock::new(|| watch::channel(0).0);

/// Held while a client follows `processes`: sessions read their processes
/// every second then, and seldom otherwise.
pub(crate) struct Demand(());

impl Demand {
    pub(crate) fn new() -> Self {
        DEMAND.send_modify(|count| *count += 1);
        Self(())
    }
}

impl Drop for Demand {
    fn drop(&mut self) {
        DEMAND.send_modify(|count| *count = count.saturating_sub(1));
    }
}

/// Waits until the next read of the process table: a second while someone
/// follows `processes` or the session works, longer otherwise, and at once
/// when a follower comes or the session starts working. Returns how old a
/// shared table that read may use, the time it waited or a second when it
/// was woken; `None` once the demand can no longer change.
async fn pace<T>(
    demand: &mut watch::Receiver<usize>,
    session: &mut watch::Receiver<T>,
    works: impl Fn(&T) -> bool,
) -> Option<Duration> {
    let working = watched::read_and_update(session, &works);
    let wait = if watched::read_and_update(demand, |demand| *demand) > 0 || working {
        WATCHED
    } else {
        UNWATCHED
    };

    tokio::select! {
        () = sleep(wait) => Some(wait),
        changed = demand.changed() => changed.is_ok().then_some(WATCHED),
        Ok(()) = watched::wait_for(session, &works), if !working => Some(WATCHED),
    }
}

/// The machine's processes, by pid, from the table every reader shares: it
/// is read at most once a second however many ask.
pub(super) async fn table() -> anyhow::Result<Arc<HashMap<u32, Process>>> {
    snapshot::read(WATCHED).await
}

/// What the poll task last published: when it read the table, and its list
/// or why it could not.
#[derive(Clone)]
struct Published {
    at: Instant,
    listing: Result<Listing, String>,
}

/// How old a published list may be and still answer a reader: what a
/// followed session's polls keep it within.
const FRESH: Duration = Duration::from_secs(2);

/// What peers read of a session: its visible processes and the Docker
/// containers it started, with their ports and pages.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct Running {
    pub(super) processes: Vec<RunningProcess>,
    pub(super) containers: Vec<RunningContainer>,
}

/// A session's processes and ports, as the poll task last saw them.
#[derive(Clone, PartialEq, Eq)]
struct Listing {
    rows: Running,
    /// Every process that is the session's but not its root, helpers
    /// included: what closing the session has to stop.
    ours: Vec<Process>,
}

/// Asks the poll task to read the table now, and the ports too when `scan`.
/// It answers once the list is published.
struct Poll {
    scan: bool,
    done: oneshot::Sender<Result<(), String>>,
}

impl From<&Poll> for &'static str {
    fn from(_: &Poll) -> Self {
        "Poll"
    }
}

/// What the poll task takes over in `watch`.
struct Inbox {
    polls: actor::Receiver<Poll>,
    publish: watch::Sender<Option<Published>>,
}

/// How many `Poll`s wait for the task.
const POLLS: usize = 8;

/// A session's process list. Its poll task (`watch`) owns what it observes
/// and publishes each list; readers take the last one and scan nothing.
pub(super) struct Processes {
    /// The agent's pid, leading its group; 0 until a bridge starts its CLI.
    root: AtomicU32,
    polls: actor::Sender<Poll>,
    /// `None` until the first poll.
    published: watch::Receiver<Option<Published>>,
    inbox: Mutex<Option<Inbox>>,
    /// The agent's own Docker socket, which marks the containers it starts.
    docker: Option<Docker>,
}

/// How many times a port that has not answered is tested for a page.
const TRIES: u8 = 30;

/// What testing a listening port for a page found.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Yes,
    No,
    /// It has not answered yet, after this many tries.
    Unsettled(u8),
}

impl Page {
    fn unsettled(&self) -> bool {
        matches!(self, Self::Unsettled(_))
    }
}

/// What the poll task has seen of the session, kept by it alone.
#[derive(Default)]
struct Observed {
    root_birth: Option<u64>,
    processes: HashMap<u32, Process>,
    /// `processes` without the root.
    ours: Vec<Process>,
    /// The listening ports of the last scan, by pid.
    ports: HashMap<u32, Vec<u16>>,
    /// When `processes` last changed.
    changed_at: Option<Instant>,
    /// What testing each listening (pid, port) for a page found.
    pages: HashMap<(u32, u16), Page>,
    /// The session's containers at the last scan.
    containers: Vec<RunningContainer>,
    /// What testing each published (container, port) for a page found.
    container_pages: HashMap<(String, u16), Page>,
}

impl Processes {
    #[cfg(test)]
    pub(super) fn root(&self) -> u32 {
        self.root.load(Ordering::Relaxed)
    }

    /// The processes under `root`, the agent's pid; with none, under the
    /// CLI its bridge starts, once `set_root` names it. With `docker`, the
    /// containers the agent starts through it too.
    pub(super) fn new(root: Option<u32>, docker: Option<Docker>) -> Arc<Self> {
        let (polls, inbox) = actor::channel("processes", POLLS);
        let (publish, published) = watch::channel(None);

        Arc::new(Self {
            root: AtomicU32::new(root.unwrap_or_default()),
            polls,
            published,
            inbox: Mutex::new(Some(Inbox {
                polls: inbox,
                publish,
            })),
            docker,
        })
    }

    /// Observe ancestry even while the desktop is closed. Remembering a child's
    /// birth time keeps it ours after its parent exits, without claiming a reused pid.
    /// The table is read every second only while the session works or a
    /// client follows `processes`. The ports are scanned when a follower
    /// arrives, on each read for `STARTING` after the processes change while
    /// a client follows, and on a standalone node each `PORTS` too. Each
    /// poll publishes the list, and a change in it is signalled for this
    /// session only. The session's actor drops the handle when the session
    /// ends, so an ended session reads nothing. Call it once.
    pub(super) fn watch(
        self: &Arc<Self>,
        mut session: watch::Receiver<super::live::Live>,
    ) -> AbortOnDropHandle<()> {
        let processes = self.clone();
        let inbox = crate::lock(&self.inbox).take();
        let mut demand = DEMAND.subscribe();

        AbortOnDropHandle::new(tokio::spawn(async move {
            let Some(Inbox { mut polls, publish }) = inbox else {
                log::warn!("session processes are watched twice");
                return;
            };
            let mut observed = Observed::default();
            let mut max_age = WATCHED;
            let mut ports_due = Instant::now() + PORTS;
            let mut followers = 0;
            let mut asked: Vec<Poll> = Vec::new();
            let mut container_started = false;

            loop {
                let forced = !asked.is_empty();
                let forced_scan = asked.iter().any(|poll| poll.scan) || container_started;
                // Its server binds a moment after it starts, as a process's does.
                if std::mem::take(&mut container_started) {
                    observed.changed_at = Some(Instant::now());
                }
                let age = if forced { Duration::ZERO } else { max_age };
                let result =
                    actor::job("processes", "table", processes.refresh(&mut observed, age)).await;
                let followers_now = watched::read_sent(&DEMAND, |demand| *demand);
                let starting = observed
                    .changed_at
                    .is_some_and(|at| at.elapsed() < STARTING);
                let due = WATCH_PORTS.load(Ordering::Relaxed) && Instant::now() >= ports_due;
                let scan = forced_scan
                    || due
                    || (starting && followers_now > 0)
                    || followers_now > followers
                    || watched::read_sent(&publish, Option::is_none);
                followers = followers_now;

                let result = result.map_err(|e| format!("{e:#}"));

                if result.is_ok() && scan {
                    ports_due = Instant::now() + PORTS;
                    actor::job("processes", "ports", observed.scan(processes.docker.as_ref())).await;
                }

                processes.publish(&observed, &result, &publish, &session);

                for poll in asked.drain(..) {
                    let _ = poll.done.send(result.clone());
                }

                tokio::select! {
                    age = pace(&mut demand, &mut session, super::live::Live::works) => match age {
                        Some(age) => max_age = age,
                        None => return,
                    },
                    () = started(processes.docker.as_ref()) => container_started = true,
                    // The table read and port scan are timed as jobs, not as the poll.
                    Some((poll, _)) = polls.recv() => {
                        asked.push(poll);
                        // One read answers every poll that is waiting.
                        while let Some((poll, _)) = polls.try_recv() {
                            asked.push(poll);
                        }
                    }
                }
            }
        }))
    }

    /// Names the CLI a bridge started as the root, and has its children listed.
    pub(super) fn set_root(&self, root: u32) {
        self.root.store(root, Ordering::Relaxed);
        // Nobody waits for the answer, and a poll already queued sees the root.
        let _ = self.polls.try_send(Poll {
            scan: false,
            done: oneshot::channel().0,
        });
    }

    /// Has the poll task read the table now, and the ports too when `scan`,
    /// and waits until it published.
    pub(super) async fn poll(&self, scan: bool) -> anyhow::Result<()> {
        let (done, answer) = oneshot::channel();
        self.polls
            .send(Poll { scan, done })
            .await
            .map_err(|_| ended())?;

        answer
            .await
            .map_err(|_| ended())?
            .map_err(|e| anyhow::anyhow!(e))
    }

    /// The last published list when it is fresh, else a new one: the poll
    /// task reads the table and the ports and the reader takes that. The
    /// first read waits for the first poll. A followed session is polled
    /// every second, so its readers never ask.
    pub(super) async fn list(&self) -> anyhow::Result<Running> {
        let mut seen = self.published.clone();
        watched::wait_for(&mut seen, Option::is_some)
            .await
            .map_err(|_| ended())?;

        if watched::read(&seen, |published| {
            published.as_ref().is_none_or(|p| p.at.elapsed() >= FRESH)
        }) {
            self.poll(true).await?;
        }

        watched::read(&seen, |published| match published {
            Some(Published {
                listing: Ok(listing),
                ..
            }) => Ok(listing.rows.clone()),
            Some(Published {
                listing: Err(e), ..
            }) => Err(anyhow::anyhow!(e.clone())),
            None => Err(ended()),
        })
    }

    /// Observes the session's processes in a table at most `max_age` old.
    async fn refresh(&self, observed: &mut Observed, max_age: Duration) -> anyhow::Result<()> {
        let root_pid = self.root.load(Ordering::Relaxed);
        if root_pid == 0 {
            return Ok(());
        }

        // A new root must not take a cached entry left by a reused pid.
        let first = observed.root_birth.is_none();
        let all = snapshot::read(if first { Duration::ZERO } else { max_age }).await?;
        let before = observed.processes.clone();
        if observed.root_birth.is_none() {
            observed.root_birth = all.get(&root_pid).map(|p| p.birth);
        }

        let root = all
            .get(&root_pid)
            .filter(|p| Some(p.birth) == observed.root_birth);
        let group_valid = !all.contains_key(&root_pid) || root.is_some();
        let known = &mut observed.processes;
        known.retain(|pid, old| {
            let Some(now) = all.get(pid).filter(|p| p.birth == old.birth) else {
                return false;
            };
            *old = now.clone();
            true
        });
        if let Some(root) = root {
            known.insert(root.pid, root.clone());
        }
        loop {
            let before = known.len();

            for p in all.values() {
                if (p.group == root_pid && group_valid) || known.contains_key(&p.parent) {
                    known.insert(p.pid, p.clone());
                }
            }

            if known.len() == before {
                break;
            }
        }

        observed.ours = known
            .values()
            .filter(|p| p.pid != root_pid)
            .cloned()
            .collect();

        if observed.processes != before {
            observed.changed_at = Some(Instant::now());
        }

        Ok(())
    }

    /// Publishes the list or the failure to read it, and signals this
    /// session's followers when the list differs from the last one read.
    fn publish(
        &self,
        observed: &Observed,
        result: &Result<(), String>,
        publish: &watch::Sender<Option<Published>>,
        session: &watch::Receiver<super::live::Live>,
    ) {
        let listing = result.clone().map(|()| Listing {
            rows: Running {
                processes: observed.rows(self.root.load(Ordering::Relaxed)),
                containers: observed.containers.clone(),
            },
            ours: observed.ours.clone(),
        });
        let mut signal = false;

        publish.send_modify(|current| {
            // The first list is read, not signalled.
            signal = match (&*current, &listing) {
                (None, _) | (_, Err(_)) => false,
                (Some(Published { listing: Ok(last), .. }), Ok(next)) => last.rows != next.rows,
                (Some(Published { listing: Err(_), .. }), Ok(_)) => true,
            };
            *current = Some(Published {
                at: Instant::now(),
                listing,
            });
        });

        if signal {
            changed(&watched::read(session, |live| live.info.id.clone()));
        }
    }

    /// The session's processes in the last list.
    fn ours(&self) -> Vec<Process> {
        watched::read(&self.published, |published| match published {
            Some(Published {
                listing: Ok(listing),
                ..
            }) => listing.ours.clone(),
            _ => Vec::new(),
        })
    }

    pub(super) async fn stop(&self, pid: u32) -> anyhow::Result<()> {
        self.poll(false).await?;
        let process = self
            .ours()
            .into_iter()
            .find(|p| p.pid == pid && !p.helper())
            .ok_or_else(|| anyhow::anyhow!("the process is not running in this session"))?;
        let stopped = stop(&[process]).await;
        snapshot::invalidate().await;
        // Readers must not be left listing what ended.
        if let Err(e) = self.poll(false).await {
            log::debug!("could not read session processes after a stop: {e:#}");
        }

        stopped
    }

    /// Stops container `id` of the last list, keeping it.
    pub(super) async fn stop_container(&self, id: &str) -> anyhow::Result<()> {
        let listed = watched::read(&self.published, |published| match published {
            Some(Published {
                listing: Ok(listing),
                ..
            }) => listing.rows.containers.iter().any(|c| c.id == id),
            _ => false,
        });
        let docker = self
            .docker
            .as_ref()
            .filter(|_| listed)
            .ok_or_else(|| anyhow::anyhow!("the container is not running in this session"))?;
        docker.stop(id).await?;
        // Readers must not be left listing what stopped.
        if let Err(e) = self.poll(true).await {
            log::debug!("could not read session processes after a stop: {e:#}");
        }

        Ok(())
    }

    pub(super) async fn close(&self) {
        let containers = async {
            let Some(docker) = &self.docker else {
                return;
            };

            match docker.containers().await {
                Ok(containers) => {
                    let stops = containers.iter().map(|c| docker.stop(&c.id));
                    for result in futures::future::join_all(stops).await {
                        if let Err(e) = result {
                            log::warn!("could not stop a session's container: {e:#}");
                        }
                    }
                }
                Err(e) => log::warn!("could not list closing session's containers: {e:#}"),
            }
        };
        let processes = async {
            match self.poll(false).await {
                Ok(()) => {
                    if let Err(e) = stop(&self.ours()).await {
                        log::warn!("could not stop session processes: {e:#}");
                    }
                }
                Err(e) => log::warn!("could not read closing session's processes: {e:#}"),
            }
        };

        tokio::join!(containers, processes);
    }
}

/// Waits until one of the session's containers starts; never without Docker.
async fn started(docker: Option<&Docker>) {
    match docker {
        Some(docker) => docker.started().await,
        None => std::future::pending().await,
    }
}

fn ended() -> anyhow::Error {
    anyhow::anyhow!("the session's processes are no longer watched")
}

impl Observed {
    /// Reads the listening ports of the visible processes and the session's
    /// containers, and tests them for pages.
    async fn scan(&mut self, docker: Option<&Docker>) {
        let visible: Vec<_> = self.ours.iter().filter(|p| !p.helper()).cloned().collect();
        let containers = async {
            match docker {
                Some(docker) => docker.containers().await,
                None => Ok(Vec::new()),
            }
        };
        let (ports, containers) = tokio::join!(os::ports(&visible), containers);
        self.ports = ports;

        match containers {
            Ok(containers) => self.containers = containers,
            // The last list stays, as Docker may be restarting.
            Err(e) => log::debug!("could not list a session's containers: {e:#}"),
        }

        let processes: BTreeSet<(u32, u16)> = self
            .ports
            .iter()
            .flat_map(|(pid, ports)| ports.iter().map(|port| (*pid, *port)))
            .collect();
        let containers: BTreeSet<(String, u16)> = self
            .containers
            .iter()
            .flat_map(|c| c.ports.iter().map(|port| (c.id.clone(), *port)))
            .collect();
        tokio::join!(
            test_pages(&mut self.pages, processes),
            test_pages(&mut self.container_pages, containers)
        );

        for container in &mut self.containers {
            container.pages = container
                .ports
                .iter()
                .copied()
                .filter(|port| {
                    self.container_pages.get(&(container.id.clone(), *port)) == Some(&Page::Yes)
                })
                .collect();
        }
    }

    /// The visible processes with the ports of the last scan, oldest first.
    fn rows(&self, root: u32) -> Vec<RunningProcess> {
        let mut rows: Vec<_> = self
            .ours
            .iter()
            .filter(|p| !p.helper())
            .map(|p| {
                let ports = self.ports.get(&p.pid).cloned().unwrap_or_default();
                RunningProcess {
                    pid: p.pid,
                    // Long enough to reach the command inside an agent's shell wrapper.
                    command: p.command.chars().take(2000).collect(),
                    started_at: p.started_at,
                    pages: ports
                        .iter()
                        .copied()
                        .filter(|port| self.pages.get(&(p.pid, *port)) == Some(&Page::Yes))
                        .collect(),
                    ports,
                    left_group: cfg!(unix) && p.group != root,
                    parent: p.parent,
                }
            })
            .collect();
        rows.sort_by_key(|p| (p.started_at, p.pid));
        rows
    }

}

/// Tests the `listening` ports for pages, keeping what was found by owner
/// and port. A pair is tested until it answers, as a request to a port that
/// is not a web server may end up in its log. One that only times out, such
/// as a server whose first page compiles, is tried again on each scan up to
/// `TRIES` times.
async fn test_pages<K: Ord + Clone + std::hash::Hash>(
    pages: &mut HashMap<(K, u16), Page>,
    listening: BTreeSet<(K, u16)>,
) {
    pages.retain(|pair, _| listening.contains(pair));
    let untested: BTreeSet<_> = listening
        .iter()
        .filter(|pair| pages.get(pair).is_none_or(Page::unsettled))
        .map(|(_, port)| *port)
        .collect();
    let tested: HashMap<_, _> = pages::test(untested).await.into_iter().collect();

    for pair in &listening {
        if let Some(answer) = tested.get(&pair.1) {
            let tries = match pages.get(pair) {
                Some(Page::Unsettled(tries)) => *tries,
                _ => 0,
            };
            let page = match answer {
                Some(true) => Page::Yes,
                Some(false) => Page::No,
                None if tries + 1 >= TRIES => Page::No,
                None => Page::Unsettled(tries + 1),
            };
            pages.insert(pair.clone(), page);
        }
    }
}

/// Tells followers of `session`'s `processes` to read them again. A session
/// that has no id yet has no followers, so that one goes to all.
fn changed(session: &SessionId) {
    let topic = if session.is_empty() {
        Topic::new("processes")
    } else {
        Topic::keyed("processes", session.to_string())
    };
    crate::events::system().changed(topic);
}

/// Asks `pid`, the process of a session started outside Beep, to end, and
/// waits up to `wait` for it to go. SIGTERM on Unix, as an idle prompt that
/// wants Ctrl-C twice still ends on it. Windows cannot ask a console program
/// it shares no console with, so there it ends at once, like `stop`; an idle
/// session has written its transcript. It is never killed after `wait`.
pub(super) async fn close_outside(pid: u32, wait: Duration) -> anyhow::Result<()> {
    if pid <= 1 || pid == std::process::id() {
        anyhow::bail!("process {pid} is not a session's");
    }
    // Watched before the signal, so its exit cannot come first.
    let exit = crate::events::exits::Exit::watch(pid)?;
    let Some(process) = snapshot::read(Duration::ZERO).await?.get(&pid).cloned() else {
        return Ok(());
    };

    send(Arc::from([process]), false).await?;
    let ended = tokio::time::timeout(wait, exit.wait()).await;
    snapshot::invalidate().await;
    ended.map_err(|_| anyhow::anyhow!("it did not end; close it in its terminal"))
}

async fn send(processes: Arc<[Process]>, kill: bool) -> anyhow::Result<()> {
    tokio::task::spawn_blocking(move || {
        let mut error = None;

        for p in processes.iter() {
            if let Err(e) = signal(p, kill) {
                error.get_or_insert(e);
            }
        }

        error.map_or(Ok(()), Err)
    })
    .await??;
    Ok(())
}

/// How long `stop` waits for processes to end on SIGTERM before it kills them.
const STOP_WAIT: Duration = Duration::from_secs(3);

async fn stop(processes: &[Process]) -> anyhow::Result<()> {
    if processes.is_empty() {
        return Ok(());
    }

    let processes: Arc<[Process]> = processes.into();
    // Watched before the signal, so an exit cannot come first.
    let exits = processes
        .iter()
        .map(|p| Exit::watch(p.pid))
        .collect::<anyhow::Result<Vec<_>>>()?;
    let result = send(processes.clone(), false).await;

    if tokio::time::timeout(STOP_WAIT, join_all(exits.iter().map(|exit| exit.wait())))
        .await
        .is_ok()
    {
        return result;
    }

    let killed = send(processes, true).await;
    result.and(killed)
}

/// The actor holds this guard so aborting its task also kills known escaped
/// children. Normal close gives them time to handle TERM first.
pub(super) struct Cleanup(pub(super) Arc<Processes>);

impl Drop for Cleanup {
    fn drop(&mut self) {
        let root = self.0.root.load(Ordering::Relaxed);

        for p in self.0.ours() {
            if p.group != root {
                let _ = signal(&p, true);
            }
        }
    }
}

#[cfg(test)]
#[path = "../../tests/agents_processes.rs"]
mod tests;
