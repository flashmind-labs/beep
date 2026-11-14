//! Keeps the nodes of the WSL distributions the user added running. WSL
//! shuts a distribution down soon after its last `wsl.exe` ends, services
//! and all, so each node runs as a child of this one: started again with a
//! growing pause when it exits, stopped when it is turned off or its
//! machine leaves the mesh. The distributions kept are saved in the node's
//! folder; one removed from WSL is dropped. When WSL stops a distribution,
//! as `wsl --shutdown` does, its node waits until it is turned on again or
//! this node starts again. A node another process already runs, such as a
//! beep-node service beside the desktop app, is left to it.
//!
//! A child stops gently when its stdin closes (`--with-parent` inside).
//! Each runs in a job that ends with this process, so a node that exits
//! any way at all takes them with it.

use super::{distro::DistroName, steps};
use crate::{
    lock,
    run::process::{self, GroupLeader},
};
use anyhow::{Context, Result};
use futures::future::join_all;
use beep_proto::watched;
use iroh::EndpointId;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::ChildStderr,
    select,
    sync::watch,
    time::{Instant, sleep, timeout},
};
use tokio_util::{sync::CancellationToken, task::AbortOnDropHandle};

/// The file in the node's folder that names the distributions kept.
const SAVED: &str = "wsl.toml";
/// How long a stopping node has to exit after its stdin closed; it gives
/// itself 10 seconds.
const STOP_LIMIT: Duration = Duration::from_secs(15);
/// How long a node must run before it counts as started well: its problem
/// clears and the pause before the next start begins again.
const SETTLED: Duration = Duration::from_secs(60);
/// How often the mesh is checked for machines removed from it.
const MEMBERSHIP_CHECK: Duration = Duration::from_secs(60);
/// The last lines of a node's output kept to say why it exited.
const TAIL_LINES: usize = 20;

/// The nodes of the WSL distributions kept running.
pub(crate) struct Keeper {
    /// The node's folder, which holds `SAVED`.
    dir: PathBuf,
    /// The node serves as a machine, so it runs the WSL machines.
    active: AtomicBool,
    kept: Mutex<BTreeMap<DistroName, Kept>>,
    /// The mesh this node belongs to, to stop the nodes removed from it.
    mesh: crate::link::from_mesh::Membership,
    /// Watches the mesh; ends with the keeper.
    watch: Mutex<Option<AbortOnDropHandle<()>>>,
}

/// One distribution's node kept running.
struct Kept {
    stop: CancellationToken,
    /// Why it is not running, while it is not.
    problem: watch::Receiver<Option<String>>,
    /// Its id once known, and whether the mesh has had it as a member.
    machine: Option<(EndpointId, bool)>,
    task: AbortOnDropHandle<()>,
}

/// The keeper of this process's node, once it served as a machine. A
/// process runs one node, and the machine's report, made apart from it,
/// reads it.
static KEEPER: OnceLock<Arc<Keeper>> = OnceLock::new();

/// The keeper, while the node serves as a machine.
pub(crate) fn running() -> Option<&'static Arc<Keeper>> {
    KEEPER
        .get()
        .filter(|keeper| keeper.active.load(Ordering::SeqCst))
}

/// How a distribution's node stands, for the machine's report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Standing {
    pub(crate) problem: Option<String>,
}

/// Why a node is no longer started again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ended {
    /// It was turned off, or the keeper halted.
    Stopped,
    /// WSL stopped its distribution, as `wsl --shutdown` does: the user
    /// stopped WSL, so it waits to be turned on, or for the next start.
    Paused,
    /// Its distribution was removed.
    Gone,
}

impl Keeper {
    /// Starts the nodes saved in `dir`, and from then on stops each one the
    /// mesh removes. A keeper halted before starts them again.
    pub(crate) fn start(dir: &Path, mesh: crate::link::from_mesh::Membership) -> Arc<Self> {
        let keeper = KEEPER.get_or_init(|| Self::new(dir, mesh)).clone();
        keeper.active.store(true, Ordering::SeqCst);
        let saved = match read(&keeper.dir) {
            Ok(saved) => saved,
            Err(e) => {
                log::warn!("not starting the WSL machines: {e:#}");
                Vec::new()
            }
        };

        for distro in saved {
            keeper.run(distro);
        }

        keeper
    }

