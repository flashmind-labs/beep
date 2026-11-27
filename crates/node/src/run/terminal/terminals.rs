//! The set of terminals the node or the host keeps, and each one's
//! shared state.

use super::{
    pty::{Live, Opener, Term},
    screen::Phase,
};
use crate::lock;
use crate::{actor, process::STOP_GRACE, run::tasks::first_id};
use anyhow::Result;
use beep_proto::node::{
    process::{Task, TaskId},
    terminal::{MAX_TERMINALS, Reply, Request, Size},
};
use beep_proto::watched;
use serde::{Deserialize, Serialize};
use std::{
    future,
    path::PathBuf,
    sync::{
        Arc, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::{
    sync::{oneshot, watch},
    task::JoinSet,
    time::{Instant, sleep_until},
};
use tokio_util::task::AbortOnDropHandle;

/// How long a stop waits for the kill to land.
const STOP_WAIT: Duration = Duration::from_secs(5);
/// How long an ended terminal stays in the list, so every reader sees how
/// it ended before it goes.
const KEEP_ENDED: Duration = Duration::from_secs(30);
/// Commands waiting for the actor.
const MAILBOX: usize = 16;

/// A terminal open when the node stopped, to open again as a new shell
/// under the same id when it starts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct OpenTerminal {
    pub(super) id: u32,
    /// Where its shell was last, else where it started.
    pub(super) cwd: Option<PathBuf>,
    /// What it ran.
    pub(super) command: String,
}

/// The node's terminals. An actor owns the set and is its only writer; the
/// set is published as `list`, and everything that only looks reads that.
/// Starting and reopening are commands. The actor starts with the first one,
/// as a set can be made outside a runtime.
pub(super) struct Terminals {
    /// Written by the actor only, so reading the list changes nothing.
    list: Arc<watch::Sender<Vec<Arc<Term>>>>,
    opener: Arc<Opener>,
    actor: OnceLock<Started>,
}

struct Started {
    commands: actor::Sender<Command>,
    _task: AbortOnDropHandle<()>,
}

#[derive(strum::IntoStaticStr)]
enum Command {
    Start {
        command: Option<String>,
        cwd: Option<String>,
        size: Size,
        reply: oneshot::Sender<Reply>,
    },
    Reopen {
        was: OpenTerminal,
        reply: oneshot::Sender<Result<()>>,
    },
}

impl Default for Terminals {
    fn default() -> Self {
        Self {
            list: Arc::new(watch::channel(Vec::new()).0),
            opener: Arc::new(Opener {
                env: watch::channel(None).0,
                changed: Arc::default(),
                killed: AtomicBool::new(false),
            }),
            actor: OnceLock::new(),
        }
    }
}

impl Terminals {
    pub(super) fn live(&self, id: TaskId) -> Option<Arc<Live>> {
        self.term(id).map(|term| term.live.clone())
    }

    pub(super) async fn answer(&self, request: Request) -> Reply {
        match request {
            Request::Start { command, cwd, size } => self.start(command, cwd, size).await,
            Request::List {} => Reply::Terminals {
                terminals: self.list(),
            },
            Request::Stop { id } => self.stop(id).await,
            other => Reply::Error {
                message: format!("{other:?} needs an attached terminal"),
            },
        }
    }

    /// Every terminal as it stands, those that ended recently with how they
    /// ended. Reading changes nothing: the actor drops them in time.
    fn list(&self) -> Vec<Task> {
        watched::read_sent(&self.list, |list| {
            list.iter().map(|term| term.live.task()).collect()
        })
    }

    /// The list, woken whenever a terminal starts, ends or leaves.
    #[cfg(any(test, all(unix, feature = "cli")))]
    pub(super) fn subscribe(&self) -> watch::Receiver<Vec<Arc<Term>>> {
        self.list.subscribe()
    }

    /// Whether any terminal runs.
    #[cfg(all(unix, feature = "cli"))]
    pub(super) fn any_running(&self) -> bool {
        watched::read_sent(&self.list, |list| {
            list.iter().any(|term| term.live.is_running())
        })
    }

    /// Sets the environment terminals started from now on get.
    #[cfg(all(unix, feature = "cli"))]
    pub(super) fn set_env(&self, env: Vec<beep_proto::node::EnvSecret>) {
        self.opener.env.send_replace(Some(env));
    }

    async fn start(&self, command: Option<String>, cwd: Option<String>, size: Size) -> Reply {
        let (reply, answer) = oneshot::channel();
        let sent = self
            .commands()
            .send(Command::Start {
                command,
                cwd,
                size,
                reply,
            })
            .await;

        match sent {
            Ok(()) => answer.await.unwrap_or_else(|_| Reply::Error {
                // The reply is dropped when opening panicked.
                message: "Could not start the terminal.".to_owned(),
            }),
            Err(_) => stopped(),
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
        if let Some(term) = self.term(id) {
            term.signal(signal);
        }
    }

    fn term(&self, id: TaskId) -> Option<Arc<Term>> {
        watched::read_sent(&self.list, |list| {
            list.iter().find(|term| term.live.id == id).cloned()
        })
    }

    pub(super) fn running(&self) -> Vec<OpenTerminal> {
        watched::read_sent(&self.list, |list| {
            list.iter()
                .filter_map(|term| {
                    let screen = lock(&term.live.screen);
                    screen.phase.has_child().then(|| OpenTerminal {
                        id: term.live.id.0,
                        cwd: term.pid.and_then(current_dir).or_else(|| term.cwd.clone()),
                        command: term.live.command.clone(),
                    })
                })
                .collect()
        })
    }

    /// Opens a new shell for `open` under its id, unless one runs there.
    pub(super) async fn reopen(&self, was: OpenTerminal) -> Result<()> {
        let (reply, answer) = oneshot::channel();
        self.commands()
            .send(Command::Reopen { was, reply })
            .await
            .map_err(|_| anyhow::anyhow!("the terminals stopped"))?;
        answer
            .await
            .map_err(|_| anyhow::anyhow!("opening the terminal failed"))?
    }

    // A terminal host lets its terminals outlive it.
    #[cfg_attr(all(unix, feature = "cli"), expect(dead_code))]
    pub(super) fn kill_all(&self) {
        // Set first: an open still running kills its terminal when it lands.
        self.opener.killed.store(true, Ordering::SeqCst);
        let terms = watched::read_sent(&self.list, Clone::clone);

        for term in terms {
            term.signal(Signal::Shutdown);
        }
    }

    /// The actor's mailbox, starting the actor the first time. The actor
    /// runs on the runtime that sends the first command, which is the only
    /// one in the node; it stops answering if that runtime ends.
    fn commands(&self) -> &actor::Sender<Command> {
        let started = self.actor.get_or_init(|| {
            let (commands, mailbox) = actor::channel("terminals", MAILBOX);
            let actor = Actor {
                last: first_id(),
                slots: Vec::new(),
                opening: JoinSet::new(),
                list: self.list.clone(),
                opener: self.opener.clone(),
            };

            Started {
                commands,
                _task: AbortOnDropHandle::new(tokio::spawn(actor.run(mailbox))),
            }
        });

        &started.commands
    }
}

pub(super) fn unknown(id: TaskId) -> Reply {
    Reply::Error {
        message: format!("No terminal {id}."),
    }
}

fn stopped() -> Reply {
    Reply::Error {
        message: "The terminals stopped.".to_owned(),
    }
}

/// Owns the set of terminals.
struct Actor {
    last: u32,
    slots: Vec<Slot>,
    /// Terminals being opened off the loop. Each holds a place.
    opening: JoinSet<Opened>,
    list: Arc<watch::Sender<Vec<Arc<Term>>>>,
    opener: Arc<Opener>,
}

struct Slot {
    term: Arc<Term>,
    /// When it goes from the list, once it ended.
    leaves: Option<Instant>,
}

/// Who asked for a terminal being opened.
enum Asked {
    Start(oneshot::Sender<Reply>),
    Reopen(oneshot::Sender<Result<()>>),
}

struct Opened {
    asked: Asked,
    term: Result<Term>,
}

impl Actor {
    async fn run(mut self, mut mailbox: actor::Receiver<Command>) {
        loop {
            let leaves = self.slots.iter().filter_map(|slot| slot.leaves).min();
            let expiry = async {
                match leaves {
                    Some(at) => sleep_until(at).await,
                    None => future::pending().await,
                }
            };

            tokio::select! {
                command = mailbox.recv() => match command {
                    Some((command, _timing)) => self.command(command),
                    None => return,
                },
                Some(opened) = self.opening.join_next() => self.opened(opened),
                () = self.opener.changed.notified() => self.publish(),
                () = expiry => self.expire(),
            }
        }
    }

    fn command(&mut self, command: Command) {
        match command {
            Command::Start {
                command,
                cwd,
                size,
                reply,
            } => {
                if let Err(message) = self.make_room() {
                    let _ = reply.send(Reply::Error { message });
                    return;
                }

                self.last = self.last.wrapping_add(1);
                let id = TaskId(self.last);
                self.open(Asked::Start(reply), move |opener| {
                    opener.open(id, command.as_deref(), cwd, size, None)
                });
            }
            Command::Reopen { was, reply } => {
                let id = TaskId(was.id);
                let taken = self.slots.iter().any(|slot| slot.term.live.id == id);

                if self.places() >= MAX_TERMINALS {
                    let _ = reply.send(Err(anyhow::anyhow!(
                        "{MAX_TERMINALS} terminals already run"
                    )));
                } else if taken {
                    let _ = reply.send(Err(anyhow::anyhow!("terminal {id} already runs")));
                } else {
                    // Ids handed out later stay clear of it.
                    self.last = self.last.max(was.id);
                    self.open(Asked::Reopen(reply), move |opener| {
                        let cwd = was.cwd.as_ref().filter(|cwd| cwd.is_dir());
                        let banner = reopened_banner(&was, cwd);
                        let cwd = cwd.map(|cwd| cwd.to_string_lossy().into_owned());
                        opener.open(id, None, cwd, Size::DEFAULT, Some(&banner))
                    });
                }
            }
        }
    }

    /// Opening a pseudo-terminal and its threads is slow, so it runs
    /// off the loop and comes back as a joined task.
    fn open(&mut self, asked: Asked, open: impl FnOnce(&Opener) -> Result<Term> + Send + 'static) {
        let opener = self.opener.clone();
        self.opening.spawn_blocking(move || Opened {
            asked,
            term: open(&opener),
        });
    }

    fn opened(&mut self, joined: Result<Opened, tokio::task::JoinError>) {
        let Opened { asked, term } = match joined {
            Ok(opened) => opened,
            // The caller's reply is dropped with it, which it reports.
            Err(e) => return log::warn!("opening a terminal failed: {e}"),
        };

        let term = term.map(|term| {
            let term = Arc::new(term);
            self.slots.push(Slot {
                term: term.clone(),
                leaves: None,
            });
            term
        });
        self.publish();

        // After the publish, so `kill_all` either sees the terminal listed
        // or this sees its flag.
        if let Ok(term) = &term
            && self.opener.killed.load(Ordering::SeqCst)
        {
            term.signal(Signal::Shutdown);
        }

        // The caller may have gone, which leaves the terminal kept.
        match (asked, term) {
            (Asked::Start(reply), Ok(term)) => {
                let _ = reply.send(Reply::Started {
                    task: term.live.task(),
                });
            }
            (Asked::Start(reply), Err(e)) => {
                let _ = reply.send(Reply::Error {
                    message: format!("{e:#}"),
                });
            }
            (Asked::Reopen(reply), term) => {
                let _ = reply.send(term.map(drop));
            }
        }
    }

    /// The places terminals hold, those open and those opening.
    fn places(&self) -> usize {
        self.slots.len() + self.opening.len()
    }

    /// Frees a place for a new terminal: at the limit the oldest that ended
    /// makes room.
    fn make_room(&mut self) -> Result<(), String> {
        if self.places() < MAX_TERMINALS {
            return Ok(());
        }

        match self
            .slots
            .iter()
            .position(|slot| !slot.term.live.is_running())
        {
            Some(oldest) => {
                self.slots.remove(oldest);
                Ok(())
            }
            None => Err(format!(
                "Terminal limit reached ({MAX_TERMINALS}). Close a terminal first."
            )),
        }
    }

    /// Drops the terminals whose time in the list is up.
    fn expire(&mut self) {
        let now = Instant::now();
        self.slots
            .retain(|slot| slot.leaves.is_none_or(|leaves| leaves > now));
        self.publish();
    }

    /// Publishes the set, giving a terminal that ended its time in the list.
    fn publish(&mut self) {
        let leaves = Instant::now() + KEEP_ENDED;

        for slot in &mut self.slots {
            if slot.leaves.is_none() && !slot.term.live.is_running() {
                slot.leaves = Some(leaves);
            }
        }

        self.list
            .send_replace(self.slots.iter().map(|slot| slot.term.clone()).collect());
    }
}

enum Signal {
    Stop,
    Escalate,
    Shutdown,
}

impl Term {
    /// The screen lock orders signals with reaping the child.
    fn signal(&self, signal: Signal) {
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

        if let Err(e) = lock(&self.killer).kill() {
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
