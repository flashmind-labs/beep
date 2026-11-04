//! The machines running beep-node, as Settings lists them: the machines of
//! the mesh, each with its live status, and the machines the server links
//! that are not in it.

use crate::{
    beep::{Beep, Service, epoch_ms},
    center::home_sessions::HOME_CHANGED,
    chat::files::{file_name, free_name},
    error::{DesktopError, Result},
    fleet::Fleet,
    projects::unknown_op,
};
use beep_proto::{
    AgentHost, Call, GetMachineLog, ListMachineDetails, ListMachineStats, ListMachineTraffic,
    ListMachines, ListUpdatableMachines, Machine, MachineDetails, MachineEnv, MachineStats,
    MachineTraffic, MachineUpdate, RemoveMachine, TrafficCounter, UpdatableMachine, UpdateMachine,
    node::logs::MAX_LINES,
};
use iroh::EndpointId;
use serde::Serialize;
use std::{
    fs,
    path::PathBuf,
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tauri::{AppHandle, Emitter, Manager, State};

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

/// What the server last listed for one request: the machines outside the
/// mesh, which are reached through it. Read at once, and refreshed in the
/// background, so a slow or absent server delays nothing.
pub struct Held<T> {
    list: Mutex<Vec<T>>,
    refreshing: AtomicBool,
}

impl<T> Default for Held<T> {
    fn default() -> Self {
        Self {
            list: Mutex::default(),
            refreshing: AtomicBool::new(false),
        }
    }
}

impl<T: Clone + PartialEq + Send + 'static> Held<T> {
    fn last(&self) -> Vec<T> {
        self.list
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Whether the caller may refresh: one refresh runs at a time.
    fn begin(&self) -> bool {
        !self.refreshing.swap(true, Ordering::Relaxed)
    }

    /// Ends the refresh that `begin` allowed. A list that could not be read
    /// keeps the last one. Whether the list changed.
    fn finish(&self, listed: Result<Vec<T>>) -> bool {
        self.refreshing.store(false, Ordering::Relaxed);
        match listed {
            Ok(listed) => {
                let mut held = self.list.lock().unwrap_or_else(PoisonError::into_inner);
                let changed = *held != listed;
                *held = listed;
                changed
            }
            Err(e) => {
                log::debug!("the server's machines are not listed: {e}");
                false
            }
        }
    }

    /// `mesh` and the machines the server last listed that it lacks, at
    /// once. With a server, the list is read again in the background, and
    /// the windows redraw when it changed.
    pub(crate) fn merged<R>(
        self: &Arc<Self>,
        app: &AppHandle,
        mesh: Vec<T>,
        request: R,
        id: impl Fn(&T) -> &str,
    ) -> Vec<T>
    where
        R: Call<Reply = Vec<T>>,
    {
        let beep = app.state::<Arc<Beep>>().inner().clone();
        if beep.server().is_none() {
            return mesh;
        }
        if self.begin() {
            let (held, app) = (self.clone(), app.clone());
            tauri::async_runtime::spawn(async move {
                let listed = beep.call(request).await;
                if held.finish(listed)
                    && let Err(e) = app.emit(HOME_CHANGED, ())
                {
                    log::warn!("telling the windows the machines changed: {e}");
                }
            });
        }
        outside(mesh, &self.last(), id)
    }
}

/// `mesh`, then the entries of `server` that are not in it.
fn outside<T: Clone>(mut mesh: Vec<T>, server: &[T], id: impl Fn(&T) -> &str) -> Vec<T> {
    let more: Vec<T> = server
        .iter()
        .filter(|item| !mesh.iter().any(|held| id(held) == id(item)))
        .cloned()
        .collect();
    mesh.extend(more);
    mesh
}

/// What the server listed that is not in the mesh, per kind of list.
#[derive(Default)]
pub struct Outside {
    machines: Arc<Held<Machine>>,
    details: Arc<Held<MachineDetails>>,
    stats: Arc<Held<MachineStats>>,
    traffic: Arc<Held<MachineTraffic>>,
    updatable: Arc<Held<UpdatableMachine>>,
    pub(crate) hosts: Arc<Held<AgentHost>>,
}

/// The machines of the mesh, and those only the server lists. A machine
/// that has not said its beep-node version has the server's.
pub(crate) fn machines(app: &AppHandle, fleet: &Fleet, held: &Outside) -> Vec<Machine> {
    let mut mesh = fleet.status().machines();
    let server = held.machines.last();
    for machine in mesh.iter_mut().filter(|m| m.version.is_empty()) {
        if let Some(listed) = server.iter().find(|s| s.id == machine.id) {
            machine.version.clone_from(&listed.version);
        }
    }
    held.machines
        .merged(app, mesh, ListMachines {}, |machine| &machine.id)
}

pub(crate) fn details(app: &AppHandle, fleet: &Fleet, held: &Outside) -> Vec<MachineDetails> {
    let mesh = fleet.status().details();
    held.details
        .merged(app, mesh, ListMachineDetails {}, |details| &details.id)
}

pub(crate) fn stats(app: &AppHandle, fleet: &Fleet, held: &Outside) -> Vec<MachineStats> {
    let mesh = fleet.status().stats();
    held.stats
        .merged(app, mesh, ListMachineStats {}, |stats| &stats.id)
}

#[tauri::command]
pub async fn list_machines(
    app: AppHandle,
    fleet: State<'_, Arc<Fleet>>,
    held: State<'_, Outside>,
) -> Result<Vec<MachineView>> {
    let machines = machines(&app, &fleet, &held);
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
    app: AppHandle,
    fleet: State<'_, Arc<Fleet>>,
    held: State<'_, Outside>,
) -> Result<Vec<TrafficView>> {
    let mesh = fleet.status().traffic();
    let traffic = held
        .traffic
        .merged(&app, mesh, ListMachineTraffic {}, |traffic| &traffic.id);
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
    app: AppHandle,
    fleet: State<'_, Arc<Fleet>>,
    held: State<'_, Outside>,
) -> Result<Vec<UpdatableMachine>> {
    let mesh = fleet.status().updatable();
    Ok(held
        .updatable
        .merged(&app, mesh, ListUpdatableMachines {}, |machine| &machine.id))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_machine_only_the_server_lists_is_added_to_the_mesh() {
        let mesh = vec!["studio".to_owned(), "box".to_owned()];
        let server = ["box".to_owned(), "phone-host".to_owned()];
        let merged = outside(mesh, &server, |id| id);
        assert_eq!(merged, ["studio", "box", "phone-host"]);
    }

    #[test]
    fn a_server_that_cannot_answer_leaves_the_last_list() {
        let held = Held::<String>::default();
        assert!(held.begin());
        assert!(!held.begin(), "one refresh runs at a time");
        assert!(held.finish(Ok(vec!["far".to_owned()])));
        assert!(held.begin());
        assert!(!held.finish(Err(DesktopError::NoServer)));
        assert_eq!(held.last(), ["far"]);
        // The failed refresh ended, so the next may start, and a list
        // that did not change is not news.
        assert!(held.begin());
        assert!(!held.finish(Ok(vec!["far".to_owned()])));
    }

    #[test]
    fn with_no_server_the_mesh_machines_stand_alone() {
        let held = Held::<String>::default();
        assert_eq!(
            outside(vec!["studio".to_owned()], &held.last(), |id| id),
            ["studio"]
        );
    }
}
