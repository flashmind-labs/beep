//! The caller's machines, as the handlers look them up.

use crate::{nodes::Online, protocol::ApiError, router::Cx};
use std::collections::{HashMap, HashSet};

/// The caller's machine `endpoint`, while it is online as them.
pub(super) fn online(cx: &Cx, endpoint: &str) -> Option<Online> {
    cx.app
        .nodes
        .online(endpoint)
        .filter(|node| node.user == cx.user.id())
}

/// The caller's machines' names, by key.
pub(super) async fn machine_names(cx: &Cx) -> Result<HashMap<String, String>, ApiError> {
    Ok(cx
        .user
        .nodes()
        .await?
        .into_iter()
        .map(|node| (node.endpoint, node.name))
        .collect())
}

/// The keys linked to the caller's account.
pub(super) async fn linked_keys(cx: &Cx) -> Result<HashSet<String>, ApiError> {
    Ok(cx
        .user
        .account_keys()
        .await?
        .into_iter()
        .map(|key| key.endpoint_id)
        .collect())
}
