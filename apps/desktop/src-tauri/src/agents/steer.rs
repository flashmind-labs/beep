//! Steering a running coding-agent session: a prompt with pasted images, a
//! message for the running turn, switching model, cancel and close.

use crate::{
    beep::Service,
    error::{DesktopError, Result},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use beep_proto::{
    AgentSteer, ApiError, ModelId, SteerAgentSession, Upload, check_agent_images, image_key,
    node::agent::Effort,
};
use serde::Deserialize;

/// What `agent_steer` does to a session, as the page sends it.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Steer {
    Prompt {
        text: String,
        /// Pasted or dropped images, sent after the text.
        #[serde(default)]
        images: Vec<PastedImage>,
    },
    /// A message for the running turn, delivered once the agent's current
    /// tool call finishes.
    #[serde(rename = "steer")]
    InTurn {
        text: String,
        #[serde(default)]
        images: Vec<PastedImage>,
    },
    /// Takes back the steers not yet delivered.
    DropSteers,
    Cancel,
    Close,
    /// Switches the running session's model or thinking from its next turn.
    Configure {
        #[serde(default)]
        model: Option<ModelId>,
        #[serde(default)]
        effort: Option<Effort>,
    },
}

/// An image as the page sends it: its bytes in base64.
#[derive(Debug, Deserialize)]
pub struct PastedImage {
    name: String,
    mime: String,
    data: String,
}

impl TryFrom<Steer> for AgentSteer {
    type Error = DesktopError;

    /// Images go as `PromptImages`, checked against its limits here first;
    /// a message without them goes as `Prompt`, which every server reads.
    fn try_from(steer: Steer) -> Result<Self> {
        Ok(match steer {
            Steer::Prompt { text, images } if images.is_empty() => Self::Prompt { text },
            Steer::Prompt { text, images } => Self::PromptImages {
                text,
                images: uploads(images)?,
            },
            Steer::InTurn { text, images } => Self::Steer {
                text,
                images: uploads(images)?,
            },
            Steer::DropSteers => Self::DropSteers {},
            Steer::Cancel => Self::Cancel {},
            Steer::Close => Self::Close {},
            Steer::Configure { model, effort } => Self::Configure {
                model,
                effort: effort.map(Into::into),
            },
        })
    }
}

/// The pasted images as uploads, checked against the limits.
fn uploads(images: Vec<PastedImage>) -> Result<Vec<Upload>> {
    let images = images
        .into_iter()
        .map(|image| -> Result<Upload> {
            let data = STANDARD.decode(&image.data).map_err(|_| {
                DesktopError::BadRequest(format!("{} could not be read", image.name))
            })?;
            Ok(Upload {
                name: image.name,
                mime: image.mime,
                data,
            })
        })
        .collect::<Result<Vec<Upload>>>()?;
    check_agent_images(&images)?;
    Ok(images)
}

/// The same message as a prompt for after the turn, for a server from
/// before steering, which reads op 5 of `AgentSteer` as unknown.
fn queued_instead(steer: &AgentSteer) -> Option<AgentSteer> {
    let AgentSteer::Steer { text, images } = steer else {
        return None;
    };
    Some(if images.is_empty() {
        AgentSteer::Prompt { text: text.clone() }
    } else {
        AgentSteer::PromptImages {
            text: text.clone(),
            images: images.clone(),
        }
    })
}

/// True when the server refused one of `ops` of `AgentSteer` as unknown:
/// 3 `PromptImages`, 4 `Configure`, 5 and 6 `Steer`.
fn older_than(error: &DesktopError, ops: &[u8]) -> bool {
    matches!(error, DesktopError::Api(ApiError::BadRequest { message })
        if ops.iter().any(|op| message.contains(&format!("unknown op {op}"))))
}

