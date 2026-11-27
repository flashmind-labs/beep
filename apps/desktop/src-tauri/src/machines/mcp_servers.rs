use crate::{
    error::{DesktopError, Result},
    fleet::{
        Fleet, Machines,
        jobs::{JobKind, JobOutcome},
    },
};
use beep_proto::node::mcp_servers::{EditableServer, Harness, PROTOCOL, Reply, Request, Server};
use futures::{StreamExt, stream};
use iroh::EndpointId;
use serde::Serialize;
use std::time::Duration;
use tauri::AppHandle;
use tokio::time::timeout;

const MCP_CALL_LIMIT: Duration = Duration::from_secs(6);
const APPLY_FAST_WAIT: Duration = Duration::from_millis(600);
const QUEUED_NOTE: &str = "Queued — will apply when online.";

#[tauri::command]
pub async fn machine_mcp_harnesses(
    fleet: Machines<'_>,
    machine_id: String,
) -> Result<Vec<Harness>> {
    match call(&fleet, fleet.machine(&machine_id)?, Request::Harnesses).await? {
        Reply::Harnesses(harnesses) => Ok(harnesses),
        _ => Err(DesktopError::BadRequest(
            "Unexpected MCP harness reply.".into(),
        )),
    }
}

#[tauri::command]
pub async fn machine_mcp_servers(
    fleet: Machines<'_>,
    machine_id: String,
    harness: Harness,
) -> Result<Vec<Server>> {
    servers(
        &fleet,
        fleet.machine(&machine_id)?,
        Request::List { harness },
    )
    .await
}

#[tauri::command]
pub async fn save_machine_mcp_server(
    fleet: Machines<'_>,
    machine_id: String,
    harness: Harness,
    previous: Option<Box<EditableServer>>,
    server: EditableServer,
) -> Result<SaveResult> {
    match call(
        &fleet,
        fleet.machine(&machine_id)?,
        Request::Save {
            harness,
            previous,
            server: Box::new(server),
        },
    )
    .await?
    {
        Reply::Servers(servers) => Ok(SaveResult::Saved { servers }),
        Reply::Conflict(servers) => Ok(SaveResult::Conflict { servers }),
        _ => Err(DesktopError::BadRequest(
            "Unexpected MCP server reply.".into(),
        )),
    }
}

#[derive(Serialize)]
#[serde(tag = "status", rename_all = "lowercase")]
pub enum SaveResult {
    Saved { servers: Vec<Server> },
    Conflict { servers: Vec<Server> },
}

#[tauri::command]
pub async fn remove_machine_mcp_server(
    fleet: Machines<'_>,
    machine_id: String,
    harness: Harness,
    name: String,
) -> Result<Vec<Server>> {
    servers(
        &fleet,
        fleet.machine(&machine_id)?,
        Request::Remove { harness, name },
    )
    .await
}

#[derive(Serialize)]
pub struct Applied {
    name: String,
    error: Option<String>,
}

/// Copies the selected server into the same harness on each other session host.
#[tauri::command]
pub async fn apply_machine_mcp_server(
    app: AppHandle,
    fleet: Machines<'_>,
    machine_id: String,
    harness: Harness,
    name: String,
) -> Result<Vec<Applied>> {
    let source = fleet.machine(&machine_id)?;
    let Reply::NativeServer(server) =
        call(&fleet, source, Request::Export { harness, name }).await?
    else {
        return Err(DesktopError::BadRequest(
            "Unexpected MCP server reply.".into(),
        ));
    };
    let hosts = fleet.ready().await?.code_hosts();
    let fleet_arc = fleet.inner().clone();
    let mut results = stream::iter(hosts.into_iter().filter(|host| host.device.id != source))
        .map(|host| {
            let app = app.clone();
            let fleet_arc = fleet_arc.clone();
            let server = server.clone();

            async move {
                let target = host.device.id;
                let host_name = host.device.name;
                let outcome = fleet_arc
                    .node_ops()
                    .submit(
                        Some(app),
                        fleet_arc.clone(),
                        target,
                        JobKind::ApplyMcpServer { harness, server },
                        APPLY_FAST_WAIT,
                    )
                    .await;
                let error = match outcome {
                    Ok(JobOutcome::Delivered) => None,
                    Ok(JobOutcome::Queued) => Some(QUEUED_NOTE.into()),
                    Err(err) => Some(err.to_string()),
                };

                Applied {
                    name: host_name,
                    error,
                }
            }
        })
        .buffer_unordered(4)
        .collect::<Vec<_>>()
        .await;
    results.sort_by(|a, b| a.name.cmp(&b.name));

    Ok(results)
}

async fn servers(fleet: &Fleet, machine: EndpointId, request: Request) -> Result<Vec<Server>> {
    match call(fleet, machine, request).await? {
        Reply::Servers(servers) => Ok(servers),
        _ => Err(DesktopError::BadRequest(
            "Unexpected MCP server reply.".into(),
        )),
    }
}

async fn call(fleet: &Fleet, machine: EndpointId, request: Request) -> Result<Reply> {
    let client = timeout(MCP_CALL_LIMIT, fleet.client(machine))
        .await
        .map_err(|_| {
            DesktopError::TimedOut(format!(
                "{} did not answer in time.",
                fleet.name_of(machine)
            ))
        })??;
    match client.call_within(MCP_CALL_LIMIT, PROTOCOL, &request).await {
        Ok(Reply::Refused(error)) => Err(DesktopError::BadRequest(error)),
        Ok(reply) => Ok(reply),
        Err(beep_mesh::MeshError::Declined { reason, .. })
            if reason == format!("this node does not offer {PROTOCOL}") =>
        {
            Err(DesktopError::BadRequest(
                "Update Beep on this machine to manage its MCP servers.".into(),
            ))
        }
        Err(error) => Err(error.into()),
    }
}
