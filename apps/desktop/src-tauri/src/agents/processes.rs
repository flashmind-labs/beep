//! Commands left running by a session, read and stopped on its own machine.

use crate::{
    error::{DesktopError, Result},
    fleet::Machines,
};
use beep_proto::node::agent::{RunningProcess, SESSIONS_PROTOCOL, SessionsReply, SessionsRequest};
use serde::Serialize;
use std::time::Duration;

#[derive(Serialize)]
pub struct ProcessesView {
    processes: Vec<RunningProcess>,
    local: bool,
}

#[tauri::command]
pub async fn agent_processes(
    fleet: Machines<'_>,
    machine_id: String,
    session: String,
) -> Result<ProcessesView> {
    let Some(machine) = fleet.member(&machine_id) else {
        return Ok(ProcessesView {
            processes: Vec::new(),
            local: false,
        });
    };
    let reply = fleet
        .client(machine)
        .await?
        .call_within::<_, SessionsReply>(
            Duration::from_secs(10),
            SESSIONS_PROTOCOL,
            &SessionsRequest::Processes { session },
        )
        .await?;
    match reply {
        SessionsReply::Processes { processes } => Ok(ProcessesView {
            processes,
            local: machine == fleet.id(),
        }),
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
    session: String,
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
