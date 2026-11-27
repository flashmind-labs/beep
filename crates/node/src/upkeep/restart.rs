//! What carries over a node restart: `Reopen` notes the open agent sessions
//! and terminals, and `listen` and `client::drain` let `beep-node update` wait
//! for the node to be idle.

use beep_proto::SessionId;
use crate::{
    agents::{Agents, OpenSession, SavedSession},
    fs::note,
    run::local_socket::{Listener, Stream},
    link::watchdog::{self, Step},
    run::terminal::{self, OpenTerminal},
};
use anyhow::{Context, Result};
use beep_proto::ModelId;
use serde::{Deserialize, Serialize};
use std::{path::Path, sync::Arc};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, split};

/// The socket an update reaches the node on, in the node's folder.
const SOCKET: &str = "control.sock";
/// What the node has open, in its folder.
const FILE: &str = "reopen.json";

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
    /// What sessions were, ended ones too.
    #[serde(default)]
    pub(crate) saved: Vec<SavedSession>,
    /// The models sessions last ran on, as nodes from before `saved` keep
    /// them: written beside `saved` so such a node, rolled back to, keeps them.
    #[serde(default)]
    models: Vec<OldModel>,
    /// The worktrees sessions created, as `models`.
    #[serde(default)]
    worktrees: Vec<OldWorktrees>,
}

/// A session's model as nodes from before `saved` keep it.
#[derive(Debug, Serialize, Deserialize)]
struct OldModel {
    session: SessionId,
    model: ModelId,
}

/// A session's worktrees as nodes from before `saved` keep them.
#[derive(Debug, Serialize, Deserialize)]
struct OldWorktrees {
    session: SessionId,
    worktrees: Vec<String>,
}

impl Reopen {
    /// What the node in `dir` had open. The note stays until the next
    /// write, so a start that dies before it took over still finds it.
    /// Nothing when there is none or it cannot be read. The models and
    /// worktrees of a note from before `saved` go into the saved session
    /// they belong to, or one made from the open session; others are left
    /// out, as nothing else is known of them.
    pub(crate) fn read(dir: &Path) -> Self {
        let mut reopen: Self = note::read(&dir.join(FILE));

        for old in std::mem::take(&mut reopen.models) {
            if let Some(saved) = reopen.saved_for(&old.session) {
                saved.info.model.get_or_insert(old.model);
            }
        }

        for old in std::mem::take(&mut reopen.worktrees) {
            if let Some(saved) = reopen.saved_for(&old.session)
                && saved.info.worktrees.is_empty()
            {
                saved.info.worktrees = old.worktrees;
            }
        }

        reopen
    }

    /// What the node has open now, what its sessions were, and the same as
    /// nodes from before `saved` keep it.
    pub(crate) fn new(
        sessions: Vec<OpenSession>,
        terminals: Vec<OpenTerminal>,
        saved: Vec<SavedSession>,
    ) -> Self {
        let models = saved
            .iter()
            .filter_map(|kept| {
                Some(OldModel {
                    session: kept.info.id.clone(),
                    model: kept.info.model.clone()?,
                })
            })
            .collect();
        let worktrees = saved
            .iter()
            .filter(|kept| !kept.info.worktrees.is_empty())
            .map(|kept| OldWorktrees {
                session: kept.info.id.clone(),
                worktrees: kept.info.worktrees.clone(),
            })
            .collect();

        Self {
            sessions,
            terminals,
            saved,
            models,
            worktrees,
        }
    }

    /// What `agents` and the terminals have open now.
    pub(crate) fn now(agents: &Agents) -> Self {
        Self::new(
            agents.open_sessions(),
            terminal::running(),
            agents.saved_sessions(),
        )
    }

    /// The saved entry of session `id`: the one there is, else one made
    /// from its open session; `None` when there is neither.
    fn saved_for(&mut self, id: &SessionId) -> Option<&mut SavedSession> {
        let at = match self.saved.iter().position(|saved| saved.info.id == *id) {
            Some(at) => at,
            None => {
                let open = self.sessions.iter().find(|open| open.session == *id)?;
                self.saved.push(Into::<SavedSession>::into(open));
                self.saved.len() - 1
            }
        };

        self.saved.get_mut(at)
    }

    /// Writes this down in `dir` for the next start, whole or not at all.
    pub(crate) fn save(&self, dir: &Path) -> Result<()> {
        note::write_json(&dir.join(FILE), self)
    }
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

        crate::events::opened_or_closed().await;
    }
}

/// For a host that embeds the node, as the Mac app: opens again what the
/// node in `dir` had open when it last stopped, terminals now and sessions
/// once the agent settings came (or `SETTINGS_WAIT` passed), then keeps the
/// note to what is open. Moves stop waiting once the sessions are back.
/// Until aborted.
pub async fn reopen_and_keep(dir: &Path, agents: &Agents) {
    let reopen = Reopen::read(dir);
    agents.keep_saved(reopen.saved);
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
    use crate::run::local_socket;
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
#[path = "../tests/upkeep_restart.rs"]
mod tests;
