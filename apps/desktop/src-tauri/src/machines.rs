//! The machines running beep-node on the account, as Settings lists them.
//! Only the account's own machines.

use crate::{
    beep::{Beep, epoch_ms},
    error::Result,
};
use beep_proto::{ListMachines, Machine, MachineEnv};
use serde::Serialize;
use std::sync::Arc;
use tauri::State;

type Service<'a> = State<'a, Arc<Beep>>;

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

impl MachineView {
    pub(crate) fn id(&self) -> &str {
        &self.id
    }

    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    pub(crate) fn online(&self) -> bool {
        self.online
    }
}

#[tauri::command]
pub async fn list_machines(beep: Service<'_>) -> Result<Vec<MachineView>> {
    let machines = beep.call(ListMachines {}).await?;
    Ok(machines.into_iter().map(MachineView::from).collect())
}
