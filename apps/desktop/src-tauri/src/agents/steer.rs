//! Steering a running coding-agent session: a prompt with pasted images, a
//! message for the running turn, switching model, cancel and close.

use crate::{
    agents::{
        kept_transcripts::SessionKey,
        outbox::{self, OrderKind},
        starts,
        steering::{Steer, SteerError, sent},
    },
    error::{DesktopError, Result},
    fleet::{
        Machines,
        asked::{BEFORE_SET_DEVICE, newer_than},
        session_mirror::{StartId, closed},
    },
};
use beep_proto::{AgentSteer, SessionId, node::agent::SessionDevice};
use tauri::{AppHandle, WebviewWindow};

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
