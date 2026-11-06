//! The machines running beep-node, as Settings lists them: the machines of
//! the mesh, each with its live status, and the machines the server links
//! that are not in it.

use crate::{
    beep::{Beep, Service, epoch_ms},
    center::home_sessions::HOME_CHANGED,
    chat::files::{file_name, free_name},
    error::{DesktopError, Result},
    fleet::{ClockOff, Fleet, Machines, files as direct},
    projects::unknown_op,
};
use beep_projects::imports::found::MachineFound;
use beep_proto::{
    AgentHost, ApiError, Call, GetMachineLog, ListMachineDetails, ListMachineStats,
    ListMachineTraffic, ListMachines, ListUpdatableMachines, Machine, MachineDetails, MachineEnv,
    MachineSession, MachineStats, MachineTraffic, MachineUpdate, RemoveMachine, Terminal,
    TrafficCounter, UpdatableMachine, UpdateMachine, node::logs::MAX_LINES,
};
use iroh::EndpointId;
use serde::Serialize;
use std::{
    fs,
    future::Future,
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
    /// Removed after a day offline, such as a container.
    temporary: bool,
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
            temporary: false,
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
                let mut held = self.list.lock().unwrap_or_else(PoisonError::into_inner);
                let changed = held.len() != listed.len()
                    || held.iter().zip(&listed).any(|(old, new)| !same(old, new));
                *held = listed;
                changed
            }
            Err(e) => {
                log::debug!("the server's machines are not listed: {e}");
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

    /// `mesh` and the machines the server last listed that it lacks, at
    /// once. With a server, the list is read again in the background, and
    /// the windows redraw when `same` tells the new list from the last.
    pub(crate) fn merged<R>(
        self: &Arc<Self>,
        app: &AppHandle,
        mesh: Vec<T>,
        request: R,
        id: impl Fn(&T) -> &str,
        same: fn(&T, &T) -> bool,
    ) -> Vec<T>
    where
        R: Call<Reply = Vec<T>> + Sync,
    {
        let beep = app.state::<Arc<Beep>>().inner().clone();
        if !beep.has_server() {
            return mesh;
        }
        self.refresh_with(app, async move { beep.call(request).await }, same);
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
    /// Beep's sessions on the other machines of the mesh, and the server's.
    pub(crate) sessions: Arc<Held<MachineSession>>,
    /// The sessions started outside Beep, found by scanning every machine.
    pub(crate) scans: Arc<Held<MachineSession>>,
    /// The terminals on the other machines of the mesh, as they stand.
    pub(crate) terminals: Arc<Held<Terminal>>,
    /// The terminals the server keeps.
    pub(crate) server_terminals: Arc<Held<Terminal>>,
    /// The checkouts each machine of the mesh named last.
    pub(crate) found: FoundOn,
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
    held.machines.merged(
        app,
        mesh,
        ListMachines {},
        |machine| &machine.id,
        |old, new| old.id == new.id && old.online == new.online,
    )
}

pub(crate) fn details(app: &AppHandle, fleet: &Fleet, held: &Outside) -> Vec<MachineDetails> {
    let mesh = fleet.status().details();
    held.details.merged(
        app,
        mesh,
        ListMachineDetails {},
        |details| &details.id,
        |old, new| old == new,
    )
}

pub(crate) fn stats(app: &AppHandle, fleet: &Fleet, held: &Outside) -> Vec<MachineStats> {
    let mesh = fleet.status().stats();
    // Live values: a new reading is never news.
    held.stats.merged(
        app,
        mesh,
        ListMachineStats {},
        |stats| &stats.id,
        |_, _| true,
    )
}

#[tauri::command]
pub async fn list_machines(
    app: AppHandle,
    fleet: State<'_, Arc<Fleet>>,
    held: State<'_, Outside>,
) -> Result<Vec<MachineView>> {
    let machines = machines(&app, &fleet, &held);
    let temporary = fleet
        .mesh()
        .map(|mesh| mesh.temporaries())
        .unwrap_or_default();
    Ok(machines
        .into_iter()
        .map(MachineView::from)
        .map(|view| MachineView {
            temporary: view
                .id
                .parse::<EndpointId>()
                .is_ok_and(|id| temporary.contains(&id)),
            ..view
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
pub async fn machine_traffic(
    app: AppHandle,
    fleet: State<'_, Arc<Fleet>>,
    held: State<'_, Outside>,
) -> Result<Vec<TrafficView>> {
    let mesh = fleet.status().traffic();
    let traffic = held.traffic.merged(
        &app,
        mesh,
        ListMachineTraffic {},
        |traffic| &traffic.id,
        // Counters that grow: a new reading is never news.
        |_, _| true,
    );
    Ok(traffic.into_iter().map(TrafficView::from).collect())
}

/// Removes a machine from the mesh, online or not. It loses its link to
/// every other member, and never rejoins with the same key.
#[tauri::command]
pub async fn mesh_remove(fleet: State<'_, Arc<Fleet>>, id: EndpointId) -> Result<()> {
    fleet.remove(id).await
}

/// Removes one of the account's machines, online or not. A member leaves the
/// mesh; and when the app is linked, the server drops the machine too, so it
/// gets no more credentials and cannot be run on. A server that cannot be
/// reached now is asked again at launch.
#[tauri::command]
pub async fn remove_machine(
    beep: Service<'_>,
    fleet: State<'_, Arc<Fleet>>,
    id: String,
) -> Result<()> {
    if let Ok(machine) = id.parse::<EndpointId>()
        && fleet.machines().iter().any(|m| m.device.id == machine)
    {
        fleet.remove(machine).await?;
        if !beep.linked() {
            return Ok(());
        }
        beep.owe_unlink(&id);
        return unlink(&beep, &id).await.map_err(|e| {
            log::warn!("the server still lists a removed machine: {e}");
            DesktopError::BadRequest(
                "Removed from the mesh, but the server still lists this machine. \
                 It will try again at launch."
                    .into(),
            )
        });
    }
    if !beep.has_server() {
        return Err(DesktopError::BadRequest("that is not a machine".into()));
    }
    match beep.call(RemoveMachine { machine_id: id }).await {
        Err(e) if unknown_op(&e) => Err(DesktopError::BadRequest(
            "The server needs an update before machines can be removed here.".into(),
        )),
        done => done,
    }
}

/// Asks the server to drop `id`. Done once it has: a machine it does not
/// list, such as one that only joined by ticket, is as good as dropped.
async fn unlink(beep: &Beep, id: &str) -> Result<()> {
    let done = beep
        .call(RemoveMachine {
            machine_id: id.to_owned(),
        })
        .await;
    if dropped(&done) {
        beep.settle_unlink(id);
        return Ok(());
    }
    done
}

/// Whether the server no longer has the machine after `reply`, or never
/// will take the request: a bad request (a server too old to know it, or
/// the machine is this one) would be refused again at every launch.
fn dropped(reply: &Result<()>) -> bool {
    matches!(
        reply,
        Ok(())
            | Err(DesktopError::Api(
                ApiError::NotFound {} | ApiError::BadRequest { .. }
            ))
    )
}

/// Asks the server again for each removal it did not take earlier.
pub(crate) async fn retry_unlinks(beep: Arc<Beep>) {
    if !beep.linked() {
        return;
    }
    for id in beep.owed_unlinks() {
        if let Err(e) = unlink(&beep, &id).await {
            log::warn!("the server still lists a removed machine: {e}");
        }
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
    Ok(held.updatable.merged(
        &app,
        mesh,
        ListUpdatableMachines {},
        |machine| &machine.id,
        |old, new| old.id == new.id,
    ))
}

/// The newest beep-node release, looked up at most hourly; `None` while the
/// releases cannot be reached, and the page then goes by the machines alone.
#[tauri::command]
pub async fn node_release(fleet: Machines<'_>) -> Result<Option<String>> {
    Ok(fleet.releases().latest_node().await)
}

/// The recent log of one of the account's machines.
#[tauri::command]
pub async fn machine_log(beep: Service<'_>, fleet: Machines<'_>, id: String) -> Result<String> {
    if let Some(machine) = fleet.member(&id) {
        return direct::machine_log(&fleet, machine, MAX_LINES).await;
    }
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
pub async fn update_machine(
    beep: Service<'_>,
    fleet: Machines<'_>,
    id: String,
) -> Result<MachineUpdate> {
    if let Some(machine) = fleet.member(&id) {
        return direct::update_machine(&fleet, machine, false).await;
    }
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
        let held = Arc::new(Held::<String>::default());
        let refresh = held.begin().expect("the first refresh starts");
        assert!(held.begin().is_none(), "one refresh runs at a time");
        assert!(held.finish(Ok(vec!["far".to_owned()]), |a, b| a == b));
        drop(refresh);
        let refresh = held.begin().expect("the next refresh starts");
        assert!(!held.finish(Err(DesktopError::NoServer), |a, b| a == b));
        assert_eq!(held.last(), ["far"]);
        drop(refresh);
        // A list that did not change is not news.
        assert!(!held.finish(Ok(vec!["far".to_owned()]), |a, b| a == b));
    }

    #[test]
    fn a_refresh_that_panicked_does_not_block_the_next() {
        let held = Arc::new(Held::<String>::default());
        let refresh = held.begin().expect("the first refresh starts");
        let died = std::thread::spawn(move || {
            let _refresh = refresh;
            panic!("the task died");
        })
        .join();
        assert!(died.is_err());
        assert!(held.begin().is_some());
    }

    #[test]
    fn a_live_value_that_moved_is_not_a_change() {
        // Entries are (id, load): only the id is structure.
        let held = Held::<(String, u32)>::default();
        let same = |a: &(String, u32), b: &(String, u32)| a.0 == b.0;
        assert!(held.finish(Ok(vec![("box".to_owned(), 3)]), same));
        assert!(!held.finish(Ok(vec![("box".to_owned(), 9)]), same));
        assert_eq!(held.last(), [("box".to_owned(), 9)]);
        // A machine that came or went is.
        assert!(held.finish(Ok(vec![("box".to_owned(), 9), ("rig".to_owned(), 1)]), same));
        assert!(held.finish(Ok(vec![("rig".to_owned(), 1)]), same));
    }

    #[test]
    fn with_no_server_the_mesh_machines_stand_alone() {
        let held = Held::<String>::default();
        assert_eq!(
            outside(vec!["studio".to_owned()], &held.last(), |id| id),
            ["studio"]
        );
    }

    #[test]
    fn a_machine_the_server_does_not_list_counts_as_dropped() {
        assert!(dropped(&Ok(())));
        assert!(dropped(&Err(DesktopError::Api(ApiError::NotFound {}))));
        assert!(dropped(&Err(DesktopError::Api(ApiError::bad_request(
            "unknown request"
        )))));
        assert!(!dropped(&Err(DesktopError::BadRequest(
            "took too long".into()
        ))));
        assert!(!dropped(&Err(DesktopError::Api(ApiError::Internal {}))));
        assert!(!dropped(&Err(DesktopError::NoServer)));
    }
}
