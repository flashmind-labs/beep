//! Recorded machine names and linked keys used by the caller's stored data.

use crate::{api::router::Cx, protocol::ApiError};
use std::collections::{HashMap, HashSet};

/// The caller's machines' names, by key, as the account recorded them.
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
