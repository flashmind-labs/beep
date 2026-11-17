//! Commands left running by a session, read and stopped on its own machine,
//! and the process of a session started outside Beep, ended to continue it here.

use crate::{
    error::{DesktopError, Result},
    fleet::{Machines, asked::SMALL_LIMIT},
};
use beep_mesh::MeshError;
use beep_proto::SessionId;
use beep_proto::node::agent::{RunningProcess, SESSIONS_PROTOCOL, SessionsReply, SessionsRequest};
use serde::Serialize;
use std::time::Duration;
use tokio::time::error::Elapsed;

#[derive(Serialize)]
pub struct ProcessesView {
    processes: Vec<RunningProcess>,
}

#[tauri::command]
pub async fn agent_processes(
    fleet: Machines<'_>,
    machine_id: String,
    session: SessionId,
) -> Result<ProcessesView> {
    let Some(machine) = fleet.member(&machine_id) else {
        return Ok(ProcessesView {
            processes: Vec::new(),
        });
    };
    let read = async {
        let request = SessionsRequest::Processes {
            session: session.clone(),
        };
        let client = fleet.client(machine).await?;
        Ok::<_, DesktopError>(
            client
                .call::<_, SessionsReply>(SESSIONS_PROTOCOL, &request)
                .await?,
        )
    };
    let step = "reading the session's running commands";
    let reply = fleet
        .asked(machine, &session, step, SMALL_LIMIT, read)
        .await?;
    match reply {
        SessionsReply::Processes { processes } => Ok(ProcessesView { processes }),
        SessionsReply::Error { message } => Err(DesktopError::BadRequest(message)),
        _ => Err(DesktopError::BadRequest(
            "The machine cannot list running commands yet.".into(),
        )),
    }
}

#[tauri::command]
pub async fn agent_stop_process(
    fleet: Machines<'_>,
    machine_id: String,
    session: SessionId,
    pid: u32,
) -> Result<()> {
    let machine = fleet.member(&machine_id).ok_or_else(|| {
        DesktopError::BadRequest("The session's machine is not in the mesh.".into())
    })?;
    let reply = fleet
        .client(machine)
        .await?
        .call_within::<_, SessionsReply>(
            Duration::from_secs(10),
            SESSIONS_PROTOCOL,
            &SessionsRequest::StopProcess { session, pid },
        )
        .await?;
    match reply {
        SessionsReply::ProcessStopped {} => Ok(()),
        SessionsReply::Error { message } => Err(DesktopError::BadRequest(message)),
        _ => Err(DesktopError::BadRequest(
            "The machine cannot stop running commands yet.".into(),
        )),
    }
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
    let reply = fleet
        .client(machine)
        .await?
        .call_within::<_, SessionsReply>(
            Duration::from_secs(20),
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
