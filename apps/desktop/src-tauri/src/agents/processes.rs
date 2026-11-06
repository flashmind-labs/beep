//! Commands left running by a session, read and stopped on its own machine.

use crate::{
    error::{DesktopError, Result},
    fleet::{Machines, asked::SMALL_LIMIT},
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
