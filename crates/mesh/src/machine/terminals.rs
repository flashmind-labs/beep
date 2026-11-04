//! Terminals on a node: background tasks (`process/1`, view only) and
//! pseudo-terminals (`terminal/1`). Which terminal is which task is for
//! the caller to remember; the node only knows its tasks.

use super::{
    Tasks,
    node::{TASK_LIMIT, Words, exchange},
    said, unexpected,
};
use crate::{MeshError, PeerClient};
use beep_proto::{
    ApiError, TerminalEvent, TerminalState,
    node::{
        LinkRx, LinkTx,
        process::{Task, TaskId, TaskState},
        terminal::{PROTOCOL, Reply, Request, Size},
    },
};

/// The longest name, command or folder.
pub const MAX_FIELD: usize = 1000;

/// A terminal a caller keeps: which task on its node, and what it ran.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Kept<'a> {
    pub task: u32,
    pub pty: bool,
    pub command: &'a str,
}

/// Sends one `terminal/1` request.
pub async fn request(client: &PeerClient, request: &Request) -> Result<Reply, MeshError> {
    let words = Words {
        silent: "the node stopped answering",
        closed: "the node closed the terminal session",
    };
    exchange(client, TASK_LIMIT, PROTOCOL, request, words).await
}

/// Starts an interactive terminal on `command`, or the shell when none.
pub async fn open(
    client: &PeerClient,
    command: Option<String>,
    cwd: Option<String>,
    size: Size,
) -> Result<Task, MeshError> {
    match request(client, &Request::Start { command, cwd, size }).await? {
        Reply::Started { task } => Ok(task),
        Reply::Error { message } => Err(said(message)),
        other => Err(unexpected(&other)),
    }
}

/// The node's interactive terminals.
pub async fn list_ptys(client: &PeerClient) -> Result<Vec<Task>, MeshError> {
    match request(client, &Request::List {}).await? {
        Reply::Terminals { terminals } => Ok(terminals),
        other => Err(unexpected(&other)),
    }
}

/// The node's background tasks.
pub async fn list_tasks(client: &PeerClient) -> Result<Vec<Task>, MeshError> {
    use beep_proto::node::process::{Reply, Request};
    match Tasks(client).ask(&Request::List {}).await? {
        Reply::Tasks { tasks } => Ok(tasks),
        other => Err(unexpected(&other)),
    }
}

/// Opens a stream following terminal `id` from byte `from`.
pub async fn attach(
    client: &PeerClient,
    id: TaskId,
    from: u64,
    size: Size,
) -> Result<(LinkTx, LinkRx), MeshError> {
    let (mut tx, rx) = client.open(PROTOCOL).await?;
    tx.send(&Request::Attach { id, from })
        .await
        .map_err(MeshError::Link)?;
    tx.send(&Request::Resize { size })
        .await
        .map_err(MeshError::Link)?;
    Ok((tx, rx))
}

/// Stops the terminal if it still runs its command; one reusing its number is left alone.
pub async fn stop(client: &PeerClient, kept: &Kept<'_>) -> Result<(), MeshError> {
    let id = TaskId(kept.task);
    if !kept.pty {
        return Tasks(client).stop(id, kept.command).await;
    }
    let running = list_ptys(client)
        .await?
        .iter()
        .any(|task| task.id == id && task.command == kept.command && task.state.is_running());
    if !running {
        return Ok(());
    }
    match request(client, &Request::Stop { id }).await? {
        Reply::Error { message } => Err(said(message)),
        _ => Ok(()),
    }
}

/// How the terminal stands among the node's `tasks`; the same number with another command is a new task.
pub fn state_of(kept: &Kept<'_>, tasks: &[Task]) -> TerminalState {
    tasks
        .iter()
        .find(|task| task.id.0 == kept.task && task.command == kept.command)
        .map_or(TerminalState::Gone {}, |task| state(task.state))
}

pub fn state(state: TaskState) -> TerminalState {
    match state {
        TaskState::Running => TerminalState::Running {},
        TaskState::Exited { code } => TerminalState::Exited { code },
        TaskState::Stopped => TerminalState::Stopped {},
        _ => TerminalState::Gone {},
    }
}

/// How the node's stream of an attached terminal reads to the app.
#[derive(Debug, PartialEq)]
pub enum Heard {
    Show(TerminalEvent),
    /// The terminal is over.
    End(TerminalState),
    Ignore,
}

pub fn heard(reply: Reply) -> Heard {
    match reply {
        Reply::Output { at, data } => Heard::Show(TerminalEvent::Output { at, data }),
        Reply::Ended { task } => Heard::End(state(task.state)),
        // The node no longer has it, such as after a restart.
        Reply::Error { .. } => Heard::End(TerminalState::Gone {}),
        _ => Heard::Ignore,
    }
}

/// `text` trimmed, refused when empty, too long or on several lines.
pub fn field<'a>(what: &str, text: &'a str) -> Result<&'a str, ApiError> {
    let text = text.trim();
    if text.is_empty() || text.len() > MAX_FIELD || text.contains(['\n', '\r', '\0']) {
        return Err(ApiError::bad_request(format!(
            "A terminal needs a {what} of 1 to {MAX_FIELD} characters on one line."
        )));
    }
    Ok(text)
}

/// `text` trimmed, which may be empty; else as `field` takes it.
pub fn optional<'a>(what: &str, text: &'a str) -> Result<&'a str, ApiError> {
    match text.trim() {
        "" => Ok(""),
        text => field(what, text),
    }
}

/// Whether an interactive terminal that stands so will never run again.
pub fn ended_for_good(kept: &Kept<'_>, state: &TerminalState) -> bool {
    kept.pty
        && matches!(
            state,
            TerminalState::Exited { .. } | TerminalState::Stopped {} | TerminalState::Gone {}
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kept(pty: bool) -> Kept<'static> {
        Kept {
            task: 3,
            pty,
            command: "npm run dev",
        }
    }

    fn task(id: u32, command: &str, state: TaskState) -> Task {
        Task {
            id: TaskId(id),
            command: command.into(),
            state,
        }
    }

    #[test]
    fn a_terminal_is_its_task_only_while_the_command_matches() {
        let kept = kept(false);
        assert_eq!(
            state_of(&kept, &[task(3, "npm run dev", TaskState::Running)]),
            TerminalState::Running {}
        );
        assert_eq!(
            state_of(&kept, &[task(3, "cargo watch", TaskState::Running)]),
            TerminalState::Gone {}
        );
        assert_eq!(state_of(&kept, &[]), TerminalState::Gone {});
    }

    #[test]
    fn only_an_ended_interactive_terminal_is_dropped() {
        assert!(ended_for_good(&kept(true), &TerminalState::Stopped {}));
        assert!(!ended_for_good(&kept(true), &TerminalState::Offline {}));
        assert!(!ended_for_good(
            &kept(false),
            &TerminalState::Exited { code: Some(1) }
        ));
    }
}
