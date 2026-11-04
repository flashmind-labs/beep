//! Projects imported onto the user's machines: the sidebar's rows, the Add a
//! project sheet, a project's page and its branches, threads, runs and sync.

pub(crate) mod branches;
pub(crate) mod file_sync;
pub(crate) mod git_items;
pub(crate) mod groups;
pub(crate) mod import;
pub(crate) mod list;
pub(crate) mod live;
pub(crate) mod overview;
pub(crate) mod page;
pub(crate) mod reviews;
pub(crate) mod runs;
pub(crate) mod threads;

pub(crate) use list::ProjectRow;

use crate::error::DesktopError;
use beep_proto::ApiError;

/// What an old server answers a request it does not know.
const UNKNOWN_OP: &str = "unknown op";

/// The server did not know the request.
pub(crate) fn unknown_op(e: &DesktopError) -> bool {
    matches!(e, DesktopError::Api(ApiError::BadRequest { message }) if message.starts_with(UNKNOWN_OP))
}

/// An old server's refusal as "The server needs an update `what`."; any
/// other error as it was.
pub(crate) fn needs_update(e: DesktopError, what: &str) -> DesktopError {
    if unknown_op(&e) {
        DesktopError::BadRequest(format!("The server needs an update {what}."))
    } else {
        e
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_old_server_finds_nothing() {
        assert!(unknown_op(&DesktopError::Api(ApiError::bad_request(
            "unknown op 169"
        ))));
        assert!(!unknown_op(&DesktopError::Api(ApiError::bad_request(
            "offline"
        ))));
    }
}
