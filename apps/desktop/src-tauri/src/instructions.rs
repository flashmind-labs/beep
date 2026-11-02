//! The user's instructions for every agent Beep runs, for the window's
//! Instructions view: the saved versions, and the agent files on the
//! machines to import from and write into.

use crate::{
    beep::{Beep, epoch_ms},
    error::{DesktopError, Result},
    projects::unknown_op,
};
use beep_proto::{
    GetInstructions, InstructionsVersion, ListInstructionsVersions, MachineAgentFiles,
    ReadAgentFiles, SaveInstructions, SyncAgentFiles,
};
use serde::Serialize;
use std::sync::Arc;
use tauri::State;

type Service<'a> = State<'a, Arc<Beep>>;

/// One saved version as the view shows it.
#[derive(Debug, Serialize)]
pub struct VersionRow {
    version: u32,
    text: String,
    note: String,
    /// In milliseconds since the Unix epoch.
    saved_at: u64,
}

impl From<InstructionsVersion> for VersionRow {
    fn from(v: InstructionsVersion) -> Self {
        Self {
            version: v.version,
            text: v.text,
            note: v.note,
            saved_at: epoch_ms(v.saved_at),
        }
    }
}

/// A server from before instructions answers with an unknown op.
fn outdated(e: DesktopError) -> DesktopError {
    if unknown_op(&e) {
        DesktopError::BadRequest("The server needs an update before it keeps instructions.".into())
    } else {
        e
    }
}

/// The newest version, or `None` before the first save.
#[tauri::command]
pub async fn instructions_get(beep: Service<'_>) -> Result<Option<VersionRow>> {
    let newest = beep.call(GetInstructions {}).await.map_err(outdated)?;
    Ok(newest.map(Into::into))
}

/// Saves `text` as the version after `base`.
#[tauri::command]
pub async fn instructions_save(
    beep: Service<'_>,
    text: String,
    base: Option<u32>,
    note: String,
) -> Result<VersionRow> {
    let saved = beep
        .call(SaveInstructions { text, base, note })
        .await
        .map_err(outdated)?;
    Ok(saved.into())
}

/// Every version, newest first.
#[tauri::command]
pub async fn instructions_versions(beep: Service<'_>) -> Result<Vec<VersionRow>> {
    let versions = beep
        .call(ListInstructionsVersions {})
        .await
        .map_err(outdated)?;
    Ok(versions.into_iter().map(Into::into).collect())
}

/// The agent files on every online machine.
#[tauri::command]
pub async fn agent_files_read(beep: Service<'_>) -> Result<Vec<MachineAgentFiles>> {
    beep.call(ReadAgentFiles {}).await.map_err(outdated)
}

/// Writes the newest version into the agent files on `machine_id`.
#[tauri::command]
pub async fn agent_files_sync(beep: Service<'_>, machine_id: String) -> Result<MachineAgentFiles> {
    beep.call(SyncAgentFiles { machine_id })
        .await
        .map_err(outdated)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, UNIX_EPOCH};

    #[test]
    fn a_version_carries_its_time_in_milliseconds() {
        let row = VersionRow::from(InstructionsVersion {
            version: 7,
            text: "- Write plainly.".into(),
            note: String::new(),
            saved_at: UNIX_EPOCH + Duration::from_millis(1_500),
        });
        assert_eq!(row.saved_at, 1_500);
        assert_eq!(row.version, 7);
    }
}
