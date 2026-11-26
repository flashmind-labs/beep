//! Reads of the mesh's lists that Home and the palette share: the projects it holds, and a list a
//! machine may not know.

use crate::{
    error::{DesktopError, Result},
    fleet::{
        Fleet,
        projects::{FleetState, desk},
    },
};
use beep_proto::{ApiError, ImportedProject};

/// The projects the mesh holds, one row per machine of it that has one; none while the mesh is
/// not open, so Home never waits for it.
pub(crate) async fn mesh_imports(fleet: &Fleet, session: &FleetState<'_>) -> Vec<ImportedProject> {
    if fleet.mesh().is_none() {
        return Vec::new();
    }

    match desk(session).await {
        Ok(desk) => desk.imported().await,
        Err(e) => {
            log::info!("no projects from the mesh: {e}");
            Vec::new()
        }
    }
}

/// `None` for a request a machine does not know; any other failure goes to
/// the log.
pub(crate) fn section<T>(what: &str, result: Result<T>) -> Option<T> {
    match result {
        Ok(listed) => Some(listed),
        Err(e) if is_unsupported(&e) => None,
        Err(e) => {
            log::warn!("listing {what} for the command center: {e}");
            None
        }
    }
}

/// Whether a machine does not know the request or may not take it, as
/// opposed to a failure on the way.
pub(crate) fn is_unsupported(e: &DesktopError) -> bool {
    matches!(
        e,
        DesktopError::Api(ApiError::BadRequest { .. } | ApiError::Unauthorized {})
    )
}
