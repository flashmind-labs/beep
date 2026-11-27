//! Commands left running by a session, stopped on its own machine,
//! and the process of a session started outside Beep, ended to continue it here.

use crate::{
    error::{DesktopError, Result},
    fleet::{
        Machines,
        jobs::{FAST_JOB_WAIT, JobKind},
    },
};
use beep_mesh::MeshError;
use beep_proto::SessionId;
use beep_proto::node::agent::{SESSIONS_PROTOCOL, SessionsReply, SessionsRequest};
use std::time::Duration;
use tauri::AppHandle;
use tokio::time::{error::Elapsed, timeout};

#[tauri::command]
pub async fn agent_stop_process(
    app: AppHandle,
    fleet: Machines<'_>,
    machine_id: String,
    session: SessionId,
    pid: u32,
) -> Result<()> {
    let machine = fleet.member(&machine_id).ok_or_else(|| {
        DesktopError::BadRequest("The session's machine is not in the mesh.".into())
    })?;
    let fleet_arc = fleet.inner().clone();
    fleet
        .node_ops()
        .submit(
            Some(app),
            fleet_arc,
            machine,
            JobKind::StopProcess { session, pid },
            FAST_JOB_WAIT,
        )
        .await
        .map(|_| ())
}

/// Ends the process running `agent`'s session `session` outside Beep, which
/// its machine finds itself, so the session continues here.
#[tauri::command]
pub async fn agent_close_external(
    fleet: Machines<'_>,
    machine_id: String,
    agent: String,
    session: SessionId,
) -> Result<()> {
    let machine = fleet.member(&machine_id).ok_or_else(|| {
        DesktopError::BadRequest("The session's machine is not in the mesh.".into())
    })?;
    let client = timeout(Duration::from_secs(3), fleet.client(machine))
        .await
        .map_err(|_| {
            DesktopError::TimedOut(format!(
                "{} did not answer in time.",
                fleet.name_of(machine)
            ))
        })??;
    let reply = client
        .call_within::<_, SessionsReply>(
            Duration::from_secs(5),
            SESSIONS_PROTOCOL,
            &SessionsRequest::CloseExternal { agent, session },
        )
        .await;
    let too_old = || {
        DesktopError::BadRequest(format!(
            "Update beep-node on {} to continue this session here.",
            fleet.name_of(machine)
        ))
    };

    match reply {
        Ok(SessionsReply::ProcessStopped {}) => Ok(()),
        Ok(SessionsReply::Error { message }) => Err(DesktopError::BadRequest(message)),
        Ok(_) => Err(too_old()),
        // A node from before it closes the stream.
        Err(MeshError::Link(e)) if e.downcast_ref::<Elapsed>().is_none() => Err(too_old()),
        Err(e) => Err(e.into()),
    }
}

/// Stops Docker container `id` that session `session` started, keeping it.
/// Docker gives it ten seconds to end before it kills it.
#[tauri::command]
pub async fn agent_stop_container(
    fleet: Machines<'_>,
    machine_id: String,
    session: SessionId,
    id: String,
) -> Result<()> {
    let machine = fleet.member(&machine_id).ok_or_else(|| {
        DesktopError::BadRequest("The session's machine is not in the mesh.".into())
    })?;
    let client = timeout(Duration::from_secs(3), fleet.client(machine))
        .await
        .map_err(|_| {
            DesktopError::TimedOut(format!(
                "{} did not answer in time.",
                fleet.name_of(machine)
            ))
        })??;
    let reply = client
        .call_within::<_, SessionsReply>(
            Duration::from_secs(20),
            SESSIONS_PROTOCOL,
            &SessionsRequest::StopContainer { session, id },
        )
        .await;

    match reply {
        Ok(SessionsReply::ProcessStopped {}) => Ok(()),
        Ok(SessionsReply::Error { message }) => Err(DesktopError::BadRequest(message)),
        Ok(_) => Err(DesktopError::BadRequest(format!(
            "Update beep-node on {} to stop containers.",
            fleet.name_of(machine)
        ))),
        Err(e) => Err(e.into()),
    }
}
