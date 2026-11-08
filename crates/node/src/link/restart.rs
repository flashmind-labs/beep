//! What carries over a node restart: `Reopen` notes the open agent sessions
//! and terminals, and `listen` and `client::drain` let `beep-node update` wait
//! for the node to be idle.

use crate::{
    agents::{Agents, OpenSession},
    link::local_socket::{Listener, Stream},
    link::note,
    link::watchdog::{self, Step},
    run::terminal::{self, OpenTerminal},
};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{path::Path, sync::Arc};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader, split},
    sync::Notify,
};

/// The socket an update reaches the node on, in the node's folder.
const SOCKET: &str = "control.sock";
/// What the node has open, in its folder.
const FILE: &str = "reopen.json";

/// Woken when a session or terminal opens or closes.
static CHANGED: Notify = Notify::const_new();

/// The update's request: say when the sessions are idle.
const DRAIN: &str = "drain";
/// The node's answer: no turn runs.
const IDLE: &str = "idle";

/// What the node has open, or had when it stopped, rewritten as things
/// open and close and once more on stop. The next start, after a crash
/// too, resumes sessions by the agent's own id and opens terminals as new
/// shells in their folders. On Unix the terminal host keeps its own note.
#[derive(Debug, Default, Serialize, Deserialize)]
pub(crate) struct Reopen {
    pub(crate) sessions: Vec<OpenSession>,
    pub(crate) terminals: Vec<OpenTerminal>,
}

impl Reopen {
    /// What the node in `dir` had open. The note stays until the next
    /// write, so a start that dies before it took over still finds it.
    /// Nothing when there is none or it cannot be read.
    pub(crate) fn read(dir: &Path) -> Self {
        note::read(&dir.join(FILE))
    }

    /// What `agents` and the terminals have open now.
    pub(crate) fn now(agents: &Agents) -> Self {
        Self {
            sessions: agents.open_sessions(),
            terminals: terminal::running(),
        }
    }

    /// Writes this down in `dir` for the next start, whole or not at all.
    pub(crate) fn save(&self, dir: &Path) -> Result<()> {
        note::write_json(&dir.join(FILE), self)
    }
}

/// Says a session or terminal opened or closed.
pub(crate) fn changed() {
    CHANGED.notify_one();
}

/// Keeps the note in `dir` to what is open: written now, then after each
/// change. Changes that come while it writes make one more write.
pub(crate) async fn keep(dir: &Path, agents: &Agents) {
    loop {
        let noting = watchdog::enter(Step::OpenNote);
        let saved = Reopen::now(agents).save(dir);
        drop(noting);
        if let Err(e) = saved {
            log::warn!("what is open would not open again after a crash: {e:#}");
        }
        CHANGED.notified().await;
    }
}

/// For a host that embeds the node, as the Mac app: opens again what the
/// node in `dir` had open when it last stopped, terminals now and sessions
/// once the agent settings came (or `SETTINGS_WAIT` passed), then keeps the
/// note to what is open. Moves stop waiting once the sessions are back.
/// Until aborted.
pub async fn reopen_and_keep(dir: &Path, agents: &Agents) {
    let reopen = Reopen::read(dir);
    terminal::reopen(dir, reopen.terminals).await;
    agents.reopen(reopen.sessions).await;
    keep(dir, agents).await;
}

/// For a host that embeds the node, as it quits, once `reopen_and_keep` is
/// aborted: sessions that end from now on ended with it, and what is open
/// is noted in `dir` for the next start.
pub fn note_on_quit(dir: &Path, agents: &Agents) -> Result<()> {
    agents.begin_stopping();
    Reopen::now(agents).save(dir)
}

/// Answers updates asking the node to hold still, one at a time.
pub(crate) async fn listen(dir: &Path, agents: Arc<Agents>) -> Result<()> {
    let mut listener = Listener::bind(&dir.join(SOCKET))?;
    loop {
        let stream = listener.accept().await.context("taking an update")?;
        if let Err(e) = answer(stream, &agents).await {
            log::debug!("an update's connection failed: {e:#}");
        }
    }
}

