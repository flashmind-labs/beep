//! Making images, videos and 3D models with the user's own API keys
//! (OpenRouter, OpenAI, Venice, Meshy). Each tool is offered only when a
//! key the user saved makes it, and every call asks first, as it spends
//! the user's money (see `crate::permissions`).

mod call;
mod job;
mod openai;
mod produce;
mod provider;
mod tool;

#[cfg(test)]
mod tests;

pub(crate) use call::approval_fields;
pub(crate) use produce::Endpoints;
pub(crate) use provider::{Keys, MediaKind};
pub(crate) use tool::Media;

use flashmind::types::tool::Tool;
use std::sync::Arc;
use tool::MediaTool;

/// The media tools `media`'s keys make, none without a key.
pub(crate) fn tools(media: Media) -> Vec<Arc<dyn Tool>> {
    let media = Arc::new(media);
    MediaKind::ALL
        .into_iter()
        .filter_map(|kind| MediaTool::new(kind, media.clone()))
        .map(|tool| Arc::new(tool) as Arc<dyn Tool>)
        .collect()
}
