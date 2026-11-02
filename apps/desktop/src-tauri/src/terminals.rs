//! A project's terminals for the window: commands kept running on the
//! user's machines, their output, starting and closing them.

use crate::{
    beep::{Beep, epoch_ms},
    error::{DesktopError, Result},
    projects::unknown_op,
};
use beep_proto::{
    CloseTerminal, GetTerminal, ListTerminals, StartTerminal, Terminal, TerminalOutput,
    TerminalState,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::State;
use uuid::Uuid;

type Service<'a> = State<'a, Arc<Beep>>;

/// One terminal as the window shows it.
#[derive(Debug, Serialize)]
pub struct TerminalRow {
    id: Uuid,
    machine_id: String,
    machine_name: String,
    name: String,
    command: String,
    cwd: String,
    /// `running`, `exited`, `stopped`, `offline` or `gone`.
    state: &'static str,
    /// How it exited, when it did on its own.
    code: Option<i32>,
    /// In milliseconds since the Unix epoch.
    started_at: u64,
}

impl From<Terminal> for TerminalRow {
    fn from(t: Terminal) -> Self {
        let code = match t.state {
            TerminalState::Exited { code } => code,
            _ => None,
        };
        Self {
            id: t.id,
            machine_id: t.machine_id,
            machine_name: t.machine_name,
            name: t.name,
            command: t.command,
            cwd: t.cwd,
            state: t.state.into(),
            code,
            started_at: epoch_ms(t.started_at),
        }
    }
}

/// A terminal with the last of its output.
#[derive(Debug, Serialize)]
pub struct OutputView {
    terminal: TerminalRow,
    lines: Vec<String>,
    earlier: bool,
}

fn outdated(e: DesktopError) -> DesktopError {
    if unknown_op(&e) {
        DesktopError::BadRequest("The server needs an update before it keeps terminals.".into())
    } else {
        e
    }
}

/// `project`'s terminals; `None` from a server too old to keep them.
#[tauri::command]
pub async fn terminals_list(
    beep: Service<'_>,
    project: String,
) -> Result<Option<Vec<TerminalRow>>> {
    match beep.call(ListTerminals { project }).await {
        Ok(rows) => Ok(Some(rows.into_iter().map(Into::into).collect())),
        Err(e) if unknown_op(&e) => Ok(None),
        Err(e) => Err(e),
    }
}

/// The fields of a new terminal, as the form gives them.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewTerminal {
    project: String,
    machine_id: String,
    name: String,
    command: String,
    cwd: String,
}

#[tauri::command]
pub async fn terminal_start(beep: Service<'_>, terminal: NewTerminal) -> Result<TerminalRow> {
    let NewTerminal {
        project,
        machine_id,
        name,
        command,
        cwd,
    } = terminal;
    let started = beep
        .call(StartTerminal {
            project,
            machine_id,
            name,
            command,
            cwd,
        })
        .await
        .map_err(outdated)?;
    Ok(started.into())
}

#[tauri::command]
pub async fn terminal_output(beep: Service<'_>, id: Uuid) -> Result<OutputView> {
    let TerminalOutput {
        terminal,
        lines,
        earlier,
    } = beep.call(GetTerminal { id }).await.map_err(outdated)?;
    Ok(OutputView {
        terminal: terminal.into(),
        lines,
        earlier,
    })
}

#[tauri::command]
pub async fn terminal_close(beep: Service<'_>, id: Uuid) -> Result<()> {
    beep.call(CloseTerminal { id }).await.map_err(outdated)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::UNIX_EPOCH;

    #[test]
    fn an_exited_terminal_keeps_its_code() {
        let row = TerminalRow::from(Terminal {
            id: Uuid::nil(),
            project: "acme/widget".into(),
            machine_id: "m1".into(),
            machine_name: "build-box".into(),
            name: "tests".into(),
            command: "just test".into(),
            cwd: "/srv/project".into(),
            state: TerminalState::Exited { code: Some(2) },
            started_at: UNIX_EPOCH,
        });
        assert_eq!(row.state, "exited");
        assert_eq!(row.code, Some(2));
    }
}
