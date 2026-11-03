//! The app's own executable as the command of the agents' Beep MCP server.
//! The embedded node names it in each session's `session/new`, with
//! `--dir <node folder> mcp --session <id>`; run that way, the executable
//! only pipes stdin and stdout to the running app's node and never starts
//! the app. Being the app itself, it is signed and notarized with it.

use beep_node::mcp::Invocation;
use std::ffi::OsString;
use tokio::runtime::Builder;

/// Runs the pipe when `args` (after the executable) are a session's `mcp`
/// command, returning the exit status; `None` when they are not, and the
/// app starts as usual.
pub fn run<I>(args: I) -> Option<i32>
where
    I: IntoIterator<Item = OsString>,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_app_starts_on_its_own_arguments() {
        assert_eq!(run(Vec::<OsString>::new()), None);
        assert_eq!(run([OsString::from("--login")]), None);
    }

    #[test]
    fn a_session_command_runs_the_pipe_and_fails_without_a_node() {
        // Without the token, or with it and no node at that folder, the
        // pipe fails rather than the app starting.
        let args = ["--dir", "/srv/node", "mcp", "--session", "a3f9c1"].map(OsString::from);
        assert_eq!(run(args), Some(1));
    }
}
