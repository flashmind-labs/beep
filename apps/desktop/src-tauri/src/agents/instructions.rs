//! The user's instructions for every agent Beep runs, for the window's
//! Instructions view: the saved versions, and the agent files on the
//! machines to import from and write into.

use crate::{
    beep::{Service, epoch_ms},
    error::{DesktopError, Result},
    fleet::{
        Machines, extra, files as direct,
        projects::{FleetState, desk},
    },
    projects::needs_update,
};
use beep_proto::{InstructionsVersion, MachineAgentFiles, ReadAgentFiles, SyncAgentFiles};
use serde::Serialize;

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
    needs_update(e, "before it keeps instructions")
}

/// The newest version, or `None` before the first save.
#[tauri::command]
pub async fn instructions_get(fleet: FleetState<'_>) -> Result<Option<VersionRow>> {
    Ok(desk(&fleet).await?.instructions().map(Into::into))
}

/// Saves `text` as the version after `base`.
#[tauri::command]
pub async fn instructions_save(
    fleet: FleetState<'_>,
    text: String,
    base: Option<u32>,
    note: String,
) -> Result<VersionRow> {
    let saved = desk(&fleet)
        .await?
        .save_instructions(&text, base, &note)
        .await?;
    Ok(saved.into())
}

/// Every version, newest first.
#[tauri::command]
pub async fn instructions_versions(fleet: FleetState<'_>) -> Result<Vec<VersionRow>> {
    let versions = desk(&fleet).await?.instructions_versions();
    Ok(versions.into_iter().map(Into::into).collect())
}

/// The agent files on every online machine: those of the mesh read directly,
/// then the server's other machines.
#[tauri::command]
pub async fn agent_files_read(
    beep: Service<'_>,
    fleet: FleetState<'_>,
    machines: Machines<'_>,
) -> Result<Vec<MachineAgentFiles>> {
    let newest = desk(&fleet).await.ok().and_then(|desk| desk.instructions());
    let text = newest.as_ref().map(|version| version.text.as_str());
    let mut files = direct::agent_files(&machines, text).await;
    let others = extra(beep.call(ReadAgentFiles {}).await);
    files.extend(
        others
            .into_iter()
            .filter(|machine| machines.member(&machine.machine_id).is_none()),
    );
    Ok(files)
}

/// Writes the newest version into the agent files on `machine_id`.
#[tauri::command]
pub async fn agent_files_sync(
    beep: Service<'_>,
    fleet: FleetState<'_>,
    machines: Machines<'_>,
    machine_id: String,
) -> Result<MachineAgentFiles> {
    let Some(machine) = machines.member(&machine_id) else {
        return beep
            .call(SyncAgentFiles { machine_id })
            .await
            .map_err(outdated);
    };
    let newest = desk(&fleet).await?.instructions().ok_or_else(|| {
        DesktopError::BadRequest(
            "Save your instructions before writing them to agent files.".into(),
        )
    })?;
    direct::sync_agent_files(&machines, machine, &newest.text).await
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
