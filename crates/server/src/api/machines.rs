//! Removing a recorded machine from the caller's account.

use super::{Reply, devices::unlink};
use crate::{api::router::Cx, protocol::*};
use iroh::EndpointId;

/// Unlinks one of the machines the account recorded.
pub(super) async fn remove_machine(
    cx: Cx,
    RemoveMachine { machine_id }: RemoveMachine,
) -> Reply<()> {
    let key: EndpointId = machine_id.parse().map_err(|_| ApiError::NotFound {})?;

    if key == cx.key {
        return Err(ApiError::bad_request(
            "This is the machine you are on. Unlink it from Settings.",
        ));
    }

    let listed = cx
        .user
        .nodes()
        .await?
        .iter()
        .any(|node| node.endpoint == machine_id);
    if !listed {
        return Err(ApiError::NotFound {});
    }

    unlink(&cx, key).await
}
