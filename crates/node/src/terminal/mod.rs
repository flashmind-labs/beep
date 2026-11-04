//! `terminal/1`: interactive terminals. Each runs in a pseudo-terminal
//! that outlives the stream that started it, so it can be attached to
//! again. At most `MAX_TERMINALS` run, each keeping the last
//! `MAX_SCROLLBACK` lines of its output.
//!
//! On Unix the pseudo-terminals belong to a process of their own, the
//! terminal host (`host`), so they outlive the node too: the node relays
//! each stream to it. Elsewhere, and in a node an app embeds, the node
//! keeps them itself.
//!
//! A terminal has three threads, each ending with it: one reads the
//! pseudo-terminal into the scrollback, one writes typed keys to it, and
//! one waits for the child to exit. The pseudo-terminal's calls block, so
//! they stay off the async workers.

#[cfg(all(unix, feature = "cli"))]
pub(crate) mod host;
mod pty;
mod screen;
mod terminals;

use crate::lock;
use anyhow::Result;
use beep_proto::node::{
    LinkRx, LinkTx,
    process::TaskId,
    terminal::{MAX_INPUT, Reply, Request},
};
use serde::{Deserialize, Serialize};
use serde_bytes::ByteBuf;
#[cfg(not(all(unix, feature = "cli")))]
use std::sync::LazyLock;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use terminals::{Live, Terminals};

pub use beep_proto::node::terminal::PROTOCOL;

/// The node's terminals, when it keeps them itself: one set per process,
/// like the background tasks.
#[cfg(not(all(unix, feature = "cli")))]
static TERMINALS: LazyLock<Terminals> = LazyLock::new(Terminals::default);

/// Kills every terminal the node keeps itself, at once, for a host about
/// to exit. Those of a terminal host outlive the node.
pub fn kill_all() {
    #[cfg(not(all(unix, feature = "cli")))]
    TERMINALS.kill_all();
}

/// A terminal open when the node stopped, to open again as a new shell
/// under the same id when it starts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct OpenTerminal {
    id: u32,
    /// Where its shell was last, else where it started.
    cwd: Option<PathBuf>,
    /// What it ran.
    command: String,
}

/// The terminals the node keeps itself and that run now; none when a
/// terminal host keeps them, which notes its own.
pub(crate) fn running() -> Vec<OpenTerminal> {
    #[cfg(all(unix, feature = "cli"))]
    return Vec::new();
    #[cfg(not(all(unix, feature = "cli")))]
    TERMINALS.running()
}

/// Opens again what was open when the node in `dir` stopped. The terminal
/// host still runs its terminals; one that died, as with the machine,
/// opens those it noted again. Otherwise `terminals` open again, each a new
/// shell in its folder that says so.
pub(crate) async fn reopen(dir: &Path, terminals: Vec<OpenTerminal>) {
    #[cfg(all(unix, feature = "cli"))]
    {
        // The host notes them; the node has none.
        drop(terminals);
        host::reopen(dir).await;
    }
    #[cfg(not(all(unix, feature = "cli")))]
    {
        let _ = dir;
        for open in terminals {
            if let Err(e) = TERMINALS.reopen(&open) {
                log::warn!("could not reopen terminal {}: {e:#}", open.id);
            }
        }
    }
}

/// Serves one session stream until the server closes it. An `Attach`
/// keeps the stream to itself until the terminal ends.
pub async fn serve(tx: LinkTx, rx: LinkRx) -> Result<()> {
    let mut link = Link { tx, rx };
    #[cfg(all(unix, feature = "cli"))]
    return host::relay(&mut link).await;
    #[cfg(not(all(unix, feature = "cli")))]
    serve_with(&TERMINALS, &mut link).await
}

/// The far side of a `terminal/1` stream: the server, over the link, or
/// the node, over the terminal host's socket.
trait Peer {
    async fn send(&mut self, reply: &Reply) -> Result<()>;
    async fn recv(&mut self) -> Result<Option<Request>>;
}

struct Link {
    tx: LinkTx,
    rx: LinkRx,
}

impl Peer for Link {
    async fn send(&mut self, reply: &Reply) -> Result<()> {
        self.tx.send(reply).await
    }

    async fn recv(&mut self) -> Result<Option<Request>> {
        self.rx.recv().await
    }
}

async fn serve_with(terminals: &Terminals, peer: &mut impl Peer) -> Result<()> {
    while let Some(request) = peer.recv().await? {
        if let Request::Attach { id, from } = request {
            return match terminals.live(id) {
                Some(live) => attach(live, from, peer).await,
                None => peer.send(&unknown(id)).await,
            };
        }
        let reply = terminals.answer(request).await;
        peer.send(&reply).await?;
    }
    Ok(())
}

fn unknown(id: TaskId) -> Reply {
    Reply::Error {
        message: format!("No terminal {id}."),
    }
}

/// Streams terminal `live` from byte `from` and types what the peer sends
/// into it, until it ends or the peer closes the stream.
async fn attach(live: Arc<Live>, from: u64, peer: &mut impl Peer) -> Result<()> {
    let mut changed = live.changed.subscribe();
    let mut next = from;
    loop {
        // Marked seen before reading, so output that lands after the read
        // still wakes the wait below.
        changed.borrow_and_update();
        let (chunk, state) = {
            let screen = lock(&live.screen);
            (screen.read_from(next), screen.state)
        };
        if let Some((at, data)) = chunk {
            next = at + data.len() as u64;
            peer.send(&Reply::Output {
                at,
                data: ByteBuf::from(data),
            })
            .await?;
            continue;
        }
        if !state.is_running() {
            return peer.send(&Reply::Ended { task: live.task() }).await;
        }
        tokio::select! {
            woke = changed.changed() => if woke.is_err() {
                return Ok(());
            },
            request = peer.recv() => match request {
                Ok(Some(Request::Input { data })) if data.len() <= MAX_INPUT => {
                    live.type_keys(data.into_vec());
                }
                Ok(Some(Request::Input { data })) => {
                    log::debug!("dropped {} bytes of input past the limit", data.len());
                }
                Ok(Some(Request::Resize { size })) => {
                    live.resize(size);
                }
                Ok(Some(other)) => log::debug!("ignored {other:?} on an attached terminal"),
                // The peer detached.
                Ok(None) | Err(_) => return Ok(()),
            },
        }
    }
}
