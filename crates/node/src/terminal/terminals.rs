//! The set of terminals the node or the host keeps, and each one's
//! shared state.

use super::{OpenTerminal, lock, pty::pty_size, screen::Screen, unknown};
use anyhow::Result;
use beep_proto::node::{
    EnvSecret,
    process::{Task, TaskId},
    terminal::{MAX_TERMINALS, Reply, Request, Size},
};
use portable_pty::{ChildKiller, MasterPty};
use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::{
    sync::{Notify, mpsc, watch},
    time::timeout,
};

/// How long a hung-up terminal has to exit before it is killed.
const STOP_GRACE: Duration = Duration::from_secs(2);
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
            last: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |since| {
                    u32::try_from(since.as_secs() % 1_000_000 * 100).unwrap_or(0)
                }),
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
    pub(super) group: Option<crate::process::Group>,
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
    /// Set by a stop, so the exit reads as stopped.
    pub(super) stopping: AtomicBool,
    /// Its set's `changed`.
    pub(super) started_or_ended: Arc<Notify>,
}
impl Live {
    pub(super) fn task(&self) -> Task {
        Task {
            id: self.id,
            command: self.command.clone(),
            state: lock(&self.screen).state,
        }
    }

    pub(super) fn is_running(&self) -> bool {
        lock(&self.screen).state.is_running()
    }

    /// Says it started or ended, to the node's note and its set.
    pub(super) fn started_or_ended(&self) {
        crate::restart::changed();
        self.started_or_ended.notify_one();
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
        let listed = inner.terms.iter().map(|term| term.live.task()).collect();
        inner.terms.retain(|term| term.live.is_running());
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
            live.stopping.store(true, Ordering::Release);
            self.signal(id, false);
            if !live.wait_ended(STOP_GRACE).await {
                log::debug!("terminal {id} outlived its hang-up; killing it");
                self.signal(id, true);
                if !live.wait_ended(STOP_WAIT).await {
                    log::warn!("terminal {id} did not stop in time");
                }
            }
        }
        Reply::Stopped { task: live.task() }
    }

    /// Hangs up terminal `id`, or kills it when `hard`, while it runs: once
    /// its child is reaped the group's id may be someone else's.
    fn signal(&self, id: TaskId, hard: bool) {
        let mut inner = lock(&self.inner);
        let Some(term) = inner.terms.iter_mut().find(|term| term.live.id == id) else {
            return;
        };
        if term.live.is_running() {
            term.signal(hard);
        }
    }

    pub(super) fn running(&self) -> Vec<OpenTerminal> {
        lock(&self.inner)
            .terms
            .iter()
            .filter(|term| term.live.is_running())
            .map(|term| OpenTerminal {
                id: term.live.id.0,
                cwd: term.pid.and_then(current_dir).or_else(|| term.cwd.clone()),
                command: term.live.command.clone(),
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
            if term.live.is_running() {
                term.signal(true);
            }
        }
    }
}
impl Term {
    fn signal(&mut self, hard: bool) {
        #[cfg(unix)]
        if let Some(group) = &self.group {
            if hard {
                group.kill();
            } else {
                group.hang_up();
            }
            return;
        }
        // Windows has no gentle way; elsewhere SIGHUP to the child alone.
        let _ = hard;
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
mod tests {
    use super::*;
    use beep_proto::node::process::TaskState;

    #[cfg(unix)]
    fn started(reply: Reply) -> TaskId {
        match reply {
            Reply::Started { task } => task.id,
            other => panic!("not started: {other:?}"),
        }
    }

    /// Waits until the terminal's output so far holds `needle`.
    #[cfg(unix)]
    async fn wait_for_output(live: &Live, needle: &str) -> String {
        let mut changed = live.changed.subscribe();
        timeout(Duration::from_secs(20), async {
            loop {
                changed.borrow_and_update();
                let bytes: Vec<u8> = lock(&live.screen).lines.iter().flatten().copied().collect();
                let text = String::from_utf8_lossy(&bytes).into_owned();
                if text.contains(needle) {
                    return text;
                }
                changed.changed().await.expect("the terminal is kept");
            }
        })
        .await
        .unwrap_or_else(|_| panic!("no {needle:?} in the terminal"))
    }

    // A pseudo-terminal round trip: keys typed reach the shell, and what it
    // prints comes back.
    #[cfg(unix)]
    #[tokio::test]
    async fn typed_keys_reach_the_shell_and_its_output_comes_back() {
        let terminals = Terminals::default();
        let id = started(
            terminals
                .answer(Request::Start {
                    command: Some("read line; echo \"got:$line\"; read wait".into()),
                    cwd: None,
                    size: Size {
                        cols: 100,
                        rows: 30,
                    },
                })
                .await,
        );
        let live = terminals.live(id).expect("the terminal is kept");
        live.type_keys(b"hello\r".to_vec());
        wait_for_output(&live, "got:hello").await;
        assert!(
            live.resize(Size { cols: 50, rows: 10 }),
            "a new size applies"
        );
        assert!(
            !live.resize(Size { cols: 50, rows: 10 }),
            "the same size again is left alone, so the shell does not redraw"
        );
        let size = lock(&live.master).get_size().expect("a size");
        assert_eq!((size.cols, size.rows), (50, 10));
        let Reply::Stopped { task } = terminals.answer(Request::Stop { id }).await else {
            panic!("not stopped");
        };
        assert_eq!(task.state, TaskState::Stopped);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_terminal_ends_with_its_command() {
        let terminals = Terminals::default();
        let id = started(
            terminals
                .answer(Request::Start {
                    command: Some("echo done; exit 3".into()),
                    cwd: None,
                    size: Size::DEFAULT,
                })
                .await,
        );
        let live = terminals.live(id).expect("the terminal is kept");
        assert!(live.wait_ended(Duration::from_secs(20)).await);
        assert_eq!(live.task().state, TaskState::Exited { code: Some(3) });
        wait_for_output(&live, "done").await;
        let Reply::Terminals { terminals: listed } = terminals.answer(Request::List {}).await
        else {
            panic!("no list");
        };
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].command, "echo done; exit 3");
        // Listed with how it ended once, then gone.
        let Reply::Terminals { terminals: listed } = terminals.answer(Request::List {}).await
        else {
            panic!("no list");
        };
        assert!(listed.is_empty());
        assert!(terminals.live(id).is_none());
    }
}