    fn new(dir: &Path, mesh: crate::link::from_mesh::Membership) -> Arc<Self> {
        let keeper = Arc::new(Self {
            dir: dir.to_owned(),
            active: AtomicBool::new(false),
            kept: Mutex::default(),
            mesh,
            watch: Mutex::default(),
        });
        let weak = Arc::downgrade(&keeper);
        *lock(&keeper.watch) = Some(AbortOnDropHandle::new(tokio::spawn(async move {
            loop {
                sleep(MEMBERSHIP_CHECK).await;
                let Some(keeper) = weak.upgrade() else {
                    return;
                };
                keeper.stop_removed().await;
            }
        })));
        keeper
    }

    /// Stops every node, keeping the ones saved for the next start, as
    /// while the node does not serve as a machine.
    pub(crate) async fn halt(&self) {
        self.active.store(false, Ordering::SeqCst);
        let kept = std::mem::take(&mut *lock(&self.kept));
        let stops = kept
            .into_iter()
            .map(|(distro, kept)| async move { finish(&distro, kept).await });
        join_all(stops).await;
    }

    /// Keeps `distro`'s node running from now on, starting it unless it
    /// runs, such as after it was turned off or WSL stopped.
    pub(crate) fn keep(self: &Arc<Self>, distro: &DistroName) -> Result<()> {
        let mut saved = read(&self.dir)?;

        if !saved.contains(distro) {
            saved.push(distro.clone());
            write(&self.dir, &saved)?;
        }

        self.run(distro.clone());
        Ok(())
    }

    /// Stops `distro`'s node and keeps it no longer.
    pub(crate) async fn stop(&self, distro: &DistroName) -> Result<()> {
        self.unsave(distro)?;
        let kept = lock(&self.kept).remove(distro);

        if let Some(kept) = kept {
            finish(distro, kept).await;
        }

        Ok(())
    }

    /// Notes that `distro`'s node has id `machine`, to stop it once the mesh
    /// removes it.
    pub(crate) fn know(&self, distro: &DistroName, machine: EndpointId) {
        if let Some(kept) = lock(&self.kept).get_mut(distro)
            && kept.machine.is_none_or(|(known, _)| known != machine)
        {
            kept.machine = Some((machine, false));
        }
    }

    /// How `distro`'s node stands, `None` when it is not kept.
    pub(crate) fn standing(&self, distro: &DistroName) -> Option<Standing> {
        lock(&self.kept).get(distro).map(|kept| Standing {
            problem: watched::read(&kept.problem, Clone::clone),
        })
    }

    /// The distributions kept.
    pub(crate) fn kept(&self) -> Vec<DistroName> {
        lock(&self.kept).keys().cloned().collect()
    }

    fn unsave(&self, distro: &DistroName) -> Result<()> {
        let mut saved = read(&self.dir)?;
        saved.retain(|kept| kept != distro);
        write(&self.dir, &saved)
    }

    /// Starts keeping `distro`'s node running, unless that runs.
    fn run(self: &Arc<Self>, distro: DistroName) {
        let mut kept = lock(&self.kept);

        if kept.get(&distro).is_some_and(|kept| !kept.task.is_finished()) {
            return;
        }

        let stop = CancellationToken::new();
        let (problem, standing) = watch::channel(None);
        let keeper = Arc::downgrade(self);
        let (running, cancel) = (distro.clone(), stop.clone());
        let task = tokio::spawn(async move {
            let ended = keep_running(&running, &cancel, &problem).await;

            if ended == Ended::Gone
                && let Some(keeper) = keeper.upgrade()
            {
                keeper.forget(&running);
            }
        });
        log::info!("keeping the node in WSL distribution {distro} running");
        kept.insert(
            distro,
            Kept {
                stop,
                problem: standing,
                machine: None,
                task: AbortOnDropHandle::new(task),
            },
        );
    }

