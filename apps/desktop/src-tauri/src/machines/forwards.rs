//! Ports on other machines, reached through a port on this computer's
//! loopback. A forward listens until the app quits.

use crate::store::lock;
use crate::{
    error::{DesktopError, Result},
    fleet::Machines,
};
use anyhow::Context;
use beep_mesh::Mesh;
use iroh::EndpointId;
use std::{collections::HashMap, net::Ipv4Addr, sync::Mutex};
use tauri::State;
use tokio::{
    io::{copy_bidirectional, join},
    net::{TcpListener, TcpStream},
    task::{JoinHandle, JoinSet},
};

/// The most connections one forward carries at once, as many as the
/// machine takes on one connection.
const MAX_STREAMS: usize = 64;

/// The open forwards, by machine and port.
#[derive(Default)]
pub struct Forwards(Mutex<HashMap<(EndpointId, u16), Forward>>);

struct Forward {
    local: u16,
    task: JoinHandle<()>,
}

/// Forwards `port` on `machine` to this computer and returns the local port:
/// the same one when it is free. This computer's own ports need none.
#[tauri::command]
pub async fn machine_forward_port(
    fleet: Machines<'_>,
    forwards: State<'_, Forwards>,
    machine_id: String,
    port: u16,
) -> Result<u16> {
    // A phone app is suspended once the browser comes up, and its listener with it.
    if cfg!(mobile) {
        return Err(DesktopError::BadRequest(
            "Open another machine's ports from a desktop.".into(),
        ));
    }

    let machine = fleet.member(&machine_id).ok_or(DesktopError::NotInMesh)?;
    if machine == fleet.id() {
        return Ok(port);
    }

    if let Some(local) = forwards.open(machine, port) {
        return Ok(local);
    }

    // Offline first, so a failed dial below is most likely an older node.
    fleet.client(machine).await?;
    let mesh = fleet.ready().await?;
    mesh.forward_connection(machine).await.map_err(|_| {
        DesktopError::BadRequest(format!(
            "Could not reach the ports of {}. Update beep-node there if it is older.",
            fleet.name_of(machine)
        ))
    })?;
    let listener = match TcpListener::bind((Ipv4Addr::LOCALHOST, port)).await {
        Ok(listener) => listener,
        Err(_) => TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .context("could not listen on this computer")?,
    };
    let local = listener
        .local_addr()
        .context("could not listen on this computer")?
        .port();

    let mut open = lock(&forwards.0);
    // Another click may have opened one while this one bound.
    if let Some(forward) = open.get(&(machine, port))
        && !forward.task.is_finished()
    {
        return Ok(forward.local);
    }

    let task = tokio::spawn(serve(listener, mesh, machine, port));
    open.insert((machine, port), Forward { local, task });
    Ok(local)
}

impl Forwards {
    fn open(&self, machine: EndpointId, port: u16) -> Option<u16> {
        let open = lock(&self.0);
        open.get(&(machine, port))
            .filter(|forward| !forward.task.is_finished())
            .map(|forward| forward.local)
    }
}

/// Pipes each connection to the listener over its own stream to `port`.
async fn serve(listener: TcpListener, mesh: Mesh, machine: EndpointId, port: u16) {
    let mut streams = JoinSet::new();

    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let tcp = match accepted {
                    Ok((tcp, _)) => tcp,
                    Err(error) => {
                        log::warn!("stopped forwarding port {port}: {error}");
                        return;
                    }
                };

                if streams.len() < MAX_STREAMS {
                    streams.spawn(pipe(tcp, mesh.clone(), machine, port));
                }
            },
            Some(ended) = streams.join_next(), if !streams.is_empty() => {
                if let Ok(Err(error)) = ended {
                    log::debug!("a connection to port {port} ended: {error:#}");
                }
            },
        }
    }
}

async fn pipe(
    mut tcp: TcpStream,
    mesh: Mesh,
    machine: EndpointId,
    port: u16,
) -> anyhow::Result<()> {
    let (send, recv) = mesh.forward_stream(machine, port).await?;
    copy_bidirectional(&mut tcp, &mut join(recv, send)).await?;
    Ok(())
}
