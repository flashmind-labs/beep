//! Steering a running coding-agent session: a prompt with pasted images, a
//! message for the running turn, switching model, cancel and close.

use crate::{
    agents::{kept_transcripts::SessionKey, outbox, sessions::closed},
    error::{DesktopError, Result},
    fleet::{Machines, agents as direct, asked::ORDER_LIMIT},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use beep_proto::{
    AgentSteer, ModelId, SteerAgentSession, Upload, check_agent_images, image_key,
    node::agent::Effort,
};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, WebviewWindow};

/// What `agent_steer` does to a session, as the page sends it.
#[derive(Clone, Debug, Deserialize)]
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
    /// Stops the main agent's turn; its subagents and background commands keep running.
    Cancel,
    /// Stops the turn, its subagents, background commands and the agents it started.
    StopAll,
    Close,
    /// Switches the running session's model or thinking from its next turn.
    Configure {
        #[serde(default)]
        model: Option<ModelId>,
        #[serde(default)]
        effort: Option<Effort>,
    },
    /// Switches the running session's mode, one `agent_modes` lists.
    Mode {
        mode: String,
    },
    /// Stops one of the session's background tasks, by the id
    /// `agent_tasks` gives, without ending the turn.
    StopTask {
        task: String,
    },
}

/// Why `agent_steer` failed, as the page tells it apart:
/// `{ kind: "lost" | "unsure" | "waiting" | "failed", message }`.
#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SteerError {
    /// The machine could not be reached: nothing went, send again.
    Lost { message: String },
    /// The link dropped or timed out once the order went: the machine may
    /// have taken it, so read the session before sending again.
    Unsure { message: String },
    /// The machine could not be reached, and the message waits in this
    /// device's outbox until it can.
    Waiting { message: String },
    /// What the machine or the app said.
    Failed { message: String },
}

impl From<DesktopError> for SteerError {
    fn from(e: DesktopError) -> Self {
        let message = e.to_string();
        if e.is_transport() {
            Self::Lost { message }
        } else if matches!(e, DesktopError::Unsure(_) | DesktopError::TimedOut(_)) {
            Self::Unsure { message }
        } else {
            Self::Failed { message }
        }
    }
}

/// An image as the page sends it: its bytes in base64.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PastedImage {
    pub(crate) name: String,
    pub(crate) mime: String,
    pub(crate) data: String,
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
            Steer::StopAll => Self::StopAll {},
            Steer::Close => Self::Close {},
            Steer::Configure { model, effort } => Self::Configure {
                model,
                effort: effort.map(Into::into),
            },
            Steer::Mode { mode } => Self::SetMode { mode },
            Steer::StopTask { task } => Self::StopTask { task },
        })
    }
}

/// The pasted images as uploads, checked against the limits.
pub(crate) fn uploads(images: Vec<PastedImage>) -> Result<Vec<Upload>> {
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

/// Whether a node of `version` reads `StopAll`: one after 0.5.0. A node
/// that has not said its version is taken as older.
fn stops_all(version: Option<&str>) -> bool {
    version
        .and_then(|version| semver::Version::parse(version).ok())
        .is_some_and(|version| version > semver::Version::new(0, 5, 0))
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn agent_steer(
    app: AppHandle,
    window: WebviewWindow,
    fleet: Machines<'_>,
    machine_id: String,
    agent: String,
    session: String,
    steer: Steer,
    // What the window saw of the session when it sent a message.
    seen: Option<outbox::Seen>,
) -> Result<Vec<String>, SteerError> {
    // A message goes after any waiting for its machine, and waits on this
    // device while the machine cannot be reached.
    if matches!(steer, Steer::Prompt { .. } | Steer::InTurn { .. }) {
        fleet.machine(&machine_id)?;
        AgentSteer::try_from(steer.clone())?;
        let key = SessionKey {
            machine_id,
            agent,
            session,
        };
        return outbox::send(&app, window.label(), key, steer, seen).await;
    }

    Ok(sent(
        app,
        window.label(),
        fleet,
        machine_id,
        agent,
        session,
        steer,
    )
    .await?)
}

// The arguments are those of `agent_steer`.
pub(crate) async fn sent(
    app: AppHandle,
    window: &str,
    fleet: Machines<'_>,
    machine_id: String,
    agent: String,
    session: String,
    steer: Steer,
) -> Result<Vec<String>> {
    let steer = AgentSteer::try_from(steer)?;
    let indexed_prompt = match &steer {
        AgentSteer::Prompt { text }
        | AgentSteer::PromptImages { text, .. }
        | AgentSteer::Steer { text, .. } => Some(text.clone()),
        _ => None,
    };
    // The keys of the images sent, which the session's log names them by.
    let keys: Vec<String> = match &steer {
        AgentSteer::PromptImages { images, .. } | AgentSteer::Steer { images, .. } => {
            images.iter().map(|image| image_key(&image.data)).collect()
        }
        _ => Vec::new(),
    };

    let machine = fleet.machine(&machine_id)?;
    let steer = match steer {
        // A node that cannot read it would end its whole event stream.
        AgentSteer::StopAll {} if !stops_all(fleet.status().version(machine).as_deref()) => {
            AgentSteer::Cancel {}
        }
        steer => steer,
    };
    let close = matches!(steer, AgentSteer::Close {});
    let operation = beep_proto::node::events::Operation::Agent {
        agent: agent.clone(),
        session: session.clone(),
        request: beep_mesh::machine::node_request(steer.clone())?,
    };

    if let Some(reply) = app
        .state::<crate::fleet::events::Events>()
        .call(window, machine, operation)
        .await?
    {
        let beep_proto::node::events::CallReply::Agent { info } = reply else {
            return Err(DesktopError::BadRequest(
                "unexpected session acknowledgement".into(),
            ));
        };

        if let Some(prompt) = &indexed_prompt {
            let host = beep_mesh::machine::Host {
                id: machine_id.clone(),
                name: fleet.name_of(machine),
            };
            crate::palette::prompts::sent(
                &app,
                beep_mesh::machine::beep_session(&host, *info),
                prompt,
            )
            .await;
        }

        if close {
            closed(&app, &machine_id, &session);
        }

        return Ok(keys);
    }

    let order = SteerAgentSession {
        machine_id: machine_id.clone(),
        agent,
        session: session.clone(),
        steer,
    };
    let sent = direct::steer(&fleet, machine, order);
    let opened = fleet
        .asked(
            machine,
            &session,
            "sending to the session",
            ORDER_LIMIT,
            sent,
        )
        .await?;
    if let Some(prompt) = &indexed_prompt {
        let host = beep_mesh::machine::Host {
            id: machine_id.clone(),
            name: fleet.name_of(machine),
        };
        let metadata = beep_mesh::machine::beep_session(&host, opened.info.clone());
        crate::palette::prompts::sent(&app, metadata, prompt).await;
    }

    if close {
        // The lists show it ended once the machine says so, about two
        // seconds on; bounded by `until_ended`, nothing else owns it.
        let fleet = fleet.inner().clone();
        tauri::async_runtime::spawn(async move {
            direct::until_ended(&fleet, machine, opened).await;
            closed(&app, &machine_id, &session);
        });
    }

    Ok(keys)
}

#[cfg(test)]
#[path = "../tests/agents_steer.rs"]
mod tests;
