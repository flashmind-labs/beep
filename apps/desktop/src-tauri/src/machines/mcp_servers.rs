use crate::{
    error::{DesktopError, Result},
    fleet::{Fleet, Machines},
};
use beep_proto::node::mcp_servers::{EditableServer, Harness, PROTOCOL, Reply, Request, Server};
use futures::{StreamExt, stream};
use iroh::EndpointId;
use serde::Serialize;

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
    let mut results = stream::iter(hosts.into_iter().filter(|host| host.device.id != source))
        .map(|host| {
            let fleet = &*fleet;
            let request = Request::Apply {
                harness,
                server: server.clone(),
            };

            async move {
                let result =
                    call(fleet, host.device.id, request)
                        .await
                        .and_then(|reply| match reply {
                            Reply::Applied => Ok(()),
                            _ => Err(DesktopError::BadRequest(
                                "Unexpected MCP server reply.".into(),
                            )),
                        });

                Applied {
                    name: host.device.name,
                    error: result.err().map(|error| error.to_string()),
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
    let client = fleet.client(machine).await?;
    match client.call(PROTOCOL, &request).await {
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
