//! The machines running beep-node, as Settings lists them: the machines of
//! the mesh, each with its live status.

use crate::{
    beep::epoch_ms,
    center::home_sessions::HOME_CHANGED,
    chat::files::{file_name, free_name},
    error::{DesktopError, Result},
    fleet::{ClockOff, Fleet, Machines, files as direct},
};
use beep_projects::imports::found::MachineFound;
use beep_proto::{
    Machine, MachineEnv, MachineSession, MachineTraffic, MachineUpdate, Terminal, TrafficCounter,
    UpdatableMachine,
    node::{AdapterInstall, logs::MAX_LINES},
};
use iroh::EndpointId;
use serde::Serialize;
use std::{
    collections::HashMap,
    fs,
    future::Future,
    path::PathBuf,
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter, State};

/// A machine as the bar shows it.
#[derive(Debug, Serialize)]
pub struct MachineView {
    id: String,
    name: String,
    online: bool,
    can_host: bool,
    /// When it last connected or reported, in milliseconds since the Unix
    /// epoch.
    seen_at: u64,
    version: String,
    /// `None` until its first report.
    env: Option<MachineEnv>,
    /// Removed after a day offline, such as a container.
    temporary: bool,
    /// The agents whose ACP adapter it is installing or could not install.
    adapters: Vec<AdapterInstall>,
}

impl From<Machine> for MachineView {
    fn from(machine: Machine) -> Self {
        Self {
            id: machine.id,
            name: machine.name,
            online: machine.online,
            can_host: true,
            seen_at: epoch_ms(machine.seen_at),
            version: machine.version,
            env: machine.env,
            temporary: false,
            adapters: Vec::new(),
        }
    }
}

/// What the machines last listed for one request. Read at once, and
/// refreshed in the background, so a slow machine delays nothing.
pub struct Held<T> {
    list: Mutex<Vec<T>>,
    refreshing: AtomicBool,
    /// When the list was last read, for `refreshed_after`.
    read_at: Mutex<Option<Instant>>,
}

impl<T> Default for Held<T> {
    fn default() -> Self {
        Self {
            list: Mutex::default(),
            refreshing: AtomicBool::new(false),
            read_at: Mutex::default(),
        }
    }
}

/// A refresh in progress; the next may start once it is dropped, however
/// its task ended.
struct Refresh<T>(Arc<Held<T>>);

impl<T> Drop for Refresh<T> {
    fn drop(&mut self) {
        self.0.refreshing.store(false, Ordering::Relaxed);
    }
}

