//! How each machine stands with the projects it keeps in step. A node follows `sync/<name>` in
//! the mesh by itself; the desktop writes that key and asks each node how it is doing.

use crate::{
    desk::Desk,
    error::{FleetError, Result},
    records::{ProjectRecord, WRITES, read, scan, write},
};
use beep_mesh::Key;
use beep_proto::{
    ConflictKeep, MachineSync, MachineSyncDetail, ProjectMachine, ProjectSync, ProjectSyncDetail,
    SyncProgress, SyncStatus, SyncthingPlace,
    node::{
        mesh_state::SyncSpec,
        sync::{Keep, PROTOCOL, ProjectStatus, Reply, Request, State},
    },
};
use futures::future::join_all;
use iroh::EndpointId;

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
    /// What `machine` says of `project`; `None` when it is offline or does not answer.
    async fn node_status(&self, machine: EndpointId, project: &str) -> Option<ProjectStatus> {
        let client = self.peers.client(machine).await.ok()?;
        let reply: Reply = client.call(PROTOCOL, &Request::Status {}).await.ok()?;
        match reply {
            Reply::Status { projects } => projects.into_iter().find(|p| p.name == project),
            _ => None,
        }
    }

    fn name_of(&self, machine: EndpointId) -> String {
        self.mesh
            .devices()
            .into_iter()
            .find(|status| status.device.id == machine)
            .map_or_else(
                || machine.fmt_short().to_string(),
                |status| status.device.name,
            )
    }

    /// One machine's sync of `project`, and what it reports in detail.
    async fn machine_sync(
        &self,
        machine: EndpointId,
        folder: &str,
        project: &str,
    ) -> (MachineSync, Option<ProjectStatus>) {
        let seen = self.node_status(machine, project).await;
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
                status: SyncStatus::Offline {},
                files: 0,
                bytes: 0,
                pending: 0,
                conflicts: Vec::new(),
                rescan_secs: None,
                hint: None,
                error: None,
            },
        };
        (sync, seen)
    }

    /// Every synced project with each machine's standing.
    pub async fn sync(&self) -> Vec<ProjectSync> {
        let specs = scan::<SyncSpec>(&self.mesh, "sync/");
        join_all(specs.into_iter().filter_map(|(key, spec)| {
            let Key::Sync(project) = key else { return None };
            Some(async move {
                let machines = join_all(
                    spec.folders
                        .iter()
                        .map(|(machine, folder)| self.machine_sync(*machine, folder, &project)),
                )
                .await;
                ProjectSync {
                    project,
                    pinned: false,
                    machines: machines.into_iter().map(|(sync, _)| sync).collect(),
                }
            })
        }))
        .await
    }

    /// `project`'s sync in detail.
    pub async fn sync_detail(&self, project: &str) -> Result<ProjectSyncDetail> {
        let spec = read::<SyncSpec>(&self.mesh, &Key::Sync(project.to_owned()))
            .ok_or(FleetError::NotFound)?;
        let machines = join_all(
            spec.folders
                .iter()
                .map(|(machine, folder)| self.machine_sync(*machine, folder, project)),
        )
        .await;
        Ok(ProjectSyncDetail {
            project: project.to_owned(),
            pinned: false,
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
    pub async fn syncthing(&self, project: &str) -> Vec<SyncthingPlace> {
        let Some(record) = scan::<ProjectRecord>(&self.mesh, "project/")
            .into_iter()
            .find_map(|(key, record)| match key {
                Key::Project(name) if name.eq_ignore_ascii_case(project) => Some(record),
                _ => None,
            })
        else {
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
    pub async fn project_machines(&self, project: &str) -> Result<Vec<ProjectMachine>> {
        let record = scan::<ProjectRecord>(&self.mesh, "project/")
            .into_iter()
            .find_map(|(key, record)| match key {
                Key::Project(name) if name.eq_ignore_ascii_case(project) => Some(record),
                _ => None,
            })
            .ok_or(FleetError::NotFound)?;
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
                    .map(|place| place.folder.clone()),
                checking: false,
                checked_at: None,
            })
            .collect())
    }
}
