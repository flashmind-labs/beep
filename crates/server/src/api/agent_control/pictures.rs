//! The pictures of a coding agent session's log, whole or made small for it.

use super::live_host;
use crate::{
    account::user::User,
    api::{Reply, router::Cx},
    protocol::*,
    store::{AgentImageBytes, AgentImageOf},
};
use beep_mesh::machine::thumb::{Picture, thumb_of};
use std::sync::Arc;
use tokio::sync::Semaphore;

/// Copies being kept at once...
static SAVING: Semaphore = Semaphore::const_new(2);
/// ...and the MiB of copies waiting or being kept. Past this a copy is not
/// kept: the node still has the picture for the next time it is asked for.
static SAVE_ROOM: Semaphore = Semaphore::const_new(256);

/// The kept copy of the picture, or else the node's, which is then kept.
pub(in crate::api) async fn get_agent_image(cx: Cx, get: GetAgentImage) -> Reply<AgentImageData> {
    let image = full_picture(&cx, &get.machine_id, &get.session, &get.key).await?;
    Ok(AgentImageData {
        key: get.key,
        mime: image.mime,
        data: image.data,
    })
}

/// `get_agent_image`'s picture, made small.
pub(in crate::api) async fn get_agent_thumb(cx: Cx, get: GetAgentThumb) -> Reply<AgentImageData> {
    let image = full_picture(&cx, &get.machine_id, &get.session, &get.key).await?;
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

async fn full_picture(
    cx: &Cx,
    machine_id: &str,
    session: &str,
    key: &str,
) -> Result<AgentImageBytes, ApiError> {
    if !is_image_key(key) {
        return Err(ApiError::bad_request("That is not an image key."));
    }
    match cx.user.agent_image(key).await {
        Ok(Some(kept)) => return Ok(kept),
        Ok(None) => {}
        // The node may still have it.
        Err(e) => log::warn!("agent image not read; key={key} error={e:#}"),
    }
    let live = live_host(cx, machine_id).await?;
    let image = live
        .online
        .agent_image(session, key)
        .await
        .map_err(|e| {
            // A node from before images by key closes the stream.
            ApiError::bad_request(format!(
                "Could not read the image from {}: {e:#}. Updating beep-node there may help.",
                live.host.machine_name
            ))
        })?
        .ok_or(ApiError::NotFound {})?;
    if image_key(&image.data) != key {
        log::warn!("node gave another image; session={session} key={key}");
        return Err(ApiError::NotFound {});
    }
    let image = AgentImageBytes {
        mime: image.mime,
        data: image.data,
    };
    keep_later(&cx.user, machine_id, session, key, &image);
    Ok(image)
}

/// Keeps a copy of `image` without holding up the reply. The task owns all it
/// needs, ends with the save and logs its failure.
fn keep_later(
    user: &Arc<User>,
    machine_id: &str,
    session: &str,
    key: &str,
    image: &AgentImageBytes,
) {
    let mib = u32::try_from(image.data.len() >> 20)
        .unwrap_or(u32::MAX)
        .saturating_add(1);
    let Ok(room) = SAVE_ROOM.try_acquire_many(mib) else {
        log::warn!("agent image not kept, too many waiting; key={key}");
        return;
    };
    let (user, machine_id, session, key, image) = (
        user.clone(),
        machine_id.to_owned(),
        session.to_owned(),
        key.to_owned(),
        image.clone(),
    );
    tokio::spawn(async move {
        let _room = room;
        let saved = match SAVING.acquire().await {
            Ok(_saving) => {
                let of = AgentImageOf {
                    machine_id: &machine_id,
                    session: &session,
                };
                user.save_agent_image(of, &image).await
            }
            Err(e) => Err(e.into()),
        };
        if let Err(e) = saved {
            log::warn!("agent image not kept; key={key} error={e:#}");
        }
    });
}

/// Whether `key` is what `image_key` makes: 16 lowercase hex digits.
fn is_image_key(key: &str) -> bool {
    key.len() == 16 && key.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}