    /// Keeps a removed distribution no longer. Called last by its own task.
    fn forget(&self, distro: &DistroName) {
        log::info!("WSL distribution {distro} was removed: no longer keeping its node");

        if let Err(e) = self.unsave(distro) {
            log::warn!("could not forget WSL distribution {distro}: {e:#}");
        }

        let kept = lock(&self.kept).remove(distro);
        drop(kept);
    }

    /// Stops the nodes whose machines the mesh had and no longer has.
    async fn stop_removed(&self) {
        let Some(mesh) = self.mesh.get() else {
            return;
        };
        let removed: Vec<DistroName> = {
            let mut kept = lock(&self.kept);
            kept.iter_mut()
                .filter_map(|(distro, kept)| {
                    let (id, seen) = kept.machine.as_mut()?;
                    let member = mesh.is_member(*id);
                    let gone = *seen && !member;
                    *seen |= member;
                    gone.then(|| distro.clone())
                })
                .collect()
        };

        for distro in removed {
            log::info!("{distro} left the mesh: no longer running its node");

            if let Err(e) = self.stop(&distro).await {
                log::warn!("could not forget WSL distribution {distro}: {e:#}");
            }
        }
    }
}

/// Stops a kept node and waits for it.
async fn finish(distro: &DistroName, kept: Kept) {
    kept.stop.cancel();

    if timeout(STOP_LIMIT * 2, kept.task).await.is_err() {
        log::warn!("the node in {distro} did not stop in time");
    }
}

/// Runs `distro`'s node until `stop`, starting it again whenever it exits,
/// after a pause that grows while it keeps failing, unless WSL stopped or
/// removed the distribution. It waits while another process runs that node.
/// `problem` says why it is not running.
async fn keep_running(
    distro: &DistroName,
    stop: &CancellationToken,
    problem: &watch::Sender<Option<String>>,
) -> Ended {
    let mut restarts = Restarts::default();

    loop {
        let started = Instant::now();
        let ended = if steps::node_running(distro).await {
            "Its node already runs, started by another Beep on this computer".to_owned()
        } else {
            let ran = match run_once(distro, stop, problem).await {
                Ok(None) => return Ended::Stopped,
                Ok(Some(ended)) => ended,
                Err(e) => format!("{e:#}"),
            };

            if let Some(ended) = after_exit(distro, problem).await {
                return ended;
            }

            ran
        };
        let pause = restarts.after(started.elapsed());
        let said = format!(
            "{ended}. Trying again in {} s.",
            pause.as_secs().max(1)
        );
        log::warn!("the node in WSL distribution {distro}: {said}");
        problem.send_replace(Some(said));

        select! {
            () = sleep(pause) => {}
            () = stop.cancelled() => return Ended::Stopped,
        }
    }
}

/// Whether WSL stopped or removed `distro`, after its node exited; `None`
/// when it still runs and the node starts again.
async fn after_exit(
    distro: &DistroName,
    problem: &watch::Sender<Option<String>>,
) -> Option<Ended> {
    let listed = match steps::list().await {
        Ok(listed) => listed,
        Err(e) => {
            log::debug!("could not list the WSL distributions: {e:#}");
            return None;
        }
    };

    match listed.iter().find(|listed| listed.name == *distro) {
        None => {
            problem.send_replace(Some(format!("There is no WSL distribution {distro} any more.")));
            Some(Ended::Gone)
        }
        Some(listed) if !listed.running => {
            log::info!("WSL stopped {distro}: its node waits to be turned on");
            problem.send_replace(Some(format!(
                "WSL stopped {distro}. Turn it on to start its node again."
            )));
            Some(Ended::Paused)
        }
        Some(_) => None,
    }
}

