//! Processes a session started, including children that changed groups.

use crate::AsyncMutex;
use beep_proto::node::agent::RunningProcess;
use beep_proto::watched;
use std::{
    collections::HashMap,
    sync::{Arc, LazyLock, Mutex},
    time::Duration,
};
use tokio::{
    sync::watch,
    time::{Instant, interval, sleep},
};
use tokio_util::task::AbortOnDropHandle;

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
/// leave the session's group, so closing the session can stop them. Each
/// idle session reads on its own, so this is slow.
const UNWATCHED: Duration = Duration::from_secs(60);

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
/// when a follower comes or the session starts working. `false` once the
/// demand can no longer change.
async fn pace<T>(
    demand: &mut watch::Receiver<usize>,
    session: &mut watch::Receiver<T>,
    works: impl Fn(&T) -> bool,
) -> bool {
    let working = watched::read_and_update(session, &works);
    let wait = if watched::read_and_update(demand, |demand| *demand) > 0 || working {
        WATCHED
    } else {
        UNWATCHED
    };

    tokio::select! {
        () = sleep(wait) => true,
        changed = demand.changed() => changed.is_ok(),
        // An ended session still keeps its watch until it is dropped.
        Ok(()) = watched::wait_for(session, &works), if !working => true,
    }
}

#[derive(Clone, PartialEq, Eq)]
pub(super) struct Process {
    pub(super) pid: u32,
    parent: u32,
    group: u32,
    /// The OS creation time, kept at its original precision to detect pid reuse.
    pub(super) birth: u64,
    started_at: u64,
    pub(super) command: Arc<str>,
}

/// The machine's processes, by pid, from the table every reader shares: it
/// is read at most once a second however many ask.
pub(super) async fn table() -> anyhow::Result<Arc<HashMap<u32, Process>>> {
    snapshot::read(false).await
}

/// The coding agents' own programs, never a command they ran.
const HARNESSES: [&str; 7] = [
    "claude", "codex", "gemini", "opencode", "pi", "flsh", "hermes",
];
/// The npm packages an agent's CLI runs from, as Node starts it.
const HARNESS_PACKAGES: [&str; 3] = [
    "@openai/codex",
    "@anthropic-ai/claude-code",
    "@google/gemini-cli",
];

impl Process {
    fn helper(&self) -> bool {
        let mut words = self.command.split_whitespace();
        let Some(program) = words.next() else {
            return true;
        };
        let name = program.rsplit(['/', '\\']).next().unwrap_or(program);
        let name = name.strip_suffix(".exe").unwrap_or(name);
        // `codex-code-mode-host` and the like are an agent's own helpers.
        let harness = HARNESSES.iter().any(|h| {
            name.strip_prefix(h)
                .is_some_and(|rest| rest.is_empty() || rest.starts_with('-'))
        });

        if harness
            || matches!(name, "chrome_crashpad_handler" | "bwrap" | "sandbox-exec")
            || name.ends_with("-acp")
        {
            return true;
        }

        let args: Vec<_> = words.collect();
        // Codex keeps its JavaScript and computer-use runtimes alive between calls.
        let program = program.replace('\\', "/");

        if name == "node_repl" && program.ends_with("/cua_node/bin/node_repl")
            || name == "node"
                && args.iter().any(|arg| {
                    arg.replace('\\', "/")
                        .ends_with("/node_modules/@oai/cua-repl/bin/cua-repl.mjs")
                })
        {
            return true;
        }

        if args.windows(2).any(|pair| pair == ["mcp", "--session"]) {
            return true;
        }
        // npm may put a shell or Node between the adapter and its CLI, and
        // Node runs the CLI itself from its package.
        matches!(name, "node" | "sh" | "bash" | "npm" | "npx")
            && args.iter().any(|arg| {
                let arg = arg.replace('\\', "/");
                ["claude-agent-acp", "codex-acp", "pi-acp"]
                    .iter()
                    .any(|adapter| arg.split('/').any(|part| part == *adapter))
                    || HARNESS_PACKAGES
                        .iter()
                        .any(|package| arg.contains(&format!("{package}/")))
            })
    }
}

pub(super) struct Processes {
    root: u32,
    known: Mutex<Observed>,
    reading: AsyncMutex<()>,
}

#[derive(Default)]
struct Observed {
    root_birth: Option<u64>,
    processes: HashMap<u32, Process>,
}

impl Processes {
    #[cfg(all(test, target_os = "linux"))]
    pub(super) fn root(&self) -> u32 {
        self.root
    }

    pub(super) fn new(root: u32) -> Arc<Self> {
        Arc::new(Self {
            root,
            known: Default::default(),
            reading: AsyncMutex::default(),
        })
    }