impl<T: Clone + Send + 'static> Held<T> {
    pub(crate) fn last(&self) -> Vec<T> {
        self.list
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Changes the list as held, for what this app knows before the next read.
    pub(crate) fn edit(&self, change: impl FnOnce(&mut [T])) {
        let mut held = self.list.lock().unwrap_or_else(PoisonError::into_inner);
        change(held.as_mut_slice());
    }

    /// Allows a refresh, unless one runs: one at a time.
    fn begin(self: &Arc<Self>) -> Option<Refresh<T>> {
        (!self.refreshing.swap(true, Ordering::Relaxed)).then(|| Refresh(self.clone()))
    }

    /// Keeps `listed`; a list that could not be read keeps the last one.
    /// Whether the list changed in a way `same` tells apart, which leaves
    /// out values that move on every read.
    fn finish(&self, listed: Result<Vec<T>>, same: impl Fn(&T, &T) -> bool) -> bool {
        match listed {
            Ok(listed) => {
                *self.read_at.lock().unwrap_or_else(PoisonError::into_inner) = Some(Instant::now());
                let mut held = self.list.lock().unwrap_or_else(PoisonError::into_inner);
                let changed = held.len() != listed.len()
                    || held.iter().zip(&listed).any(|(old, new)| !same(old, new));
                *held = listed;
                changed
            }
            Err(e) => {
                log::debug!("a list was not read again: {e}");
                false
            }
        }
    }

    /// Reads the list again with `ask` in the background, one at a time;
    /// the windows redraw when `same` tells the new list from the last.
    fn refresh_with(
        self: &Arc<Self>,
        app: &AppHandle,
        ask: impl Future<Output = Result<Vec<T>>> + Send + 'static,
        same: fn(&T, &T) -> bool,
    ) {
        let Some(refresh) = self.begin() else {
            return;
        };
        let (held, app) = (self.clone(), app.clone());
        // Ends with `ask`, which the caller bounds; nothing else owns it.
        tauri::async_runtime::spawn(async move {
            let _refresh = refresh;
            let listed = ask.await;

            if held.finish(listed, same)
                && let Err(e) = app.emit(HOME_CHANGED, ())
            {
                log::warn!("telling the windows a list changed: {e}");
            }
        });
    }

    /// What `ask` listed last, at once, and `ask` run again in the
    /// background for the next call.
    pub(crate) fn refreshed(
        self: &Arc<Self>,
        app: &AppHandle,
        ask: impl Future<Output = Result<Vec<T>>> + Send + 'static,
        same: fn(&T, &T) -> bool,
    ) -> Vec<T> {
        self.refresh_with(app, ask, same);
        self.last()
    }

    /// As `refreshed`, but `ask` runs only once the last read is `fresh`
    /// old, so the callers in between share it.
    pub(crate) fn refreshed_after(
        self: &Arc<Self>,
        app: &AppHandle,
        ask: impl Future<Output = Result<Vec<T>>> + Send + 'static,
        same: fn(&T, &T) -> bool,
        fresh: Duration,
    ) -> Vec<T> {
        let due = self
            .read_at
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_none_or(|at| at.elapsed() >= fresh);
        if due {
            self.refresh_with(app, ask, same);
        }

        self.last()
    }

    /// Replaces the rows `of` picks with `rows`, for a change that one machine
    /// sent; the others stay as held.
    pub(crate) fn replace(&self, rows: Vec<T>, of: impl Fn(&T) -> bool) {
        let mut held = self.list.lock().unwrap_or_else(PoisonError::into_inner);
        held.retain(|row| !of(row));
        held.extend(rows);
    }

    /// Whether the list was read since it started or was last made stale.
    pub(crate) fn was_read(&self) -> bool {
        self.read_at
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_some()
    }

    /// Keeps `listed`, read by the caller.
    pub(crate) fn keep(&self, listed: Vec<T>) {
        *self.read_at.lock().unwrap_or_else(PoisonError::into_inner) = Some(Instant::now());
        *self.list.lock().unwrap_or_else(PoisonError::into_inner) = listed;
    }

    /// The next `refreshed_after` reads again, however recent the last read.
    pub(crate) fn stale(&self) {
        *self.read_at.lock().unwrap_or_else(PoisonError::into_inner) = None;
    }
}

/// What the machines of the mesh last listed, per kind of list.
#[derive(Default)]
pub struct MachineLists {
    /// Beep's sessions on this Mac, from the node in this process.
    pub(crate) local: Arc<Held<MachineSession>>,
    /// Beep's sessions on the other machines of the mesh.
    pub(crate) sessions: Arc<Held<MachineSession>>,
    /// The sessions started outside Beep, found by scanning every machine.
    pub(crate) scans: Arc<Held<MachineSession>>,
    /// The terminals on the other machines of the mesh, as they stand.
    pub(crate) terminals: Arc<Held<Terminal>>,
    /// The checkouts each machine of the mesh named last.
    pub(crate) found: FoundOn,
    /// The machines whose sessions are listed again now, and whether another
    /// change came meanwhile.
    pub(crate) relisting: Mutex<HashMap<String, bool>>,
}

/// What each machine named last when asked for its checkouts.
#[derive(Default)]
pub(crate) struct FoundOn(Mutex<Vec<MachineFound>>);

