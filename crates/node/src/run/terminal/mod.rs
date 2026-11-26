//! `terminal/1`: interactive terminals whose pseudo-terminals outlive the
//! stream that started them. `serve` handles a stream; on Unix the terminal
//! host (`host`) keeps them, elsewhere and in an embedded node the node does.

#[cfg(all(unix, feature = "cli"))]
pub(crate) mod host;
mod pty;
mod screen;
mod serve;
mod terminals;

use anyhow::Result;
use beep_proto::node::{LinkRx, LinkTx};
use serve::Link;
#[cfg(not(all(unix, feature = "cli")))]
use serve::serve_with;
#[cfg(not(all(unix, feature = "cli")))]
use std::sync::LazyLock;
use std::path::Path;
#[cfg(not(all(unix, feature = "cli")))]
use terminals::Terminals;

pub(crate) use terminals::OpenTerminal;
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
            let id = open.id;

            if let Err(e) = TERMINALS.reopen(open).await {
                log::warn!("could not reopen terminal {id}: {e:#}");
            }
        }
    }
}

/// Serves one session stream until the caller closes it. An `Attach`
/// keeps the stream to itself until the terminal ends.
pub async fn serve(tx: LinkTx, rx: LinkRx) -> Result<()> {
    let mut link = Link { tx, rx };
    #[cfg(all(unix, feature = "cli"))]
    return host::relay(&mut link).await;
    #[cfg(not(all(unix, feature = "cli")))]
    serve_with(&TERMINALS, &mut link).await
}
