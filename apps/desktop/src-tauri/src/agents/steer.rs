//! Steering a running coding-agent session: a prompt with pasted images, a
//! message for the running turn, switching model, cancel and close.

use crate::{
    agents::{
        kept_folder::PastedImage,
        kept_transcripts::SessionKey,
        outbox::{self, OrderKind},
        sessions::closed,
        starts::{self, StartId},
    },
    error::{DesktopError, Result},
    fleet::{Machines, agents as direct, asked::ORDER_LIMIT},
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
use semver::Version;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tauri::{AppHandle, Manager, WebviewWindow};

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

/// The last node that reads no `StopAll`.
pub(crate) const BEFORE_STOP_ALL: Version = Version::new(0, 5, 0);
/// The last node that reads no `SetDevice`.
pub(crate) const BEFORE_SET_DEVICE: Version = Version::new(0, 6, 28);
/// The last released node, which starts no home assistant. This computer's
/// own node is the desktop's build and is not held to it.
pub(crate) const BEFORE_ASSISTANT: Version = Version::new(0, 7, 12);
/// The last released node, which runs no named account. This computer's own
/// node is the desktop's build and is not held to it.
pub(crate) const BEFORE_ACCOUNTS: Version = Version::new(0, 8, 4);

/// The last released node, which moves no chat. This computer's own node is
/// the desktop's build and is not held to it.
pub(crate) const BEFORE_CHAT_MOVES: Version = Version::new(0, 8, 8);

/// Whether a node of `version` is newer than `last`, the last release
/// without a request. A node that has not said its version is taken as older.
pub(crate) fn newer_than(version: Option<&str>, last: &Version) -> bool {
    version
        .and_then(|version| Version::parse(version).ok())
        .is_some_and(|version| version > *last)
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn agent_steer(
    app: AppHandle,
    window: WebviewWindow,
    fleet: Machines<'_>,
    machine_id: String,
    agent: String,
    session: SessionId,
    steer: Steer,
    // What the window saw of the session when it sent a message.
    seen: Option<outbox::Seen>,
    // Files the message uploads before it goes, with the paths it mentions.
    attach: Option<outbox::Attach>,
    // Set for a session that ended: it resumes before the message goes.
    resume: Option<outbox::Resume>,
) -> Result<Vec<String>, SteerError> {
    if let Ok(start) = session.as_ref().parse::<StartId>() {
        return starts::steer(&app, start, &steer);
    }

    // An id the session was resumed from stands for the one that runs now, which
    // a message to it does not resume again.
    let asked = SessionKey {
        machine_id,
        agent,
        session,
    };
    let key = outbox::resolved(&app, &asked);
    let resume = resume.filter(|_| key == asked);

    // A message goes after any waiting for its machine, and waits on this
    // device while the machine cannot be reached.
    if matches!(steer, Steer::Prompt { .. } | Steer::InTurn { .. }) {
        fleet.machine(&key.machine_id)?;
        AgentSteer::try_from(steer.clone())?;
        return Err(outbox::send(
            &app,
            window.label(),
            key,
            steer,
            seen,
            attach.unwrap_or_default(),
            resume,
        ));
    }

    if let Some(kind) = actor_order(&fleet, &key.machine_id, &steer)? {
        let close = matches!(kind, OrderKind::Close);
        let (machine_id, session) = (key.machine_id.clone(), key.session.clone());
        outbox::send_order(&app, window.label(), key, kind).await?;

        if close {
            closed(&app, &machine_id, &session);
        }

        return Ok(Vec::new());
    }

    let dropping = matches!(steer, Steer::DropSteers).then(|| key.clone());
    let keys = sent(
        app.clone(),
        window.label(),
        fleet,
        key.machine_id,
        key.agent,
        key.session,
        steer,
    )
    .await?;

    // The machine holds no steers now, so no message is on its way to its queue.
    if let Some(key) = dropping {
        outbox::dropped(&app, &key);
    }

    Ok(keys)
}

fn actor_order(fleet: &Machines<'_>, machine_id: &str, steer: &Steer) -> Result<Option<OrderKind>> {
    if matches!(
        steer,
        Steer::DropSteers | Steer::Prompt { .. } | Steer::InTurn { .. }
    ) {
        return Ok(None);
    }

    let machine = fleet.machine(machine_id)?;
    let version = fleet.status().version(machine);

    Ok(Some(match steer {
        Steer::Configure {
            model,
            effort,
            fast,
        } => OrderKind::Configure {
            model: model.clone(),
            effort: *effort,
            fast: *fast,
        },
        Steer::Mode { mode } => OrderKind::Mode { mode: mode.clone() },
        Steer::Device { device } => {
            if !newer_than(version.as_deref(), &BEFORE_SET_DEVICE) {
                return Err(DesktopError::BadRequest(
                    "beep-node there is too old to keep a session's device".into(),
                ));
            }
            OrderKind::Device {
                device: device.clone().map(SessionDevice::try_from).transpose()?,
            }
        }
        Steer::Cancel => OrderKind::Cancel,
        Steer::StopAll => OrderKind::StopAll,
        Steer::Close => OrderKind::Close,
        Steer::StopTask { task } => OrderKind::StopTask { task: task.clone() },
        Steer::Prompt { .. } | Steer::InTurn { .. } | Steer::DropSteers => unreachable!(),
    }))
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
    let called = if window.is_empty() {
        events.call_machine(machine, operation, limit).await?
    } else {
        match events.call(window, machine, operation.clone()).await? {
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
#[path = "../tests/agents_steer.rs"]
mod tests;