/// Tells the update once the sessions are idle.
async fn answer(stream: Stream, agents: &Agents) -> Result<()> {
    let (read, mut write) = split(stream);
    let mut lines = BufReader::new(read).lines();
    if lines.next_line().await?.as_deref() != Some(DRAIN) {
        return Ok(());
    }
    log::info!("an update waits for the sessions to be idle");
    tokio::select! {
        () = agents.until_idle() => {}
        // The update gave up.
        _ = lines.next_line() => return Ok(()),
    }
    write.write_all(format!("{IDLE}\n").as_bytes()).await?;
    // Stay connected until the update restarts the node, or gives up.
    while lines.next_line().await?.is_some() {}
    log::info!("the update went on without restarting the node");
    Ok(())
}

/// The update's side: only the CLI updates itself.
#[cfg(feature = "cli")]
mod client {
    use super::*;
    use crate::link::local_socket;
    use std::time::Duration;
    use tokio::{
        io::{Lines, ReadHalf, WriteHalf},
        time::timeout,
    };

    /// The connection to the running node.
    pub(crate) struct UpdateConnection {
        _read: Lines<BufReader<ReadHalf<Stream>>>,
        _write: WriteHalf<Stream>,
    }

    /// How the node in a folder took an update's request to hold still.
    pub(crate) enum Drained {
        /// No node runs there, or one too old to be asked.
        NoNode,
        /// Its sessions were idle.
        Idle(UpdateConnection),
        /// A turn still ran after the wait.
        TimedOut(UpdateConnection),
    }

    /// Asks the node in `dir` to wait up to `limit` for its sessions to be idle.
    pub(crate) async fn drain(dir: &Path, limit: Duration) -> Drained {
        let Ok(stream) = local_socket::connect(&dir.join(SOCKET)).await else {
            return Drained::NoNode;
        };
        let (read, mut write) = split(stream);
        if write
            .write_all(format!("{DRAIN}\n").as_bytes())
            .await
            .is_err()
        {
            return Drained::NoNode;
        }
        let mut lines = BufReader::new(read).lines();
        match timeout(limit, lines.next_line()).await {
            Ok(Ok(Some(line))) if line == IDLE => Drained::Idle(UpdateConnection {
                _read: lines,
                _write: write,
            }),
            Ok(_) => Drained::NoNode,
            Err(_) => Drained::TimedOut(UpdateConnection {
                _read: lines,
                _write: write,
            }),
        }
    }
}
#[cfg(feature = "cli")]
pub(crate) use client::{Drained, UpdateConnection, drain};

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tokio::time::timeout;

    #[tokio::test]
    async fn an_update_can_wait_for_idle() {
        let dir = tempfile::tempdir().expect("a temporary folder");
        let agents = Arc::new(Agents::new(Vec::new()));
        let listening = {
            let (dir, agents) = (dir.path().to_path_buf(), agents.clone());
            tokio_util::task::AbortOnDropHandle::new(tokio::spawn(async move {
                listen(&dir, agents).await
            }))
        };
        // Bound before the update asks: it waits for the socket.
        let drained = loop {
            match drain(dir.path(), Duration::from_secs(20)).await {
                Drained::NoNode if !listening.is_finished() => tokio::task::yield_now().await,
                other => break other,
            }
        };
        let Drained::Idle(held) = drained else {
            panic!("a node without sessions is idle at once");
        };
        drop(held);
    }

    #[tokio::test]
    async fn a_host_marks_sessions_reopened_and_notes_what_is_open() {
        let dir = tempfile::tempdir().expect("a temporary folder");
        let agents = Arc::new(Agents::new(Vec::new()));
        let keeping = {
            let (dir, agents) = (dir.path().to_path_buf(), agents.clone());
            tokio_util::task::AbortOnDropHandle::new(tokio::spawn(async move {
                reopen_and_keep(&dir, &agents).await;
            }))
        };
        // No note and no settings: moves stop waiting at once.
        timeout(Duration::from_secs(20), agents.until_reopened())
            .await
            .expect("reopened without waiting for settings");
        keeping.abort();
        let _ = keeping.await;
        std::fs::remove_file(dir.path().join(FILE)).ok();
        note_on_quit(dir.path(), &agents).expect("the note is written");
        let note = Reopen::read(dir.path());
        assert!(dir.path().join(FILE).exists());
        assert!(note.sessions.is_empty() && note.terminals.is_empty());
    }
}
