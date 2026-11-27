//! The machines running beep-node, as Settings lists them: the machines of
//! the mesh, each with its live status.

use crate::{
    beep::{epoch_ms, state::Prefs},
    chat::files::{file_name, free_name},
    error::{DesktopError, Result},
    fleet::{ClockOff, Fleet, Machines},
    store::lock,
};
use beep_projects::imports::found::MachineFound;
use beep_proto::{Machine, MachineEnv, Terminal, node::AdapterInstall, node::device::PROTOCOL};
use futures::future::BoxFuture;
use iroh::EndpointId;
use serde::Serialize;
use std::{
    fs,
    future::Future,
    path::PathBuf,
    sync::{Arc, Mutex},
};
use tauri::{AppHandle, State};
use tokio::sync::mpsc::{self, error::TrySendError};

/// A machine that serves `device/1`.
#[derive(Debug, Serialize)]
pub struct DeviceHost {
    machine_id: String,
    machine_name: String,
}

/// The machines that are on and serve `device/1`, as last reported; no
/// machine is asked.
#[tauri::command]
pub fn device_hosts(fleet: Machines<'_>) -> Vec<DeviceHost> {
    let mut hosts: Vec<DeviceHost> = fleet
        .status()
        .offering(PROTOCOL)
        .into_iter()
        .map(|machine| DeviceHost {
            machine_id: machine.to_string(),
            machine_name: fleet.name_of(machine),
        })
        .collect();
    hosts.sort_by(|a, b| a.machine_name.cmp(&b.machine_name));
    hosts
}

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

struct HeldState<T> {
    list: Vec<T>,
    refresh: Option<mpsc::Sender<BoxFuture<'static, ()>>>,
}

/// What the machines last listed for one request. Read at once, and
/// refreshed in the background, so a slow machine delays nothing.
pub struct Held<T> {
    state: Mutex<HeldState<T>>,
}

impl<T> Default for Held<T> {
    fn default() -> Self {
        Self {
            state: Mutex::new(HeldState {
                list: Vec::new(),
                refresh: None,
            }),
        }
    }
}

impl<T: Clone + Send + 'static> Held<T> {
    fn last(&self) -> Vec<T> {
        lock(&self.state).list.clone()
    }

    /// The worker runs one refresh at a time. Further requests share the queued pass.
    fn queue_refresh(&self, refresh: BoxFuture<'static, ()>) {
        let mut held = lock(&self.state);
        let refresh = match &held.refresh {
            Some(queue) => match queue.try_send(refresh) {
                Ok(()) | Err(TrySendError::Full(_)) => return,
                Err(TrySendError::Closed(refresh)) => refresh,
            },
            None => refresh,
        };

        let (queue, mut requests) = mpsc::channel(1);
        held.refresh = Some(queue);
        drop(held);
        // Only the individual requests hold the app or list. Closing the queue ends the worker.
        tauri::async_runtime::spawn(async move {
            refresh.await;
            while let Some(refresh) = requests.recv().await {
                refresh.await;
            }
        });
    }

    /// Keeps `listed`; a list that could not be read keeps the last one.
    /// Whether the list changed in a way `same` tells apart, which leaves
    /// out values that move on every read.
    fn finish(&self, listed: Result<Vec<T>>, same: impl Fn(&T, &T) -> bool) -> bool {
        match listed {
            Ok(listed) => {
                let mut held = lock(&self.state);
                let changed = held.list.len() != listed.len()
                    || held
                        .list
                        .iter()
                        .zip(&listed)
                        .any(|(old, new)| !same(old, new));
                held.list = listed;
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
        let (held, app) = (self.clone(), app.clone());
        self.queue_refresh(Box::pin(async move {
            if !held.finish(ask.await, same) {
                return;
            }

            crate::fleet::home_signal::poke(&app);
        }));
    }

    /// Asks now, keeps the answer and gives the list, the last held when `ask` fails.
    pub(crate) async fn read_now(
        &self,
        app: &AppHandle,
        ask: impl Future<Output = Result<Vec<T>>>,
        same: fn(&T, &T) -> bool,
    ) -> Vec<T> {
        if self.finish(ask.await, same) {
            crate::fleet::home_signal::poke(app);
        }

        self.last()
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

    /// Replaces the rows `of` picks with `rows`, for a change that one machine
    /// sent; the others stay as held.
    pub(crate) fn replace(&self, rows: Vec<T>, of: impl Fn(&T) -> bool) {
        let mut held = lock(&self.state);
        held.list.retain(|row| !of(row));
        held.list.extend(rows);
    }
}

/// What the machines of the mesh last listed, per kind of list. Their
/// sessions are the session mirror's.
#[derive(Default)]
pub struct MachineLists {
    /// The terminals on the other machines of the mesh, as they stand.
    pub(crate) terminals: Arc<Held<Terminal>>,
    /// The checkouts each machine of the mesh named last.
    pub(crate) found: FoundOn,
}

/// What each machine named last when asked for its checkouts.
#[derive(Default)]
pub(crate) struct FoundOn(Mutex<Vec<MachineFound>>);

impl FoundOn {
    pub(crate) fn last(&self) -> Vec<MachineFound> {
        lock(&self.0).clone()
    }

    /// Keeps `found` in place of what its machine named before.
    pub(crate) fn keep(&self, found: MachineFound) {
        let mut all = lock(&self.0);
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
                    .and_then(|id| fleet.reach().last_env(id))
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

/// Removes a machine of the mesh by its id as text.
#[tauri::command]
pub async fn remove_machine(fleet: State<'_, Arc<Fleet>>, id: String) -> Result<()> {
    fleet.remove(member(&fleet, &id)?).await
}

/// Renames a machine of the mesh by its id as text; an empty name restores its own.
#[tauri::command]
pub async fn rename_machine(fleet: State<'_, Arc<Fleet>>, id: String, name: String) -> Result<()> {
    fleet.rename(member(&fleet, &id)?, &name).await
}

/// The machine of the mesh whose id `id` spells.
fn member(fleet: &Fleet, id: &str) -> Result<EndpointId> {
    id.parse::<EndpointId>()
        .ok()
        .filter(|machine| fleet.machines().iter().any(|m| m.device.id == *machine))
        .ok_or_else(|| DesktopError::BadRequest("that is not a machine".into()))
}

/// The newest beep-node release, looked up at most hourly; `None` while the
/// releases cannot be reached, and the page then goes by the machines alone.
#[tauri::command]
pub async fn node_release(fleet: Machines<'_>, prefs: State<'_, Prefs>) -> Result<Option<String>> {
    Ok(fleet
        .releases()
        .latest_node(prefs.node_release_channel())
        .await)
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

#[cfg(test)]
#[path = "../tests/machines_list.rs"]
mod tests;