#[tauri::command]
pub async fn agent_steer(
    beep: Service<'_>,
    machine_id: String,
    agent: String,
    session: String,
    steer: Steer,
) -> Result<Vec<String>> {
    let steer = AgentSteer::try_from(steer)?;
    // The keys of the images sent, which the session's log names them by.
    let keys: Vec<String> = match &steer {
        AgentSteer::PromptImages { images, .. } | AgentSteer::Steer { images, .. } => {
            images.iter().map(|image| image_key(&image.data)).collect()
        }
        _ => Vec::new(),
    };
    let images = !keys.is_empty();
    let configure = matches!(steer, AgentSteer::Configure { .. });
    let fallback = queued_instead(&steer);
    let call = |steer: AgentSteer| SteerAgentSession {
        machine_id: machine_id.clone(),
        agent: agent.clone(),
        session: session.clone(),
        steer,
    };
    let sent = match (beep.call(call(steer)).await, fallback) {
        // A server from before steering takes it as a message for after
        // the turn, which is the nearest it can do.
        (Err(e), Some(prompt)) if older_than(&e, &[5, 6]) => beep.call(call(prompt)).await,
        (sent, _) => sent,
    };
    match sent {
        Err(e) if images && older_than(&e, &[3]) => Err(DesktopError::BadRequest(
            "This Beep server does not take images yet. Send the message without them.".into(),
        )),
        Err(e) if configure && older_than(&e, &[4]) => Err(DesktopError::BadRequest(
            "This Beep server cannot switch a running session's model yet.".into(),
        )),
        Err(e) if older_than(&e, &[5, 6]) => Err(DesktopError::BadRequest(
            "This Beep server cannot take a message back yet.".into(),
        )),
        other => other.map(|()| keys),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pasted(data: &[u8]) -> PastedImage {
        PastedImage {
            name: "shot.png".into(),
            mime: "image/png".into(),
            data: STANDARD.encode(data),
        }
    }

    #[test]
    fn a_message_without_images_goes_as_text() {
        let steer = Steer::Prompt {
            text: "hi".into(),
            images: Vec::new(),
        };
        assert_eq!(
            AgentSteer::try_from(steer).unwrap(),
            AgentSteer::Prompt { text: "hi".into() }
        );
    }

    #[test]
    fn images_go_as_prompt_images_within_the_limits() {
        let png = b"\x89PNG\r\n\x1a\n".to_vec();
        let steer = Steer::Prompt {
            text: "look".into(),
            images: vec![pasted(&png)],
        };
        let AgentSteer::PromptImages { images, .. } = AgentSteer::try_from(steer).unwrap() else {
            panic!("an image goes as PromptImages");
        };
        assert_eq!(images[0].data, png);
        let six = Steer::Prompt {
            text: "look".into(),
            images: (0..6).map(|_| pasted(&png)).collect(),
        };
        assert!(AgentSteer::try_from(six).is_err());
    }

    #[test]
    fn a_steer_carries_its_images_and_falls_back_to_a_prompt() {
        let png = b"\x89PNG\r\n\x1a\n".to_vec();
        let steer = AgentSteer::try_from(Steer::InTurn {
            text: "use the fixture".into(),
            images: vec![pasted(&png)],
        })
        .unwrap();
        let AgentSteer::Steer { images, .. } = &steer else {
            panic!("a steer goes as Steer");
        };
        assert_eq!(images[0].data, png);
        assert!(matches!(
            queued_instead(&steer),
            Some(AgentSteer::PromptImages { text, .. }) if text == "use the fixture"
        ));
        let old = DesktopError::Api(ApiError::bad_request("unknown op 5"));
        assert!(older_than(&old, &[5, 6]));
    }

    #[test]
    fn an_old_server_is_told_apart() {
        let old = DesktopError::Api(ApiError::bad_request("unknown op 3"));
        assert!(older_than(&old, &[3]));
        assert!(!older_than(
            &DesktopError::Api(ApiError::bad_request("The message is empty.")),
            &[3]
        ));
    }
}
