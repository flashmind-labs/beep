//! Steering a running coding-agent session.

use super::{
    super::{Reply, agent_sessions::limited},
    live_host,
};
use crate::{
    api::router::Cx,
    protocol::{node::agent::Request, *},
};
use beep_mesh::machine::{node_request, queued_instead};

/// Only sessions Beep runs take orders; one started outside Beep is not found.
pub(in crate::api) async fn steer_agent_session(cx: Cx, steer: SteerAgentSession) -> Reply<()> {
    // Refused before the machine is looked up.
    let request = node_request(steer.steer)?;
    let images = matches!(request, Request::PromptImages { .. });
    let configure = matches!(request, Request::Configure { .. });
    let drop_steers = matches!(request, Request::DropSteers {});
    let live = live_host(&cx, &steer.machine_id).await?;
    let listed = limited(live.online.agent_sessions()).await?;
    if !listed
        .iter()
        .any(|info| info.id == steer.session && info.agent == steer.agent)
    {
        return Err(ApiError::NotFound {});
    }
    let link = live
        .online
        .agent_session(&steer.agent, &steer.session)
        .await
        .map_err(|e| ApiError::bad_request(format!("Could not reach the session: {e:#}")))?;
    // A node from before steering takes the message for after the turn.
    let fallback = queued_instead(&request);
    let sent = match (link.send(request).await, fallback) {
        (Err(e), Some(prompt)) if format!("{e:#}").contains("unknown request") => {
            link.send(prompt).await
        }
        (sent, _) => sent,
    };
    sent.map_err(|e| {
        let error = format!("{e:#}");
        // A node from before images does not know the request.
        if images && error.contains("unknown request") {
            ApiError::bad_request(
                "beep-node on that machine is too old to send images. Update it, or send text.",
            )
        } else if configure && error.contains("unknown request") {
            ApiError::bad_request(
                "beep-node on that machine is too old to switch a running session's model. Update it.",
            )
        } else if drop_steers && error.contains("unknown request") {
            ApiError::bad_request(
                "beep-node on that machine is too old to take a message back. Update it.",
            )
        } else {
            ApiError::bad_request(error)
        }
    })?;
    Ok(())
}
