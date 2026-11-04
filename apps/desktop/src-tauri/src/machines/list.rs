//! The machines running beep-node, as Settings lists them: the machines of
//! the mesh, each with its live status, and the machines the server links
//! that are not in it.

use crate::{
    beep::{Beep, Service, epoch_ms},
    chat::files::{file_name, free_name},
    error::{DesktopError, Result},
    fleet::Fleet,
    projects::unknown_op,
};
use beep_proto::{
    Call, GetMachineLog, ListMachineDetails, ListMachineStats, ListMachineTraffic, ListMachines,
    ListUpdatableMachines, Machine, MachineDetails, MachineEnv, MachineStats, MachineTraffic,
    MachineUpdate, RemoveMachine, TrafficCounter, UpdatableMachine, UpdateMachine,
    node::logs::MAX_LINES,
};
use iroh::EndpointId;
use serde::Serialize;
use std::{fs, path::PathBuf, sync::Arc, time::Duration};
use tauri::{AppHandle, Manager, State};

/// How long a machine has to read its log; the server gives it 30 seconds.
const LOG_TIMEOUT: Duration = Duration::from_secs(40);

/// A machine as the bar shows it.
#[derive(Debug, Serialize)]
pub struct MachineView {
    id: String,
    name: String,
    online: bool,
    /// When it last connected or reported, in milliseconds since the Unix
    /// epoch.
    seen_at: u64,
    version: String,
    /// `None` until its first report.
    env: Option<MachineEnv>,
}

impl From<Machine> for MachineView {
    fn from(machine: Machine) -> Self {
        Self {
            id: machine.id,
            name: machine.name,
            online: machine.online,
            seen_at: epoch_ms(machine.seen_at),
            version: machine.version,
            env: machine.env,
        }
    }
}

/// `mesh`, and what the server lists that it does not: a machine outside the
/// mesh is reached through the server, when there is one. A server that
/// cannot answer adds nothing.
pub(crate) async fn with_outside<R, T>(
    beep: &Beep,
    mut mesh: Vec<T>,
    request: R,
    id: impl Fn(&T) -> &str,
) -> Vec<T>
where
    R: Call<Reply = Vec<T>>,
{
    if beep.server().is_none() {
        return mesh;
    }
    match beep.call(request).await {
        Ok(listed) => {
            let outside: Vec<T> = listed
                .into_iter()
                .filter(|item| !mesh.iter().any(|held| id(held) == id(item)))
                .collect();
            mesh.extend(outside);
        }
        Err(e) => log::debug!("the server's machines are not listed: {e}"),
    }
    mesh
}

pub(crate) async fn machines(beep: &Beep, fleet: &Fleet) -> Vec<Machine> {
    let mesh = fleet.status().machines();
    with_outside(beep, mesh, ListMachines {}, |machine| &machine.id).await
}

pub(crate) async fn details(beep: &Beep, fleet: &Fleet) -> Vec<MachineDetails> {
    let mesh = fleet.status().details();
    with_outside(beep, mesh, ListMachineDetails {}, |details| &details.id).await
}

pub(crate) async fn stats(beep: &Beep, fleet: &Fleet) -> Vec<MachineStats> {
    let mesh = fleet.status().stats();
    with_outside(beep, mesh, ListMachineStats {}, |stats| &stats.id).await
}

#[tauri::command]
pub async fn list_machines(
    beep: Service<'_>,
    fleet: State<'_, Arc<Fleet>>,
) -> Result<Vec<MachineView>> {
    let machines = machines(&beep, &fleet).await;
    Ok(machines.into_iter().map(MachineView::from).collect())
}

/// What a machine moved over iroh, as the Machines screens read it.
#[derive(Debug, Serialize)]
pub struct TrafficView {
    id: String,
    counters: Vec<TrafficCounter>,
}

impl From<MachineTraffic> for TrafficView {
    fn from(traffic: MachineTraffic) -> Self {
        Self {
            id: traffic.id,
            counters: traffic.counters,
        }
    }
}

#[tauri::command]
pub async fn machine_traffic(
    beep: Service<'_>,
    fleet: State<'_, Arc<Fleet>>,
) -> Result<Vec<TrafficView>> {
    let mesh = fleet.status().traffic();
    let traffic = with_outside(&beep, mesh, ListMachineTraffic {}, |traffic| &traffic.id).await;
    Ok(traffic.into_iter().map(TrafficView::from).collect())
}

/// Removes a machine from the mesh, online or not. It loses its link to
/// every other member, and never rejoins with the same key.
#[tauri::command]
pub async fn mesh_remove(fleet: State<'_, Arc<Fleet>>, id: EndpointId) -> Result<()> {
    fleet.remove(id).await
}

/// Removes one of the account's machines, online or not: from the mesh when
/// it is a member, else from the server, which drops its connection and
/// refuses its node from then on.
#[tauri::command]
pub async fn remove_machine(
    beep: Service<'_>,
    fleet: State<'_, Arc<Fleet>>,
    id: String,
) -> Result<()> {
    if let Ok(machine) = id.parse::<EndpointId>()
        && fleet.machines().iter().any(|m| m.device.id == machine)
    {
        return fleet.remove(machine).await;
    }
    if beep.server().is_none() {
        return Err(DesktopError::BadRequest("that is not a machine".into()));
    }
    match beep.call(RemoveMachine { machine_id: id }).await {
        Err(e) if unknown_op(&e) => Err(DesktopError::BadRequest(
            "The server needs an update before machines can be removed here.".into(),
        )),
        done => done,
    }
}

/// How long a node has to download and install a release.
const UPDATE_TIMEOUT: Duration = Duration::from_secs(6 * 60);

/// The machines Update works on: those of the mesh that update over
/// `update/1`, and those the server updates.
#[tauri::command]
pub async fn updatable_machines(
    beep: Service<'_>,
    fleet: State<'_, Arc<Fleet>>,
) -> Result<Vec<UpdatableMachine>> {
    let mesh = fleet.status().updatable();
    Ok(with_outside(&beep, mesh, ListUpdatableMachines {}, |machine| &machine.id).await)
}

/// The recent log of one of the account's machines.
#[tauri::command]
pub async fn machine_log(beep: Service<'_>, id: String) -> Result<String> {
    let request = GetMachineLog {
        machine_id: id,
        lines: MAX_LINES,
    };
    match beep.call_within(request, LOG_TIMEOUT).await {
        Err(e) if unknown_op(&e) => Err(DesktopError::BadRequest(
            "The server needs an update before logs can be read here.".into(),
        )),
        read => read,
    }
}

/// Saves a machine's log to Downloads for a report and returns where it went.
#[tauri::command]
pub fn save_machine_log(app: AppHandle, name: String, text: String) -> Result<PathBuf> {
    let to = free_name(
        &app.path().download_dir()?,
        &file_name(&format!("{name}.log")),
    );
    fs::write(&to, text).map_err(anyhow::Error::from)?;
    Ok(to)
}

/// Has one of the account's machines update its beep-node and restart
/// once its agent sessions are idle.
#[tauri::command]
pub async fn update_machine(beep: Service<'_>, id: String) -> Result<MachineUpdate> {
    let request = UpdateMachine {
        machine_id: id,
        stop_sessions: false,
    };
    match beep.call_within(request, UPDATE_TIMEOUT).await {
        Err(e) if unknown_op(&e) => Err(DesktopError::BadRequest(
            "The server needs an update before machines can be updated here.".into(),
        )),
        done => done,
    }
}
