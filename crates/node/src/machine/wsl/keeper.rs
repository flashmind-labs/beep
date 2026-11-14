//! Keeps the nodes of the WSL distributions the user added running. WSL
//! shuts a distribution down soon after its last `wsl.exe` ends, services
//! and all, so each node runs as a child of this one: started again with a
//! growing pause when it exits, stopped when it is turned off or its
//! machine leaves the mesh. The distributions kept are saved in the node's
//! folder.
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
use beep_proto::watched;
use iroh::EndpointId;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{Arc, Mutex, OnceLock},
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

/// The keeper of this process's node, once it serves. A process runs one
/// node, and the machine's report, made apart from it, reads it.
static KEEPER: OnceLock<Arc<Keeper>> = OnceLock::new();

/// The keeper, once the node started it.
pub(crate) fn running() -> Option<&'static Arc<Keeper>> {
    KEEPER.get()
}

/// How a distribution's node stands, for the machine's report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Standing {
    pub(crate) problem: Option<String>,
}

impl Keeper {
    /// Starts the nodes saved in `dir`, and stops each one the mesh
    /// removes. Only the first call starts them; later ones get its keeper.
    pub(crate) fn start(dir: &Path, mesh: crate::link::from_mesh::Membership) -> Arc<Self> {
        KEEPER
            .get_or_init(|| Self::started(dir, mesh))
            .clone()
    }

    fn started(dir: &Path, mesh: crate::link::from_mesh::Membership) -> Arc<Self> {
        let keeper = Arc::new(Self {
            dir: dir.to_owned(),
            kept: Mutex::default(),
            mesh,
            watch: Mutex::default(),
        });
        let saved = match read(dir) {
            Ok(saved) => saved,
            Err(e) => {
                log::warn!("not starting the WSL machines: {e:#}");
                Vec::new()
            }
        };

        for distro in saved {
            keeper.run(distro);
        }

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

    /// Keeps `distro`'s node running from now on, starting it unless it runs.
    pub(crate) fn keep(&self, distro: &DistroName) -> Result<()> {
        let mut saved = self.saved();

        if !saved.contains(distro) {
            saved.push(distro.clone());
            write(&self.dir, &saved)?;
        }

        self.run(distro.clone());
        Ok(())
    }

    /// Stops `distro`'s node and keeps it no longer.
    pub(crate) async fn stop(&self, distro: &DistroName) -> Result<()> {
        let mut saved = self.saved();
        saved.retain(|kept| kept != distro);
        write(&self.dir, &saved)?;
        let kept = lock(&self.kept).remove(distro);

        if let Some(kept) = kept {
            kept.stop.cancel();
            let stopped = timeout(STOP_LIMIT * 2, kept.task).await;

            if stopped.is_err() {
                log::warn!("the node in {distro} did not stop in time");
            }
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

    fn saved(&self) -> Vec<DistroName> {
        lock(&self.kept).keys().cloned().collect()
    }

    /// Starts keeping `distro`'s node running, unless it is.
    fn run(&self, distro: DistroName) {
        let mut kept = lock(&self.kept);

        if kept.contains_key(&distro) {
            return;
        }

        let stop = CancellationToken::new();
        let (problem, standing) = watch::channel(None);
        let task = tokio::spawn(keep_running(distro.clone(), stop.clone(), problem));
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

/// Runs `distro`'s node until `stop`, starting it again whenever it exits,
/// after a pause that grows while it keeps failing. `problem` says why it
/// is not running.
async fn keep_running(
    distro: DistroName,
    stop: CancellationToken,
    problem: watch::Sender<Option<String>>,
) {
    let mut restarts = Restarts::default();

    loop {
        let started = Instant::now();
        let ended = match run_once(&distro, &stop, &problem).await {
            Ok(None) => return,
            Ok(Some(ended)) => ended,
            Err(e) => format!("{e:#}"),
        };
        let pause = restarts.after(started.elapsed());
        let said = format!(
            "{ended}. Starting it again in {} s.",
            pause.as_secs().max(1)
        );
        log::warn!("the node in WSL distribution {distro}: {said}");
        problem.send_replace(Some(said));

        select! {
            () = sleep(pause) => {}
            () = stop.cancelled() => return,
        }
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
