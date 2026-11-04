//! What a desktop in a mesh tells the server, so phones and the project agent still see it.

use super::Reply;
use crate::{api::router::Cx, protocol::ApiError, protocol::MirrorImport, store::NewImport};

/// Records an import a desktop made itself: the row `ImportProject` would
/// save, with no clone and no machine contacted.
pub(super) async fn mirror_import(cx: Cx, request: MirrorImport) -> Reply<()> {
    let linked = cx
        .user
        .nodes()
        .await?
        .iter()
        .any(|node| node.endpoint == request.machine_id);
    if !linked {
        return Err(ApiError::NotFound {});
    }
    cx.user
        .save_import(NewImport {
            project: &request.project,
            endpoint: &request.machine_id,
            folder: &request.folder,
            url: &request.url,
        })
        .await?;
    Ok(())
}
