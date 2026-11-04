//! What carries over a node restart: `Reopen` notes the open agent sessions
//! and terminals, and `listen` and `client::drain` let `beep-node update` wait
//! for the node to be idle.

use crate::{
    agents::{Agents, OpenSession},
    link::local_socket::{Listener, Stream},
    link::note,
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

/// The update's request: hold prompts and say when the sessions are idle.
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
        if let Err(e) = Reopen::now(agents).save(dir) {
            log::warn!("what is open would not open again after a crash: {e:#}");
        }
        CHANGED.notified().await;
    }
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

/// Holds prompts while the update that sent `drain` stays connected, and
/// tells it once the sessions are idle.
async fn answer(stream: Stream, agents: &Agents) -> Result<()> {
    let (read, mut write) = split(stream);
    let mut lines = BufReader::new(read).lines();
    if lines.next_line().await?.as_deref() != Some(DRAIN) {
        return Ok(());
    }
    let _hold = agents.hold();
    log::info!("an update waits: holding new prompts until the sessions are idle");
    tokio::select! {
        () = agents.until_idle() => {}
        // The update gave up; prompts go again.
        _ = lines.next_line() => return Ok(()),
    }
    write.write_all(format!("{IDLE}\n").as_bytes()).await?;
    // Held until the update restarts the node, or gives up.
    while lines.next_line().await?.is_some() {}
    log::info!("the update went on without restarting the node; prompts go again");
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

    /// The running node holding its prompts while this lives.
    pub(crate) struct Holding {
        _read: Lines<BufReader<ReadHalf<Stream>>>,
        _write: WriteHalf<Stream>,
    }

    /// How the node in a folder took an update's request to hold still.
    pub(crate) enum Drained {
        /// No node runs there, or one too old to be asked.
        NoNode,
        /// Its sessions are idle; it holds new prompts.
        Idle(Holding),
        /// A turn still ran after the wait; it holds new prompts.
        TimedOut(Holding),
    }

    /// Asks the node in `dir` to hold new prompts and waits up to `limit` for
    /// its sessions to be idle.
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
            Ok(Ok(Some(line))) if line == IDLE => Drained::Idle(Holding {
                _read: lines,
                _write: write,
            }),
            Ok(_) => Drained::NoNode,
            Err(_) => Drained::TimedOut(Holding {
                _read: lines,
                _write: write,
            }),
        }
    }
}
#[cfg(feature = "cli")]
pub(crate) use client::{Drained, Holding, drain};

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tokio::time::timeout;

    #[tokio::test]
    async fn an_update_holds_prompts_while_it_stays_connected() {
        let dir = tempfile::tempdir().expect("a temporary folder");
        let agents = Arc::new(Agents::new(Vec::new()));
        let mut holding = agents.holding_now();
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
        assert!(*holding.borrow_and_update());
        drop(held);
        timeout(Duration::from_secs(20), holding.wait_for(|held| !held))
            .await
            .expect("the hold ends with the update's connection")
            .expect("the node is kept");
    }
}
