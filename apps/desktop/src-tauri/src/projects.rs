//! Projects imported onto the user's machines: the Import a project sheet
//! and the rows the sidebar, Home and the Projects page list.

use crate::{
    beep::{Beep, epoch_ms},
    error::{DesktopError, Result},
};
use beep_proto::{ApiError, ImportProject, ImportedProject};
use serde::Serialize;
use std::{sync::Arc, time::Duration};
use tauri::State;

type Service<'a> = State<'a, Arc<Beep>>;

/// What an old server answers a request it does not know.
const UNKNOWN_OP: &str = "unknown op";
/// The server stops a clone after 30 minutes; this leaves it time to say so.
const IMPORT_TIMEOUT: Duration = Duration::from_secs(31 * 60);

/// One imported project as the window shows it.
#[derive(Debug, Serialize)]
pub struct ProjectRow {
    /// `org/repo`.
    name: String,
    url: String,
    machine_id: String,
    machine_name: String,
    folder: String,
    cloned: bool,
    /// In milliseconds since the Unix epoch.
    imported_at: u64,
}

impl From<ImportedProject> for ProjectRow {
    fn from(p: ImportedProject) -> Self {
        Self {
            name: p.project,
            url: p.url,
            machine_id: p.machine_id,
            machine_name: p.machine_name,
            folder: p.folder,
            cloned: p.cloned,
            imported_at: epoch_ms(p.imported_at),
        }
    }
}

/// Clones `repo` onto `machine_id`, or the machine the server picks.
#[tauri::command]
pub async fn import_project(
    beep: Service<'_>,
    repo: String,
    machine_id: Option<String>,
) -> Result<ProjectRow> {
    let machine_id = machine_id.filter(|id| !id.is_empty());
    beep.call_within(ImportProject { repo, machine_id }, IMPORT_TIMEOUT)
        .await
        .map(ProjectRow::from)
        .map_err(outdated)
}

/// An old server's refusal, in words the sheet shows; any other refusal
/// is the server's own sentence.
fn outdated(e: DesktopError) -> DesktopError {
    match e {
        DesktopError::Api(ApiError::BadRequest { message }) if message.starts_with(UNKNOWN_OP) => {
            DesktopError::BadRequest(
                "The server needs an update before projects can be imported.".into(),
            )
        }
        DesktopError::Api(ApiError::BadRequest { message }) => DesktopError::BadRequest(message),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refusals_read_as_the_servers_sentence() {
        let old = DesktopError::Api(ApiError::bad_request("unknown op 166"));
        assert!(outdated(old).to_string().contains("needs an update"));
        let said = DesktopError::Api(ApiError::bad_request("build-box is offline."));
        assert_eq!(outdated(said).to_string(), "build-box is offline.");
    }
}
