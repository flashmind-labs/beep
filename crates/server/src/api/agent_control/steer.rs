//! Steering a running coding-agent session.

use super::{
    super::{Reply, agent_sessions::limited},
    live_host,
};
use crate::{
    api::router::Cx,
    protocol::{node::agent::Request, *},
};

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

/// The prompt a node from before `Request::Steer` gets for one, sent after the running turn.
pub(super) fn queued_instead(request: &Request) -> Option<Request> {
    let Request::Steer { text, images } = request else {
        return None;
    };
    Some(if images.is_empty() {
        Request::Prompt { text: text.clone() }
    } else {
        Request::PromptImages {
            text: text.clone(),
            images: images.clone(),
        }
    })
}

/// What the node is asked for `steer`; an empty message or images over the limits are refused.
pub(super) fn node_request(steer: AgentSteer) -> Result<Request, ApiError> {
    Ok(match steer {
        AgentSteer::Prompt { text } if text.trim().is_empty() => {
            return Err(ApiError::bad_request("The message is empty."));
        }
        AgentSteer::Prompt { text } => Request::Prompt { text },
        AgentSteer::PromptImages { text, images } if images.is_empty() => {
            return node_request(AgentSteer::Prompt { text });
        }
        AgentSteer::PromptImages { text, images } => {
            check_agent_images(&images)?;
            Request::PromptImages { text, images }
        }
        AgentSteer::Steer { text, images } if text.trim().is_empty() && images.is_empty() => {
            return Err(ApiError::bad_request("The message is empty."));
        }
        AgentSteer::Steer { text, images } => {
            check_agent_images(&images)?;
            Request::Steer { text, images }
        }
        AgentSteer::DropSteers {} => Request::DropSteers {},
        AgentSteer::Cancel {} => Request::Cancel {},
        AgentSteer::Close {} => Request::Close {},
        AgentSteer::Configure {
            model: None,
            effort: None,
        } => return Err(ApiError::bad_request("Nothing to switch.")),
        AgentSteer::Configure { model, effort } => Request::Configure {
            model: model
                .as_ref()
                .map(|m| m.as_str().trim())
                .filter(|m| !m.is_empty())
                .map(ModelId::from),
            effort: effort.map(Into::into),
        },
    })
}
