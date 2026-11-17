//! The app's own executable as the command of the agents' Beep MCP server.
//! The embedded node names it in each session's `session/new`, with
//! `--dir <node folder> mcp --session <id>`; run that way, the executable
//! only pipes stdin and stdout to the running app's node and never starts
//! the app. Being the app itself, it is signed and notarized with it.

#[cfg(feature = "local-host")]
use beep_node::agents::mcp::Invocation;
use std::ffi::OsString;
#[cfg(feature = "local-host")]
use tokio::runtime::Builder;

/// Runs the pipe when `args` (after the executable) are a session's `mcp`
/// command, returning the exit status; `None` when they are not, and the
/// app starts as usual.
pub fn run<I>(args: I) -> Option<i32>
where
    I: IntoIterator<Item = OsString>,
{
    #[cfg(not(feature = "local-host"))]
    {
        if args.into_iter().any(|arg| arg == "mcp") {
            eprintln!("beep mcp: local agent hosting is unavailable in this build");
            return Some(1);
        }

        None
    }
    #[cfg(feature = "local-host")]
    {
        let invocation = Invocation::parse(args)?;
        let ended = Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(anyhow::Error::from)
            .and_then(|runtime| runtime.block_on(invocation.run()));
        Some(match ended {
            Ok(()) => 0,
            Err(e) => {
                // Stdout is the MCP stream; the agent logs stderr.
                eprintln!("beep mcp: {e:#}");
                1
            }
        })
    }
}

#[cfg(test)]
#[path = "../tests/machines_mcp_command.rs"]
mod tests;
