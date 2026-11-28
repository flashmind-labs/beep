//! How each machine stands with the projects it keeps in step. A node follows `sync/<name>` in
//! the mesh by itself; the desktop writes that key and asks each node how it is doing.

mod ignored;

use crate::{
    desk::Desk,
    direct_syncs::{SYNC_FIRST_WAIT, fetch_node_statuses},
    error::{FleetError, Result},
    records::{WRITES, find_project, project_key, read, scan, write},
};
use beep_mesh::{Key, PeerClient};
use beep_projects::model::plain_path;
use beep_proto::{
    AgentSlug, ConflictKeep, MachineSync, MachineSyncDetail, ProjectMachine, ProjectSync,
    ProjectSyncDetail, SyncProgress, SyncStatus, SyncthingPlace, check_agent,
    node::{
        events::{Read, Value},
        mesh_state::{ProjectSyncSettings, SyncSpec},
        sync::{Keep, PROTOCOL, ProjectStatus, Reply, Request, State},
    },
};
use futures::{StreamExt, future::join_all, stream};
use iroh::EndpointId;
use std::{
    collections::{BTreeSet, HashMap},
    time::Duration,
};
use tokio::time::timeout;

/// Machines asked about their sync at once.
const MACHINES_AT_ONCE: usize = 8;
/// How long a subscribed sync read waits for the stream before asking the node directly.
const SNAPSHOT_POLL_WAIT: Duration = Duration::from_millis(50);
/// How long a Syncthing folder-share check waits on one machine.
const SYNCTHING_CALL_LIMIT: Duration = Duration::from_secs(3);
/// Why a machine that answered without the project does not sync it: a node from before it
/// reported the projects it refused.
const NOT_LISTED: &str =
    "Its beep-node does not sync this project. Update beep-node there to see why.";
/// Why an online machine says nothing of its sync.
const NO_ANSWER: &str = "It did not say how its sync stands.";

