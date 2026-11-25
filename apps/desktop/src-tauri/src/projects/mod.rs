//! Projects imported onto the user's machines: the sidebar's rows, the Add a
//! project sheet, a project's page and its branches, threads, runs and sync.

#[cfg(desktop)]
pub(crate) mod agent;
pub(crate) mod branches;
pub(crate) mod create;
pub(crate) mod file_sync;
pub(crate) mod git_items;
pub(crate) mod git_sync;
pub(crate) mod groups;
pub(crate) mod history;
pub(crate) mod import;
pub(crate) mod list;
pub(crate) mod live;
pub(crate) mod page;
pub(crate) mod runs;
pub(crate) mod team;
pub(crate) mod team_live;
pub(crate) mod threads;

pub(crate) use list::ProjectRow;

use crate::error::DesktopError;
use beep_proto::ApiError;

/// What an older machine answers a request it does not know.
const UNKNOWN_OP: &str = "unknown op";

/// The machine did not know the request.
pub(crate) fn unknown_op(e: &DesktopError) -> bool {
    matches!(e, DesktopError::Api(ApiError::BadRequest { message }) if message.starts_with(UNKNOWN_OP))
}

/// `None` from a machine too old to know the request.
pub(crate) fn known<T>(result: Result<T, DesktopError>) -> Result<Option<T>, DesktopError> {
    match result {
        Ok(found) => Ok(Some(found)),
        Err(e) if unknown_op(&e) => Ok(None),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
#[path = "../tests/projects.rs"]
mod tests;
