//! The user's instructions for every agent Beep runs, for the window's
//! Instructions view: the editor and saved versions.

use crate::{
    beep::epoch_ms,
    error::Result,
    fleet::projects::{FleetState, desk},
};
use beep_proto::InstructionsVersion;
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

#[cfg(test)]
#[path = "../tests/agents_instructions.rs"]
mod tests;