/// Runs the node once: `None` when it was stopped, else how it ended.
async fn run_once(
    distro: &DistroName,
    stop: &CancellationToken,
    problem: &watch::Sender<Option<String>>,
) -> Result<Option<String>> {
    let mut command = steps::run_command(distro);
    process::own_group(&mut command)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let child = command.spawn().context("could not start wsl.exe")?;
    let mut leader = GroupLeader::new(child);
    let stdin = leader.child_mut().stdin.take();
    let tail = Arc::new(Mutex::new(VecDeque::new()));
    let _reading = leader
        .child_mut()
        .stderr
        .take()
        .map(|stderr| AbortOnDropHandle::new(tokio::spawn(keep_tail(stderr, tail.clone()))));
    let settled = sleep(SETTLED);
    tokio::pin!(settled);
    let mut was_settled = false;

    loop {
        select! {
            exited = leader.child_mut().wait() => {
                let status = exited.context("could not wait for wsl.exe")?;
                let last = lock(&tail).back().cloned();
                return Ok(Some(match last {
                    Some(last) => format!("beep-node in {distro} exited ({status}): {last}"),
                    None => format!("beep-node in {distro} exited ({status})"),
                }));
            }
            () = &mut settled, if !was_settled => {
                was_settled = true;
                problem.send_replace(None);
            }
            () = stop.cancelled() => break,
        }
    }

    // Closing stdin asks it to stop; the job ends what is left.
    drop(stdin);

    if timeout(STOP_LIMIT, leader.child_mut().wait()).await.is_err() {
        log::warn!("the node in {distro} did not stop in time; ending it");
    }

    leader.stop(Duration::ZERO).await;
    Ok(None)
}

/// Keeps the last `TAIL_LINES` lines of the node's log.
async fn keep_tail(stderr: ChildStderr, tail: Arc<Mutex<VecDeque<String>>>) {
    let mut lines = BufReader::new(stderr).lines();

    while let Ok(Some(line)) = lines.next_line().await {
        let mut tail = lock(&tail);
        if tail.len() == TAIL_LINES {
            tail.pop_front();
        }
        tail.push_back(line);
    }
}

/// The pause before a node is started again: from `FIRST` it doubles with
/// each exit up to `LAST`, and starts over after a run of `SETTLED`.
#[derive(Debug)]
pub(crate) struct Restarts {
    next: Duration,
}

impl Restarts {
    const FIRST: Duration = Duration::from_secs(2);
    const LAST: Duration = Duration::from_secs(5 * 60);

    /// The pause after a run that lasted `ran`.
    pub(crate) fn after(&mut self, ran: Duration) -> Duration {
        if ran >= SETTLED {
            self.next = Self::FIRST;
        }

        let pause = self.next;
        self.next = (self.next * 2).min(Self::LAST);
        pause
    }
}

impl Default for Restarts {
    fn default() -> Self {
        Self { next: Self::FIRST }
    }
}

/// What `SAVED` holds.
#[derive(Debug, Default, Serialize, Deserialize)]
struct Saved {
    /// The distributions whose nodes are kept running.
    #[serde(default)]
    keep: BTreeSet<DistroName>,
}

/// The distributions saved in `dir`; none when nothing was saved.
pub(crate) fn read(dir: &Path) -> Result<Vec<DistroName>> {
    let path = dir.join(SAVED);
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
    };
    let saved: Saved =
        toml::from_str(&text).with_context(|| format!("reading {}", path.display()))?;
    Ok(saved.keep.into_iter().collect())
}

/// Saves `distros` in `dir` as the ones kept.
pub(crate) fn write(dir: &Path, distros: &[DistroName]) -> Result<()> {
    let path = dir.join(SAVED);
    let saved = Saved {
        keep: distros.iter().cloned().collect(),
    };
    fs::write(&path, toml::to_string(&saved)?)
        .with_context(|| format!("writing {}", path.display()))
}

#[cfg(test)]
#[path = "../../tests/machine_wsl_keeper.rs"]
mod tests;