    /// Observe ancestry even while the desktop is closed. Remembering a child's
    /// birth time keeps it ours after its parent exits, without claiming a reused pid.
    /// The table is read every second only while the session works or a
    /// client follows `processes`.
    pub(super) fn watch(
        self: &Arc<Self>,
        mut session: watch::Receiver<super::live::Live>,
    ) -> AbortOnDropHandle<()> {
        let processes = self.clone();
        let mut demand = DEMAND.subscribe();
        AbortOnDropHandle::new(tokio::spawn(async move {
            loop {
                if let Err(e) = processes.refresh(false).await {
                    log::debug!("could not read session processes: {e:#}");
                }

                if !pace(&mut demand, &mut session, super::live::Live::works).await {
                    return;
                }
            }
        }))
    }

    async fn refresh(self: &Arc<Self>, fresh: bool) -> anyhow::Result<Vec<Process>> {
        let _reading = self.reading.lock().await;
        // A new root must not take a cached entry left by a reused pid.
        let first = crate::lock(&self.known).root_birth.is_none();
        let all = snapshot::read(fresh || first).await?;
        let mut observed = crate::lock(&self.known);
        let before = observed.processes.clone();
        if observed.root_birth.is_none() {
            observed.root_birth = all.get(&self.root).map(|p| p.birth);
        }

        let root = all
            .get(&self.root)
            .filter(|p| Some(p.birth) == observed.root_birth);
        let group_valid = !all.contains_key(&self.root) || root.is_some();
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
                if (p.group == self.root && group_valid) || known.contains_key(&p.parent) {
                    known.insert(p.pid, p.clone());
                }
            }

            if known.len() == before {
                break;
            }
        }

        if *known != before {
            crate::events::system().changed(beep_proto::node::events::Topic::new("processes"));
        }

        Ok(known
            .values()
            .filter(|p| p.pid != self.root)
            .cloned()
            .collect())
    }

    pub(super) async fn list(self: &Arc<Self>) -> anyhow::Result<Vec<RunningProcess>> {
        let processes: Vec<_> = self
            .refresh(false)
            .await?
            .into_iter()
            .filter(|p| !p.helper())
            .collect();
        let mut ports = os::ports(&processes).await;
        let mut rows: Vec<_> = processes
            .into_iter()
            .map(|p| RunningProcess {
                pid: p.pid,
                command: p.command.chars().take(240).collect(),
                started_at: p.started_at,
                ports: ports.remove(&p.pid).unwrap_or_default(),
                left_group: cfg!(unix) && p.group != self.root,
            })
            .collect();
        rows.sort_by_key(|p| (p.started_at, p.pid));
        Ok(rows)
    }

    pub(super) async fn stop(self: &Arc<Self>, pid: u32) -> anyhow::Result<()> {
        let process = self
            .refresh(true)
            .await?
            .into_iter()
            .find(|p| p.pid == pid && !p.helper())
            .ok_or_else(|| anyhow::anyhow!("the process is not running in this session"))?;
        let stopped = stop(&[process]).await;
        snapshot::invalidate().await;
        stopped
    }

    pub(super) async fn close(self: &Arc<Self>) {
        match self.refresh(true).await {
            Ok(processes) => {
                if let Err(e) = stop(&processes).await {
                    log::warn!("could not stop session processes: {e:#}");
                }
            }
            Err(e) => log::warn!("could not read closing session's processes: {e:#}"),
        }
    }
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
    let Some(process) = snapshot::read(true).await?.get(&pid).cloned() else {
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

async fn stop(processes: &[Process]) -> anyhow::Result<()> {
    if processes.is_empty() {
        return Ok(());
    }

    let processes: Arc<[Process]> = processes.into();
    let result = send(processes.clone(), false).await;
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut ticks = interval(Duration::from_millis(50));

    loop {
        let checked = processes.clone();
        let alive = tokio::task::spawn_blocking(move || checked.iter().any(os::alive)).await?;

        if !alive {
            return result;
        }

        if Instant::now() >= deadline {
            break;
        }

        ticks.tick().await;
    }

    let killed = send(processes, true).await;
    result.and(killed)
}

/// The actor holds this guard so aborting its task also kills known escaped
/// children. Normal close gives them time to handle TERM first.
pub(super) struct Cleanup(pub(super) Arc<Processes>);

impl Drop for Cleanup {
    fn drop(&mut self) {
        for p in crate::lock(&self.0.known).processes.values() {
            if p.pid != self.0.root && p.group != self.0.root {
                let _ = signal(p, true);
            }
        }
    }
}

#[cfg(test)]
#[path = "../../tests/agents_processes.rs"]
mod tests;
