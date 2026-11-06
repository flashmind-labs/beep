//! Processes a session started, including children that changed groups.

use crate::AsyncMutex;
use beep_proto::node::agent::RunningProcess;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::time::{Instant, interval};
use tokio_util::task::AbortOnDropHandle;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;
#[cfg(target_os = "linux")]
use linux as os;
#[cfg(target_os = "macos")]
use macos as os;
#[cfg(windows)]
use windows as os;

#[derive(Clone)]
struct Process {
    pid: u32,
    parent: u32,
    group: u32,
    /// The OS creation time, kept at its original precision to detect pid reuse.
    birth: u64,
    started_at: u64,
    command: String,
}

/// The coding agents' own programs, never a command they ran.
const HARNESSES: [&str; 6] = ["claude", "codex", "gemini", "opencode", "pi", "flsh"];
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
    pub(super) fn watch(self: &Arc<Self>) -> AbortOnDropHandle<()> {
        let processes = self.clone();
        AbortOnDropHandle::new(tokio::spawn(async move {
            let mut ticks = interval(Duration::from_secs(1));
            ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                ticks.tick().await;
                if let Err(e) = processes.refresh().await {
                    log::debug!("could not read session processes: {e:#}");
                }
            }
        }))
    }

    async fn refresh(self: &Arc<Self>) -> anyhow::Result<Vec<Process>> {
        let _reading = self.reading.lock().await;
        let all = os::snapshot().await?;
        let mut observed = crate::lock(&self.known);
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
        Ok(known
            .values()
            .filter(|p| p.pid != self.root)
            .cloned()
            .collect())
    }

    pub(super) async fn list(self: &Arc<Self>) -> anyhow::Result<Vec<RunningProcess>> {
        let processes: Vec<_> = self
            .refresh()
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
            .refresh()
            .await?
            .into_iter()
            .find(|p| p.pid == pid && !p.helper())
            .ok_or_else(|| anyhow::anyhow!("the process is not running in this session"))?;
        stop(&[process]).await
    }

    pub(super) async fn close(self: &Arc<Self>) {
        match self.refresh().await {
            Ok(processes) => {
                if let Err(e) = stop(&processes).await {
                    log::warn!("could not stop session processes: {e:#}");
                }
            }
            Err(e) => log::warn!("could not read closing session's processes: {e:#}"),
        }
    }
}

async fn send(processes: Arc<[Process]>, kill: bool) -> anyhow::Result<()> {
    tokio::task::spawn_blocking(move || {
        let mut error = None;
        for p in processes.iter() {
            if let Err(e) = os::signal(p, kill) {
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
                let _ = os::signal(p, true);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adapters_and_harness_helpers_are_hidden_but_commands_are_not() {
        let process = |command: &str| Process {
            pid: 1,
            parent: 0,
            group: 1,
            birth: 1,
            started_at: 1,
            command: command.into(),
        };
        for command in [
            "node /srv/tools/node_modules/@agentclientprotocol/claude-agent-acp/dist/index.js",
            "codex app-server",
            "codex.exe app-server",
            "sh -c codex-acp",
            "beep-node --dir /srv/node mcp --session s1",
            "bwrap --unshare-pid",
            "/usr/bin/node /srv/npx/node_modules/@openai/codex/bin/codex.js app-server",
            "/srv/npx/node_modules/@openai/codex-linux-x64/vendor/x86_64-unknown-linux-musl/bin/codex-code-mode-host",
        ] {
            assert!(process(command).helper(), "{command}");
        }
        for command in [
            "npm run dev",
            "node server.js",
            "tsc --watch",
            "echo codex",
            "sh -c 'echo codex'",
            "pip install x",
            "node /srv/app/codex-report.js",
        ] {
            assert!(!process(command).helper(), "{command}");
        }
    }
}
