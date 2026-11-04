//! Picks the machine a coding agent starts on when the user did not.

use super::Live;
use crate::protocol::*;
use beep_mesh::{
    PeerClient,
    machine::auto::{self, AutoHost},
};

/// The machine Auto picks for `start`, and why.
pub(super) async fn auto(
    hosts: Vec<Live>,
    start: &StartAgentSession,
    prompt: &str,
) -> Result<(Live, String), ApiError> {
    let asking: Vec<AutoHost> = hosts
        .iter()
        .map(|live| AutoHost {
            host: live.host.clone(),
            client: PeerClient::new(live.online.connection.clone()),
        })
        .collect();
    let choice = auto::pick(&asking, start, prompt).await?;
    let live = hosts
        .into_iter()
        .nth(choice.at)
        .ok_or(ApiError::Internal {})?;
    Ok((live, choice.why))
}
