//! The app's own executable as a command an agent runs: the agents' Beep
//! MCP server, named in each session's `session/new` with
//! `--dir <node folder> mcp --session <id>`. Run that way, the executable
//! serves that command and never starts the app; otherwise each agent would
//! start another app, whose node would start another. Being the app itself,
//! it is signed and notarized with it.

use beep_node::tools::Invocation;
use std::ffi::OsString;
use std::future::Future;
use tokio::runtime::Builder;

/// Serves the command `args` (after the executable) name, returning the
/// exit status; `None` when they name none, and the app starts as usual.
pub fn run<I>(args: I) -> Option<i32>
where
    I: IntoIterator<Item = OsString>,
{
    let invocation = Invocation::parse(args)?;
    // Stdout is the MCP stream; the agent logs stderr.
    Some(serve("beep mcp", invocation.run()))
}

/// Runs `work` to its end on a runtime of its own, with its error on stderr:
/// stdout carries the protocol.
fn serve(name: &str, work: impl Future<Output = anyhow::Result<()>>) -> i32 {
    let ended = Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(anyhow::Error::from)
        .and_then(|runtime| runtime.block_on(work));

    match ended {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("{name}: {e:#}");
            1
        }
    }
}

#[cfg(test)]
#[path = "../tests/machines_mcp_command.rs"]
mod tests;
