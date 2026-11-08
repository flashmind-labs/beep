//! The pictures of a coding agent session's log that the server kept, whole or made small.

use crate::{
    api::{Reply, router::Cx},
    protocol::*,
    store::AgentImageBytes,
};
use beep_mesh::machine::thumb::{Picture, thumb_of};

pub(in crate::api) async fn get_agent_image(cx: Cx, get: GetAgentImage) -> Reply<AgentImageData> {
    let image = kept_picture(&cx, &get.key).await?;
    Ok(AgentImageData {
        key: get.key,
        mime: image.mime,
        data: image.data,
    })
}

/// `get_agent_image`'s picture, made small.
pub(in crate::api) async fn get_agent_thumb(cx: Cx, get: GetAgentThumb) -> Reply<AgentImageData> {
    let image = kept_picture(&cx, &get.key).await?;
    let made = thumb_of(Picture {
        mime: image.mime,
        data: image.data,
    })
    .await;
    Ok(AgentImageData {
        key: get.key,
        mime: made.mime,
        data: made.data,
    })
}

/// The copy kept of the picture `key`; the server asks no machine for one.
async fn kept_picture(cx: &Cx, key: &str) -> Result<AgentImageBytes, ApiError> {
    if !is_image_key(key) {
        return Err(ApiError::bad_request("That is not an image key."));
    }
    cx.user.agent_image(key).await?.ok_or(ApiError::NotFound {})
}

/// Whether `key` is what `image_key` makes: 16 lowercase hex digits.
fn is_image_key(key: &str) -> bool {
    key.len() == 16 && key.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}