/// `request` to `client`, a refusal from the node as an error.
async fn sync_call(client: &PeerClient, request: &Request) -> Result<Reply> {
    match client.call(PROTOCOL, request).await? {
        Reply::Error { message } => Err(FleetError::Refused(message)),
        reply => Ok(reply),
    }
}

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
    pub fn project_sync_enabled(&self) -> bool {
        read::<ProjectSyncSettings>(&self.mesh, &Key::ProjectSync)
            .unwrap_or_default()
            .enabled
    }

    /// Existing nodes honor the pause through their existing per-project records.
    pub async fn set_project_sync_enabled(&self, enabled: bool) -> Result<()> {
        let _write = WRITES.lock().await;
        let mut settings =
            read::<ProjectSyncSettings>(&self.mesh, &Key::ProjectSync).unwrap_or_default();
        let specs = scan::<SyncSpec>(&self.mesh, "sync/");

        if !enabled {
            if settings.enabled {
                settings.paused_projects = specs
                    .iter()
                    .filter_map(|(key, spec)| match key {
                        Key::Sync(name) if spec.paused => Some(name.clone()),
                        _ => None,
                    })
                    .collect();
            }
            settings.enabled = false;
            write(&self.mesh, Key::ProjectSync, Some(&settings)).await?;
        }

        for (key, mut spec) in specs {
            let Key::Sync(name) = &key else {
                continue;
            };
            let paused = !enabled
                || if settings.enabled {
                    spec.paused
                } else {
                    settings.paused_projects.contains(name)
                };
            if spec.paused != paused {
                spec.paused = paused;
                write(&self.mesh, key, Some(&spec)).await?;
            }
        }

        if enabled {
            settings.enabled = true;
            settings.paused_projects.clear();
            write(&self.mesh, Key::ProjectSync, Some(&settings)).await?;
        }
        Ok(())
    }

    pub async fn project_git_auto_recovery(
        &self,
        project: &str,
        enabled: Option<bool>,
    ) -> Result<bool> {
        let _write = WRITES.lock().await;
        let mut stored = find_project(&self.mesh, project).ok_or(FleetError::NotFound)?;

        if let Some(enabled) = enabled {
            stored.record.git_auto_recovery = enabled;
            write(&self.mesh, project_key(&stored.name), Some(&stored.record)).await?;
        }

        Ok(stored.record.git_auto_recovery)
    }

    /// The harnesses whose sessions may run in `project`; `None` is every harness.
    pub fn project_harnesses(&self, project: &str) -> Result<Option<Vec<AgentSlug>>> {
        let stored = find_project(&self.mesh, project).ok_or(FleetError::NotFound)?;
        Ok(stored.record.harnesses)
    }

    /// Saves the harnesses whose sessions may run in `project`, for every machine.
    pub async fn save_project_harnesses(
        &self,
        project: &str,
        harnesses: Option<Vec<AgentSlug>>,
    ) -> Result<()> {
        for agent in harnesses.iter().flatten() {
            check_agent(agent.as_ref())?;
        }

        let _write = WRITES.lock().await;
        let mut stored = find_project(&self.mesh, project).ok_or(FleetError::NotFound)?;
        stored.record.harnesses = harnesses;
        write(&self.mesh, project_key(&stored.name), Some(&stored.record)).await
    }

    /// A replicated choice, applied on each machine when it is reachable.
    pub async fn project_syncthing_mode(
        &self,
        project: &str,
        enabled: Option<bool>,
    ) -> Result<Option<bool>> {
        let _write = WRITES.lock().await;
        let mut stored = find_project(&self.mesh, project).ok_or(FleetError::NotFound)?;

        if let Some(enabled) = enabled {
            stored.record.syncthing_files = Some(enabled);
            write(&self.mesh, project_key(&stored.name), Some(&stored.record)).await?;
        }

        let sync = read::<ProjectSyncSettings>(&self.mesh, &Key::ProjectSync).unwrap_or_default();
        Ok(stored.record.syncthing_choice(&sync))
    }

    /// Brings an explicit snapshot to a clean checkout. The source stays intact.
    pub async fn handoff_work(&self, project: &str, from: &str, to: &str) -> Result<()> {
        let stored = find_project(&self.mesh, project).ok_or(FleetError::NotFound)?;
        let from: EndpointId = from
            .parse()
            .map_err(|_| FleetError::refused("Invalid source machine."))?;
        let to: EndpointId = to
            .parse()
            .map_err(|_| FleetError::refused("Invalid destination machine."))?;
        if from == to || !stored.places.contains_key(&from) || !stored.places.contains_key(&to) {
            return Err(FleetError::refused(
                "Choose two machines with checkouts of this project.",
            ));
        }

        let source = self.machines.client(from).await?;
        let reply = sync_call(
            &source,
            &Request::HandoffSnapshot {
                project: stored.name.clone(),
            },
        )
        .await?;
        let snapshot = match reply {
            Reply::HandoffSnapshot { snapshot } => snapshot,
            _ => {
                return Err(FleetError::refused(
                    "Update the source machine to hand off work.",
                ));
            }
        };
        let target = self.machines.client(to).await?;
        let reply = sync_call(
            &target,
            &Request::HandoffApply {
                project: stored.name,
                snapshot,
            },
        )
        .await?;
        match reply {
            Reply::HandoffApplied {} => Ok(()),
            _ => Err(FleetError::refused(
                "Update the destination machine to hand off work.",
            )),
        }
    }

    /// Changes only the selected project's path in this machine's Syncthing share.
    pub async fn syncthing_files(
        &self,
        machine: &str,
        folder: &str,
        enabled: Option<bool>,
    ) -> Result<beep_proto::node::sync::SyncthingFiles> {
        let client = self
            .machines
            .client(
                machine
                    .parse()
                    .map_err(|_| FleetError::refused("Invalid machine ID"))?,
            )
            .await?;
        let reply = sync_call(
            &client,
            &Request::SyncthingFiles {
                folder: folder.to_owned(),
                enabled,
            },
        )
        .await?;
        match reply {
            Reply::SyncthingFiles { files } => Ok(files),
            _ => Err(FleetError::refused(
                "Update this machine to change Syncthing project file sync.",
            )),
        }
    }

    /// What `machine` says of every project it syncs; `None` when it is offline or does not
    /// answer. Read from the machine's subscription, which the node keeps pushing. A node that
    /// does not push is read from its last direct answer.
    pub(crate) async fn node_statuses(&self, machine: EndpointId) -> Option<Vec<ProjectStatus>> {
        if !self.is_online(machine) {
            self.hosts.direct_syncs.forget(machine);
            return None;
        }

        // A node that pushed before gets the time a first direct read gets, for the client, the
        // snapshot and a direct call together.
        if self.hosts.direct_syncs.pushes(machine) {
            return timeout(
                SYNC_FIRST_WAIT,
                self.read_statuses(machine, SYNC_FIRST_WAIT),
            )
            .await
            .ok()
            .flatten();
        }

        self.read_statuses(machine, SNAPSHOT_POLL_WAIT).await
    }

    /// `machine`'s statuses from its subscription, each step waiting `wait` at most.
    async fn read_statuses(
        &self,
        machine: EndpointId,
        wait: Duration,
    ) -> Option<Vec<ProjectStatus>> {
        let direct = &self.hosts.direct_syncs;
        let client = timeout(wait, self.machines.client(machine))
            .await
            .ok()
            .and_then(Result::ok);

        if let Some(client) = &client {
            match timeout(wait, client.snapshot(Read::SyncList)).await {
                Ok(Some(Ok(Value::Sync(reply)))) => {
                    direct.pushed(machine);

                    return match *reply {
                        Reply::Status { projects } => Some(projects),
                        _ => None,
                    };
                }
                // The node refuses the topic now, as an older one does: it is read directly.
                Ok(Some(Err(_))) => direct.unpushed(machine),
                _ => {}
            }
        }

        // Its subscription is not live: ask once and keep nothing, the push comes back.
        if direct.pushes(machine) {
            return fetch_node_statuses(&client?).await;
        }

        direct.read(machine, client).await
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
        ignored: &beep_proto::node::sync::IgnoredSync,
        said: &HashMap<EndpointId, Vec<ProjectStatus>>,
    ) -> (MachineSync, Option<ProjectStatus>) {
        let seen = said.get(&machine).and_then(|all| {
            all.iter()
                .find(|status| status.name == project)
                .map(|main| ignored.status(main, all))
        });
        let listed = said.contains_key(&machine);
        let online = self.is_online(machine);
        let (status, error) = match &seen {
            Some(status) => (
                status_of(status.state, status.error.is_some()),
                status.error.clone(),
            ),
            // Offline only when it is: a machine that answered without the project, or that
            // the mesh sees but did not answer, does not sync it and says why.
            None if listed => (SyncStatus::Failed {}, Some(NOT_LISTED.to_owned())),
            None if online => (SyncStatus::Failed {}, Some(NO_ANSWER.to_owned())),
            None => (SyncStatus::Offline {}, None),
        };
        let mut sync = MachineSync {
            git_only: false,
            machine_id: machine.to_string(),
            machine_name: self.mesh.name_of(machine),
            folder: folder.to_owned(),
            status,
            files: 0,
            bytes: 0,
            pending: 0,
            conflicts: Vec::new(),
            rescan_secs: None,
            hint: None,
            error,
        };

        if let Some(status) = &seen {
            sync.git_only = status.git_only;
            sync.files = status.files;
            sync.bytes = status.bytes;
            sync.pending = status.pending;
            sync.conflicts = status.conflicts.clone();
            sync.rescan_secs = status.rescan_secs;
            sync.hint = status.hint.clone();
        }

        (sync, seen)
    }

    fn is_online(&self, machine: EndpointId) -> bool {
        self.mesh
            .code_hosts()
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
                    .map(|(machine, folder)| {
                        self.machine_sync(*machine, folder, &project, &spec.ignored, &said)
                            .0
                    })
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
            .map(|(machine, folder)| {
                self.machine_sync(*machine, folder, project, &spec.ignored, &said)
            })
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
                    progress: seen
                        .as_ref()
                        .and_then(|status| status.progress.clone())
                        .map(|p| SyncProgress {
                            files_done: p.files_done,
                            files_total: p.files_total,
                            bytes_done: p.bytes_done,
                            bytes_total: p.bytes_total,
                            rate: p.rate,
                            eta_secs: p.eta_secs,
                            file: p.file,
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
            let mut master =
                read::<ProjectSyncSettings>(&self.mesh, &Key::ProjectSync).unwrap_or_default();
            let paused = if master.enabled {
                paused
            } else {
                if paused {
                    master.paused_projects.insert(project.to_owned());
                } else {
                    master.paused_projects.remove(project);
                }
                write(&self.mesh, Key::ProjectSync, Some(&master)).await?;
                true
            };

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
        let spec = read::<SyncSpec>(&self.mesh, &Key::Sync(project.to_owned()));
        let selection = spec.as_ref().filter(|s| s.ignored.enabled).and_then(|s| {
            s.ignored.folders.iter().find_map(|folder| {
                path.strip_prefix(&format!("{folder}/")).map(|relative| {
                    (
                        beep_proto::node::sync::ignored_project(project, folder),
                        relative,
                    )
                })
            })
        });
        let (dataset, relative) = selection.unwrap_or_else(|| (project.to_owned(), path));
        let client = self.machines.client(machine).await?;
        let reply = sync_call(
            &client,
            &Request::Resolve {
                project: dataset,
                path: relative.to_owned(),
                keep,
            },
        )
        .await?;
        match reply {
            Reply::Resolved {} => Ok(()),
            _ => Err(FleetError::refused("The machine answered something else.")),
        }
    }

    /// Starts `project`'s sync over on `machine` in the folder now at its
    /// path, as for one that was replaced; nothing is deleted elsewhere.
    pub async fn start_sync_over(&self, project: &str, machine: &str) -> Result<()> {
        let machine: EndpointId = machine
            .parse()
            .map_err(|_| FleetError::refused("That machine is not known."))?;
        let client = self.machines.client(machine).await?;
        let reply = sync_call(
            &client,
            &Request::StartOver {
                project: project.to_owned(),
            },
        )
        .await?;

        match reply {
            Reply::StartedOver {} => Ok(()),
            _ => Err(FleetError::refused("The machine answered something else.")),
        }
    }

    /// Whether Syncthing keeps a folder in step on any machine that reported.
    pub async fn syncthing_in_use(&self) -> bool {
        self.seen().await.iter().any(|seen| {
            seen.env
                .as_ref()
                .is_some_and(|env| !env.syncthing.is_empty())
        })
    }

    /// The machines Syncthing keeps a folder of `project` in step on.
    pub async fn syncthing(&self, name: &str) -> Vec<SyncthingPlace> {
        let Some(record) = find_project(&self.mesh, name) else {
            return Vec::new();
        };
        let ids: Vec<EndpointId> = record.places.keys().copied().collect();
        let seen = self.seen_among(&ids).await;

        // Each machine is asked at once: a slow one costs one limit, not one per machine.
        join_all(seen.into_iter().map(|seen| async {
            let id = seen.machine.id.parse::<EndpointId>().ok()?;
            let (place, env) = (record.places.get(&id)?, seen.env?);
            let synced = env.syncthing_folder(&place.folder)?;
            let files = timeout(
                SYNCTHING_CALL_LIMIT,
                self.syncthing_files(&seen.machine.id, &place.folder, None),
            )
            .await;

            if files.is_ok_and(|res| res.is_ok_and(|files| !files.enabled)) {
                return None;
            }

            Some(SyncthingPlace {
                machine_id: seen.machine.id,
                folder: place.folder.clone(),
                label: synced.name().to_owned(),
            })
        }))
        .await
        .into_iter()
        .flatten()
        .collect()
    }

    /// Every machine as `project`'s page lists it, with its checkout when it has one.
    pub async fn project_machines(&self, name: &str) -> Result<Vec<ProjectMachine>> {
        let record = find_project(&self.mesh, name).ok_or(FleetError::NotFound)?;
        Ok(self
            .mesh
            .code_hosts()
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

#[cfg(test)]
#[path = "tests/sync.rs"]
mod tests;
