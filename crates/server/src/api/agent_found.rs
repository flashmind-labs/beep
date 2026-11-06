//! Checkouts the user's coding agents worked in: a desktop finds them over its mesh now.

use super::Reply;
use crate::{api::router::Cx, protocol::*};

pub(super) async fn list_found_folders(_: Cx, _: ListFoundFolders) -> Reply<Vec<FoundProject>> {
    Ok(Vec::new())
}

pub(super) async fn list_project_sessions(
    _: Cx,
    _: ListProjectSessions,
) -> Reply<Vec<MachineSession>> {
    Ok(Vec::new())
}
