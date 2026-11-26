//! What a steer is and how one is sent: the page's `Steer`, its conversion to the node's request,
//! the images it carries, and the direct send that the outbox and `agent_steer` share.

use crate::{
    agents::kept_folder::PastedImage,
    error::{DesktopError, Result},
    fleet::{
        Machines, agents as direct,
        asked::{BEFORE_SET_DEVICE, BEFORE_STOP_ALL, ORDER_LIMIT, newer_than},
        session_mirror::closed,
    },
};
use base64::{Engine, engine::general_purpose::STANDARD};
use beep_proto::SessionId;
use beep_proto::{
    AgentSteer, ModelId, SteerAgentSession, Upload, check_agent_images, image_key,
    node::{
        agent::{Effort, SessionDevice},
        device::DeviceId,
    },
};
use iroh::EndpointId;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tauri::{AppHandle, Manager};

const DROP_STEERS_LIMIT: Duration = Duration::from_secs(3);

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
        fast: Option<bool>,
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
    /// Keeps the device the session uses, or forgets it.
    Device {
        device: Option<DevicePick>,
    },
}

/// A device as the page names it: its machine and its id there.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct DevicePick {
    pub(crate) machine: String,
    pub(crate) device: String,
}

impl TryFrom<DevicePick> for SessionDevice {
    type Error = DesktopError;

    fn try_from(pick: DevicePick) -> Result<Self> {
        let machine = pick
            .machine
            .parse::<EndpointId>()
            .map_err(|_| DesktopError::BadRequest(format!("no machine {}", pick.machine)))?;
        Ok(Self {
            machine,
            device: DeviceId(pick.device),
        })
    }
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
            Steer::Configure {
                model,
                effort,
                fast: None,
            } => Self::Configure { model, effort },
            Steer::Configure {
                model,
                effort,
                fast: Some(fast),
            } => Self::ConfigureFast {
                model,
                effort,
                fast,
            },
            Steer::Mode { mode } => Self::SetMode { mode },
            Steer::StopTask { task } => Self::StopTask { task },
            Steer::Device { device } => Self::SetDevice {
                device: device.map(SessionDevice::try_from).transpose()?,
            },
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

// The arguments are those of `agent_steer`.
pub(crate) async fn sent(
    app: AppHandle,
    window: &str,
    fleet: Machines<'_>,
    machine_id: String,
    agent: String,
    session: SessionId,
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

    let machine = fleet.machine(&machine_id)?;
    let version = fleet.status().version(machine);
    // A node that cannot read a request would end its whole event stream.
    let steer = match steer {
        AgentSteer::StopAll {} if !newer_than(version.as_deref(), &BEFORE_STOP_ALL) => {
            AgentSteer::Cancel {}
        }
        AgentSteer::SetDevice { .. } if !newer_than(version.as_deref(), &BEFORE_SET_DEVICE) => {
            return Err(DesktopError::BadRequest(
                "beep-node there is too old to keep a session's device".into(),
            ));
        }
        steer => steer,
    };
    let close = matches!(steer, AgentSteer::Close {});
    let limit = if matches!(steer, AgentSteer::DropSteers {}) {
        DROP_STEERS_LIMIT
    } else {
        ORDER_LIMIT
    };
    let operation = beep_proto::node::events::Operation::Agent {
        agent: agent.clone(),
        session: session.clone(),
        request: beep_mesh::machine::node_request(steer.clone())?,
    };
    let events = app.state::<crate::fleet::events::Events>();
    let outbox = app.state::<crate::fleet::outbox::WindowOutbox>();
    let called = if window.is_empty() {
        events.call_machine(machine, operation, limit).await?
    } else {
        match events
            .call(&outbox, window, machine, operation.clone())
            .await?
        {
            Some(reply) => Some(reply),
            None => events.call_machine(machine, operation, limit).await?,
        }
    };

    if let Some(reply) = called {
        let beep_proto::node::events::CallReply::Agent { .. } = reply else {
            return Err(DesktopError::BadRequest(
                "unexpected session acknowledgement".into(),
            ));
        };

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
        .asked(machine, &session, "sending to the session", limit, sent)
        .await?;

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
#[path = "../tests/agents_steering.rs"]
mod tests;
