//! How each machine stands with the projects it keeps in step. A node follows `sync/<name>` in
//! the mesh by itself; the desktop writes that key and asks each node how it is doing.

use crate::{
    desk::Desk,
    error::{FleetError, Result},
    records::{WRITES, find_project, read, scan, write},
};
use beep_mesh::Key;
use beep_projects::model::plain_path;
use beep_proto::{
    ConflictKeep, MachineSync, MachineSyncDetail, ProjectMachine, ProjectSync, ProjectSyncDetail,
    SyncProgress, SyncStatus, SyncthingPlace,
    node::{
        mesh_state::SyncSpec,
        sync::{Keep, PROTOCOL, ProjectStatus, Reply, Request, State},
    },
};
use futures::{StreamExt, stream};
use iroh::EndpointId;
use std::collections::{BTreeSet, HashMap};

/// Machines asked about their sync at once.
const MACHINES_AT_ONCE: usize = 8;
/// Why a machine that answered without the project does not sync it: a node from before it
/// reported the projects it refused.
const NOT_LISTED: &str =
    "Its beep-node does not sync this project. Update beep-node there to see why.";
/// Why an online machine says nothing of its sync.
const NO_ANSWER: &str = "It did not say how its sync stands.";

fn status_of(state: State, error: bool) -> SyncStatus {
    match state {
        State::Scanning => SyncStatus::Scanning {},
        State::Syncing => SyncStatus::Syncing {},
        State::Idle | State::Paused => SyncStatus::InSync {},
        State::Failed => SyncStatus::Failed {},
        State::Other if error => SyncStatus::Failed {},
        State::Other => SyncStatus::Scanning {},
    }
}

impl Desk {
    /// What `machine` says of every project it syncs; `None` when it is offline or does not answer.
    async fn node_statuses(&self, machine: EndpointId) -> Option<Vec<ProjectStatus>> {
        let client = self.peers.client(machine).await.ok()?;
        let reply: Reply = client.call(PROTOCOL, &Request::Status {}).await.ok()?;
        match reply {
            Reply::Status { projects } => Some(projects),
            _ => None,
        }
    }

    pub(crate) fn name_of(&self, machine: EndpointId) -> String {
        self.mesh
            .devices()
            .into_iter()
            .find(|status| status.device.id == machine)
            .map_or_else(
                || machine.fmt_short().to_string(),
                |status| status.device.name,
            )
    }

    /// Asks each of `machines` once, a few at a time, for all its projects.
    async fn statuses(
        &self,
        machines: BTreeSet<EndpointId>,
    ) -> HashMap<EndpointId, Vec<ProjectStatus>> {
        stream::iter(machines)
            .map(|machine| async move { (machine, self.node_statuses(machine).await) })
            .buffer_unordered(MACHINES_AT_ONCE)
            .filter_map(|(machine, said)| async move { Some((machine, said?)) })
            .collect()
            .await
    }

    /// One machine's sync of `project`, from what it said of all of them.
    fn machine_sync(
        &self,
        machine: EndpointId,
        folder: &str,
        project: &str,
        said: &HashMap<EndpointId, Vec<ProjectStatus>>,
    ) -> (MachineSync, Option<ProjectStatus>) {
        let seen = said
            .get(&machine)
            .and_then(|all| all.iter().find(|status| status.name == project))
            .cloned();
        let sync = match &seen {
            Some(status) => MachineSync {
                machine_id: machine.to_string(),
                machine_name: self.name_of(machine),
                folder: folder.to_owned(),
                status: status_of(status.state, status.error.is_some()),
                files: status.files,
                bytes: status.bytes,
                pending: status.pending,
                conflicts: status.conflicts.clone(),
                rescan_secs: status.rescan_secs,
                hint: status.hint.clone(),
                error: status.error.clone(),
            },
            None => MachineSync {
                machine_id: machine.to_string(),
                machine_name: self.name_of(machine),
                folder: folder.to_owned(),
                // Offline only when it is: a machine that answered without the project, or that
                // the mesh sees but did not answer, does not sync it and says why.
                status: if said.contains_key(&machine) || self.is_online(machine) {
                    SyncStatus::Failed {}
                } else {
                    SyncStatus::Offline {}
                },
                files: 0,
                bytes: 0,
                pending: 0,
                conflicts: Vec::new(),
                rescan_secs: None,
                hint: None,
                error: if said.contains_key(&machine) {
                    Some(NOT_LISTED.to_owned())
                } else if self.is_online(machine) {
                    Some(NO_ANSWER.to_owned())
                } else {
                    None
                },
            },
        };
        (sync, seen)
    }

    fn is_online(&self, machine: EndpointId) -> bool {
        self.mesh
            .devices()
            .iter()
            .any(|status| status.device.id == machine && status.online)
    }

