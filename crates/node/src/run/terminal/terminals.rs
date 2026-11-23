//! The set of terminals the node or the host keeps, and each one's
//! shared state.

use super::{
    OpenTerminal, lock,
    pty::pty_size,
    screen::{Phase, Screen},
    unknown,
};
use crate::run::{process::STOP_GRACE, tasks::first_id};
use anyhow::Result;
use beep_proto::node::{
    EnvSecret,
    process::{Task, TaskId},
    terminal::{MAX_TERMINALS, Reply, Request, Size},
};
use portable_pty::{ChildKiller, MasterPty};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    sync::{Notify, mpsc, watch},
    time::timeout,
};

/// How long a stop waits for the kill to land.
const STOP_WAIT: Duration = Duration::from_secs(5);

#[derive(Default)]
pub(super) struct Terminals {
    pub(super) inner: Mutex<Inner>,
    /// Woken when a terminal starts or ends.
    pub(super) changed: Arc<Notify>,
    /// What terminals get on top of this process's environment, when not
    /// the node's own: a terminal host gets the node's.
    pub(super) env: Mutex<Option<Vec<EnvSecret>>>,
}

pub(super) struct Inner {
    pub(super) last: u32,
    pub(super) terms: Vec<Term>,
}

impl Default for Inner {
    fn default() -> Self {
        Self {
            // From the clock, so a restarted node does not hand out the
            // number of a terminal that died with the old one.
            last: first_id(),
            terms: Vec::new(),
        }
    }
}

/// One terminal, as the node keeps it.
pub(super) struct Term {
    pub(super) live: Arc<Live>,
    /// Ends the child; on Unix its group is hung up instead.
    pub(super) killer: Box<dyn ChildKiller + Send + Sync>,
    /// The child's process group: it leads a session of its own.
    #[cfg(unix)]
    pub(super) group: Option<crate::run::process::Group>,
    /// The child's process id.
    pub(super) pid: Option<u32>,
    /// The folder it started in.
    pub(super) cwd: Option<PathBuf>,
}

/// What an attached stream shares with the terminal.
pub(super) struct Live {
    pub(super) id: TaskId,
    pub(super) command: String,
    pub(super) screen: Mutex<Screen>,
    /// Bumped whenever output comes or the state changes.
    pub(super) changed: watch::Sender<u64>,
    /// Typed keys, for the writer thread.
    pub(super) input: mpsc::Sender<Vec<u8>>,
    pub(super) master: Mutex<Box<dyn MasterPty + Send>>,
    #[cfg(windows)]
    pub(super) process: Option<std::os::windows::io::OwnedHandle>,
    /// Its set's `changed`.
    pub(super) started_or_ended: Arc<Notify>,
}
impl Live {
    pub(super) fn task(&self) -> Task {
        Task {
            id: self.id,
            command: self.command.clone(),
            state: lock(&self.screen).phase.state(),
        }
    }

    pub(super) fn is_running(&self) -> bool {
        lock(&self.screen).phase.state().is_running()
    }

    /// Says it started or ended, to the node's note and its set.
    pub(super) fn started_or_ended(&self) {
        crate::link::restart::changed();
        self.started_or_ended.notify_one();
        crate::events::system().changed(beep_proto::node::events::Topic::new("terminals"));
    }

    /// Sizes the terminal to `size`, unless it already is: a resize makes
    /// some shells draw their prompt again, and every attach sends one.
    /// Returns whether it changed.
    pub(super) fn resize(&self, size: Size) -> bool {
        let master = lock(&self.master);

        if master
            .get_size()
            .is_ok_and(|now| now.cols == size.cols && now.rows == size.rows)
        {
            return false;
        }

        if let Err(e) = master.resize(pty_size(size)) {
            log::debug!("could not resize terminal {}: {e:#}", self.id);
            return false;
        }

        true
    }

    /// Hands typed keys to the writer; dropped when the terminal is not
    /// reading, so a stuck program never holds up its output.
    pub(super) fn type_keys(&self, data: Vec<u8>) {
        if let Err(e) = self.input.try_send(data) {
            log::debug!("dropped input to terminal {}: {e}", self.id);
        }
    }

    async fn wait_ended(&self, limit: Duration) -> bool {
        let mut changed = self.changed.subscribe();
        timeout(limit, async {
            while self.is_running() {
                if changed.changed().await.is_err() {
                    return;
                }
            }
        })
        .await
        .is_ok()
    }
}
impl Terminals {
    pub(super) fn live(&self, id: TaskId) -> Option<Arc<Live>> {
        lock(&self.inner)
            .terms
            .iter()
            .find(|term| term.live.id == id)
            .map(|term| term.live.clone())
    }

    pub(super) async fn answer(&self, request: Request) -> Reply {
        match request {
            Request::Start { command, cwd, size } => self.start(command, cwd, size),
            Request::List {} => Reply::Terminals {
                terminals: self.list(),
            },
            Request::Stop { id } => self.stop(id).await,
            other => Reply::Error {
                message: format!("{other:?} needs an attached terminal"),
            },
        }
    }

    /// Every terminal as it stands. One that ended is listed this once,
    /// with how it ended, then dropped: a stream still attached to it holds
    /// its own handle until it has sent the end.
    fn list(&self) -> Vec<Task> {
        let mut inner = lock(&self.inner);
        let mut listed = Vec::with_capacity(inner.terms.len());
        inner.terms.retain(|term| {
            let task = term.live.task();
            let running = task.state.is_running();
            listed.push(task);
            running
        });
        listed
    }

