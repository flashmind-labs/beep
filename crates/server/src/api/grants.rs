//! Grants: the standing permissions the user gave routines.

use super::{Reply, found};
use crate::{api::router::Cx, apps::grants, protocol::*};
use uuid::Uuid;

pub(super) async fn list_grants(cx: Cx, _: ListGrants) -> Reply<Vec<Grant>> {
    Ok(cx
        .user
        .live_grants()
        .await?
        .into_iter()
        .map(grants::listed)
        .collect())
}

pub(super) async fn revoke_grant(cx: Cx, RevokeGrant { id }: RevokeGrant) -> Reply<()> {
    let id = Uuid::parse_str(id.trim()).map_err(|_| ApiError::NotFound {})?;
    found(cx.user.revoke_grant(id).await?)
}
