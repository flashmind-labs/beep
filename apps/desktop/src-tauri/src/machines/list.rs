//! The machines running beep-node on the account, as Settings lists them.
//! Only the account's own machines.

use crate::{
    beep::{Service, epoch_ms},
    error::{DesktopError, Result},
    projects::unknown_op,
};
use beep_proto::{
    ListMachineTraffic, ListMachines, ListUpdatableMachines, Machine, MachineEnv, MachineTraffic,
    MachineUpdate, RemoveMachine, TrafficCounter, UpdatableMachine, UpdateMachine,
};
use serde::Serialize;
use std::time::Duration;

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

#[tauri::command]
pub async fn list_machines(beep: Service<'_>) -> Result<Vec<MachineView>> {
    let machines = beep.call(ListMachines {}).await?;
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
pub async fn machine_traffic(beep: Service<'_>) -> Result<Vec<TrafficView>> {
    let traffic = beep.call(ListMachineTraffic {}).await?;
    Ok(traffic.into_iter().map(TrafficView::from).collect())
}

/// Unlinks one of the account's machines, online or not. The server drops
/// its connection if it has one and refuses its node from then on.
#[tauri::command]
pub async fn remove_machine(beep: Service<'_>, id: String) -> Result<()> {
    match beep.call(RemoveMachine { machine_id: id }).await {
        Err(e) if unknown_op(&e) => Err(DesktopError::BadRequest(
            "The server needs an update before machines can be removed here.".into(),
        )),
        done => done,
    }
}

/// How long a node has to download and install a release.
const UPDATE_TIMEOUT: Duration = Duration::from_secs(6 * 60);

/// The machines Update works on.
/// None from a server without it.
#[tauri::command]
pub async fn updatable_machines(beep: Service<'_>) -> Result<Vec<UpdatableMachine>> {
    match beep.call(ListUpdatableMachines {}).await {
        Err(e) if unknown_op(&e) => Ok(Vec::new()),
        listed => listed,
    }
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