    fn start(&self, command: Option<String>, cwd: Option<String>, size: Size) -> Reply {
        let mut inner = lock(&self.inner);

        if inner.terms.len() >= MAX_TERMINALS {
            match inner.terms.iter().position(|term| !term.live.is_running()) {
                Some(oldest) => {
                    inner.terms.remove(oldest);
                }
                None => {
                    return Reply::Error {
                        message: format!(
                            "Terminal limit reached ({MAX_TERMINALS}). Close a terminal first."
                        ),
                    };
                }
            }
        }

        inner.last = inner.last.wrapping_add(1);
        let id = TaskId(inner.last);

        match self.open(id, command.as_deref(), cwd, size, None) {
            Ok(term) => {
                let task = term.live.task();
                inner.terms.push(term);
                Reply::Started { task }
            }
            Err(e) => Reply::Error {
                message: format!("{e:#}"),
            },
        }
    }

    async fn stop(&self, id: TaskId) -> Reply {
        let Some(live) = self.live(id) else {
            return unknown(id);
        };

        if live.is_running() {
            self.signal(id, Signal::Stop);

            if !live.wait_ended(STOP_GRACE).await {
                self.signal(id, Signal::Escalate);

                if !live.wait_ended(STOP_WAIT).await {
                    log::warn!("terminal {id} did not stop in time");
                }
            }
        }
        Reply::Stopped { task: live.task() }
    }

    fn signal(&self, id: TaskId, signal: Signal) {
        let mut inner = lock(&self.inner);
        let Some(term) = inner.terms.iter_mut().find(|term| term.live.id == id) else {
            return;
        };

        term.signal(signal);
    }

    pub(super) fn running(&self) -> Vec<OpenTerminal> {
        lock(&self.inner)
            .terms
            .iter()
            .filter_map(|term| {
                let screen = lock(&term.live.screen);
                screen.phase.has_child().then(|| OpenTerminal {
                    id: term.live.id.0,
                    cwd: term.pid.and_then(current_dir).or_else(|| term.cwd.clone()),
                    command: term.live.command.clone(),
                })
            })
            .collect()
    }

    /// Opens a new shell for `open` under its id, unless one runs there.
    pub(super) fn reopen(&self, was: &OpenTerminal) -> Result<()> {
        let mut inner = lock(&self.inner);
        let id = TaskId(was.id);
        anyhow::ensure!(
            inner.terms.len() < MAX_TERMINALS,
            "{MAX_TERMINALS} terminals already run"
        );
        anyhow::ensure!(
            inner.terms.iter().all(|term| term.live.id != id),
            "terminal {id} already runs"
        );
        let cwd = was.cwd.as_ref().filter(|cwd| cwd.is_dir());
        let banner = reopened_banner(was, cwd);
        let cwd = cwd.map(|cwd| cwd.to_string_lossy().into_owned());
        let term = self.open(id, None, cwd, Size::DEFAULT, Some(&banner))?;
        // Ids handed out later stay clear of it.
        inner.last = inner.last.max(was.id);
        inner.terms.push(term);
        Ok(())
    }

    // A terminal host lets its terminals outlive it.
    #[cfg_attr(all(unix, feature = "cli"), expect(dead_code))]
    pub(super) fn kill_all(&self) {
        for term in &mut lock(&self.inner).terms {
            term.signal(Signal::Shutdown);
        }
    }
}

enum Signal {
    Stop,
    Escalate,
    Shutdown,
}

impl Term {
    /// The screen lock orders signals with reaping the child.
    fn signal(&mut self, signal: Signal) {
        let mut screen = lock(&self.live.screen);
        #[cfg(windows)]
        if let Some(process) = &self.live.process {
            use std::os::windows::io::AsRawHandle;
            use windows_sys::Win32::{
                Foundation::WAIT_OBJECT_0, System::Threading::WaitForSingleObject,
            };

            // SAFETY: This owned handle stays valid for the nonblocking check.
            if unsafe { WaitForSingleObject(process.as_raw_handle(), 0) } == WAIT_OBJECT_0 {
                return;
            }
        }

        match (&signal, screen.phase) {
            (Signal::Stop, Phase::Running) => screen.phase = Phase::Stopping,
            (Signal::Escalate, Phase::Stopping)
            | (Signal::Shutdown, Phase::Running | Phase::Stopping) => {}
            _ => return,
        }

        if matches!(signal, Signal::Escalate) {
            log::debug!("terminal {} outlived its hang-up; killing it", self.live.id);
        }

        #[cfg(unix)]
        if let Some(group) = &self.group {
            if matches!(signal, Signal::Stop) {
                group.hang_up();
            } else {
                group.kill();
            }

            return;
        }

        if let Err(e) = self.killer.kill() {
            log::debug!("could not end terminal {}: {e}", self.live.id);
        }
    }
}
/// The first line of a terminal opened again after a restart.
fn reopened_banner(open: &OpenTerminal, cwd: Option<&PathBuf>) -> String {
    let place = cwd.map_or_else(
        || "the home folder".to_owned(),
        |cwd| cwd.display().to_string(),
    );
    format!(
        "\x1b[2m[beep-node restarted: this is a new shell in {place}; it was {}]\x1b[0m\r\n",
        open.command
    )
}

/// The folder process `pid` is in, where the system says.
fn current_dir(pid: u32) -> Option<PathBuf> {
    if cfg!(target_os = "linux") {
        std::fs::read_link(format!("/proc/{pid}/cwd")).ok()
    } else {
        None
    }
}
#[cfg(test)]
#[path = "../../tests/run_terminal_terminals.rs"]
mod tests;