    /// Every synced project with each machine's standing. Each machine is asked once.
    pub async fn sync(&self) -> Vec<ProjectSync> {
        let specs: Vec<(String, SyncSpec)> = scan::<SyncSpec>(&self.mesh, "sync/")
            .into_iter()
            .filter_map(|(key, spec)| match key {
                Key::Sync(project) => Some((project, spec)),
                _ => None,
            })
            .collect();
        let machines = specs
            .iter()
            .flat_map(|(_, spec)| spec.folders.keys().copied())
            .collect();
        let said = self.statuses(machines).await;
        specs
            .into_iter()
            .map(|(project, spec)| ProjectSync {
                machines: spec
                    .folders
                    .iter()
                    .map(|(machine, folder)| self.machine_sync(*machine, folder, &project, &said).0)
                    .collect(),
                pinned: spec.folders.is_empty(),
                project,
            })
            .collect()
    }

    /// `project`'s sync in detail.
    pub async fn sync_detail(&self, project: &str) -> Result<ProjectSyncDetail> {
        let spec = read::<SyncSpec>(&self.mesh, &Key::Sync(project.to_owned()))
            .ok_or(FleetError::NotFound)?;
        let said = self.statuses(spec.folders.keys().copied().collect()).await;
        let machines: Vec<(MachineSync, Option<ProjectStatus>)> = spec
            .folders
            .iter()
            .map(|(machine, folder)| self.machine_sync(*machine, folder, project, &said))
            .collect();
        Ok(ProjectSyncDetail {
            project: project.to_owned(),
            pinned: spec.folders.is_empty(),
            paused: spec.paused,
            machines: machines
                .into_iter()
                .map(|(sync, seen)| MachineSyncDetail {
                    can_resolve: seen.is_some(),
                    paused: seen.as_ref().is_some_and(|status| status.paused),
                    progress: seen.as_ref().and_then(|status| status.progress).map(|p| {
                        SyncProgress {
                            files_done: p.files_done,
                            files_total: p.files_total,
                            bytes_done: p.bytes_done,
                            bytes_total: p.bytes_total,
                            rate: p.rate,
                            eta_secs: p.eta_secs,
                        }
                    }),
                    waiting: seen.map(|status| status.waiting).unwrap_or_default(),
                    sync,
                })
                .collect(),
        })
    }

    /// Pauses `project`'s sync on every machine, or resumes it.
    pub async fn pause_sync(&self, project: &str, paused: bool) -> Result<ProjectSyncDetail> {
        {
            let _write = WRITES.lock().await;
            let key = Key::Sync(project.to_owned());
            let mut spec = read::<SyncSpec>(&self.mesh, &key).ok_or(FleetError::NotFound)?;
            if spec.paused != paused {
                spec.paused = paused;
                write(&self.mesh, key, Some(&spec)).await?;
            }
        }
        self.sync_detail(project).await
    }

    /// Settles the conflict copy at `path` on `machine`.
    pub async fn resolve_conflict(
        &self,
        project: &str,
        machine: &str,
        path: &str,
        keep: ConflictKeep,
    ) -> Result<()> {
        let machine: EndpointId = machine
            .parse()
            .map_err(|_| FleetError::refused("That machine is not known."))?;
        let keep = match keep {
            ConflictKeep::Original {} => Keep::Original,
            ConflictKeep::Copy {} => Keep::Copy,
        };
        let client = self.peers.client(machine).await?;
        let reply: Reply = client
            .call(
                PROTOCOL,
                &Request::Resolve {
                    project: project.to_owned(),
                    path: path.to_owned(),
                    keep,
                },
            )
            .await?;
        match reply {
            Reply::Resolved {} => Ok(()),
            Reply::Error { message } => Err(FleetError::Refused(message)),
            _ => Err(FleetError::refused("The machine answered something else.")),
        }
    }

    /// The machines Syncthing keeps a folder of `project` in step on.
    pub async fn syncthing(&self, name: &str) -> Vec<SyncthingPlace> {
        let Some(record) = find_project(&self.mesh, name) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for seen in self.seen().await {
            let Ok(id) = seen.machine.id.parse::<EndpointId>() else {
                continue;
            };
            let (Some(place), Some(env)) = (record.places.get(&id), seen.env) else {
                continue;
            };
            if let Some(synced) = env.syncthing_folder(&place.folder) {
                out.push(SyncthingPlace {
                    machine_id: seen.machine.id,
                    folder: place.folder.clone(),
                    label: synced.name().to_owned(),
                });
            }
        }
        out
    }

    /// Every machine as `project`'s page lists it, with its checkout when it has one.
    pub async fn project_machines(&self, name: &str) -> Result<Vec<ProjectMachine>> {
        let record = find_project(&self.mesh, name).ok_or(FleetError::NotFound)?;
        Ok(self
            .mesh
            .devices()
            .into_iter()
            .map(|status| ProjectMachine {
                machine_id: status.device.id.to_string(),
                machine_name: status.device.name,
                online: status.online,
                folder: record
                    .places
                    .get(&status.device.id)
                    .map(|place| plain_path(&place.folder).into_owned()),
                checking: false,
                checked_at: None,
            })
            .collect())
    }
}
