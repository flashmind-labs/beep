//! The caller's machines, which the server no longer reaches: released apps still ask.

use super::Reply;
use crate::{api::router::Cx, protocol::ApiError};
use std::collections::{HashMap, HashSet};

/// What a released app hears when it asks the server to act on a machine.
pub(super) const RETIRED: &str = "Machines are reached through a desktop now.";

/// The answer to a request that acted on a machine through the server.
pub(super) fn retired<T>() -> Reply<T> {
    Err(ApiError::bad_request(RETIRED))
}

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