impl FoundOn {
    pub(crate) fn last(&self) -> Vec<MachineFound> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Keeps `found` in place of what its machine named before.
    pub(crate) fn keep(&self, found: MachineFound) {
        let mut all = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        all.retain(|known| known.machine_id != found.machine_id);
        all.push(found);
    }
}

#[tauri::command]
pub async fn list_machines(fleet: State<'_, Arc<Fleet>>) -> Result<Vec<MachineView>> {
    let machines = fleet.status().machines();
    let temporary = fleet
        .mesh()
        .map(|mesh| mesh.temporaries())
        .unwrap_or_default();
    Ok(machines
        .into_iter()
        .map(MachineView::from)
        .map(|view| {
            let id = view.id.parse::<EndpointId>().ok();
            MachineView {
                can_host: id
                    .is_none_or(|id| !fleet.mesh().is_some_and(|mesh| mesh.is_observer(id))),
                temporary: id.is_some_and(|id| temporary.contains(&id)),
                adapters: id
                    .and_then(|id| fleet.status().last_env(id))
                    .map(|env| env.adapters)
                    .unwrap_or_default(),
                ..view
            }
        })
        .collect())
}

/// The machines that are on with a clock far enough off to warn about.
#[tauri::command]
pub async fn machine_clocks(fleet: State<'_, Arc<Fleet>>) -> Result<Vec<ClockOff>> {
    Ok(fleet.status().clocks_off())
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
pub async fn machine_traffic(fleet: State<'_, Arc<Fleet>>) -> Result<Vec<TrafficView>> {
    let traffic = fleet.status().traffic();
    Ok(traffic.into_iter().map(TrafficView::from).collect())
}

/// Removes a machine from the mesh, online or not. It loses its link to
/// every other member, and never rejoins with the same key.
#[tauri::command]
pub async fn mesh_remove(fleet: State<'_, Arc<Fleet>>, id: EndpointId) -> Result<()> {
    fleet.remove(id).await
}

/// Removes a machine of the mesh by its id as text, as `mesh_remove` does.
#[tauri::command]
pub async fn remove_machine(fleet: State<'_, Arc<Fleet>>, id: String) -> Result<()> {
    let machine = id
        .parse::<EndpointId>()
        .ok()
        .filter(|machine| fleet.machines().iter().any(|m| m.device.id == *machine))
        .ok_or_else(|| DesktopError::BadRequest("that is not a machine".into()))?;
    fleet.remove(machine).await
}

/// The machines Update works on: those of the mesh that update over
/// `update/1`.
#[tauri::command]
pub async fn updatable_machines(fleet: State<'_, Arc<Fleet>>) -> Result<Vec<UpdatableMachine>> {
    Ok(fleet.status().updatable())
}

/// The newest beep-node release, looked up at most hourly; `None` while the
/// releases cannot be reached, and the page then goes by the machines alone.
#[tauri::command]
pub async fn node_release(fleet: Machines<'_>) -> Result<Option<String>> {
    Ok(fleet.releases().latest_node().await)
}

/// The recent log of one of the account's machines.
#[tauri::command]
pub async fn machine_log(fleet: Machines<'_>, id: String) -> Result<String> {
    direct::machine_log(&fleet, fleet.machine(&id)?, MAX_LINES).await
}

/// Saves a machine's log to Downloads for a report and returns where it went.
#[tauri::command]
pub fn save_machine_log(app: AppHandle, name: String, text: String) -> Result<PathBuf> {
    let to = free_name(
        &crate::platform::saves_dir(&app)?,
        &file_name(&format!("{name}.log")),
    );
    fs::write(&to, text).map_err(anyhow::Error::from)?;
    Ok(to)
}

/// Has one of the account's machines update its beep-node and restart
/// once its agent sessions are idle.
#[tauri::command]
pub async fn update_machine(fleet: Machines<'_>, id: String) -> Result<MachineUpdate> {
    direct::update_machine(&fleet, fleet.machine(&id)?, false).await
}

#[cfg(test)]
#[path = "../tests/machines_list.rs"]
mod tests;
