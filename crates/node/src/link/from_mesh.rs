//! A node in a mesh takes its synced projects, credentials and agent
//! settings from the mesh's replicated state (`sync/*`, `credential/*`,
//! `harness/*`, `flsh`). The state file the mesh keeps is the only copy on
//! disk, so the credentials come back after a restart with no second file to
//! guard.
//!
//! `follow` runs while the node is in the mesh. Each change to the state or
//! the device list applies what differs from what was applied before.

use std::time::Duration;
use crate::{
    link::Offers,
    link::watchdog::{self, Step},
};
use beep_mesh::{Key, Mesh};
use beep_proto::{
    ApiProvider, FlshSettings,
    node::{
        MachineCredentials,
        harness::{AgentSettings, HarnessConfig},
        mesh_state::{Credential, Place, ProjectRecord, StateValue, SyncSpec},
        sync::{Project, ignored_project},
    },
};
use iroh::{EndpointAddr, EndpointId};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
    time::Instant,
};
use tokio::sync::{broadcast::error::RecvError, watch};

/// The mesh the node belongs to, once it does.
#[derive(Clone)]
pub(crate) struct Membership {
    mesh: Arc<watch::Sender<Option<Mesh>>>,
}

impl Default for Membership {
    fn default() -> Self {
        Self {
            mesh: Arc::new(watch::Sender::new(None)),
        }
    }
}

impl Membership {
    pub(crate) fn set(&self, mesh: Mesh) {
        self.mesh.send_replace(Some(mesh));
    }

    pub(crate) fn clear(&self) {
        self.mesh.send_replace(None);
    }

    pub(crate) fn subscribe(&self) -> watch::Receiver<Option<Mesh>> {
        self.mesh.subscribe()
    }

    pub(crate) fn get(&self) -> Option<Mesh> {
        self.mesh.borrow().clone()
    }
}

/// Applies the mesh's state to the node now and on every change, until the
/// mesh stops.
pub(crate) async fn follow(mesh: Mesh, me: EndpointId, offers: Offers) {
    // Subscribed before the first scan, so no change falls between the two.
    let mut changes = mesh.changes();
    let mut updates = mesh.updates();
    let mut applied = Applied::default();
    // Slow or stopped Syncthing must not delay credentials and project membership.
    let preferences_mesh = mesh.clone();
    let preferences_offers = offers.clone();
    let _preferences = tokio_util::task::AbortOnDropHandle::new(tokio::spawn(async move {
        let mut changes = preferences_mesh.changes();

        loop {
            apply_syncthing(&preferences_mesh, me, &preferences_offers).await;
            tokio::select! {
                change = changes.recv() => if matches!(change, Err(RecvError::Closed)) { return; },
                () = tokio::time::sleep(Duration::from_secs(30)) => {},
            }
        }
    }));

    loop {
        applied.apply(&mesh, me, &offers).await;
        tokio::select! {
            change = changes.recv() => match change {
                // A receiver that fell behind rescans, which every pass does.
                Ok(_) | Err(RecvError::Lagged(_)) => {}
                Err(RecvError::Closed) => return,
            },
            moved = updates.changed() => if moved.is_err() {
                return;
            },
            () = tokio::time::sleep(Duration::from_secs(30)) => {},
        }
    }
}

/// Each machine applies the replicated preference to its own local share.
async fn apply_syncthing(mesh: &Mesh, me: EndpointId, offers: &Offers) {
    let Some(transfers) = offers.transfers.get() else {
        return;
    };
    let syncs = transfers.syncs();

    for (key, bytes) in mesh.scan("project/") {
        let Key::Project(name) = key else {
            continue;
        };
        let Ok(record) = ProjectRecord::from_bytes(&bytes) else {
            continue;
        };
        let Some(enabled) = record.syncthing_files else {
            continue;
        };
        let Some(bytes) = mesh.get(&Key::Project(format!("{name}/{me}"))) else {
            continue;
        };
        let Ok(place) = Place::from_bytes(&bytes) else {
            continue;
        };

        match syncs.syncthing_files(&place.folder, None).await {
            Ok(files) if !files.available || files.enabled == enabled => {}
            Ok(_) => {
                if let Err(e) = syncs.syncthing_files(&place.folder, Some(enabled)).await {
                    log::warn!("project Syncthing preference is pending: {e:#}");
                }
            }
            Err(e) => log::debug!("project Syncthing preference could not be checked: {e:#}"),
        }
    }
}

/// What the node holds from the mesh now, to apply only what changed.
#[derive(Default)]
struct Applied {
    credentials: Option<MachineCredentials>,
    settings: Option<AgentSettings>,
    projects: Option<Vec<Project>>,
    /// The projects among them that name a machine that has not joined.
    pending: BTreeSet<String>,
    /// The last spec of each project that read, for a key that stops reading.
    specs: BTreeMap<Key, SyncSpec>,
}

impl Applied {
    async fn apply(&mut self, mesh: &Mesh, me: EndpointId, offers: &Offers) {
        if let Some(provision) = &offers.provision {
            let credentials = credentials(mesh);

            if self.credentials.as_ref() != Some(&credentials) {
                provision.apply(credentials.clone()).await;
                log::info!(
                    "from the mesh: {} credentials",
                    credentials.env.len() + credentials.forgejo.len()
                );
                self.credentials = Some(credentials);
            }
        }

        let settings = settings(mesh);

        if self.settings.as_ref() != Some(&settings) {
            offers.agents.set_settings(settings.clone());
            offers.agents.settings_arrived();
            log::info!(
                "from the mesh: settings for {} agents",
                settings.harnesses.len()
            );
            self.settings = Some(settings);
        }

        if let Some(transfers) = offers.transfers.get() {
            let (projects, pending) = projects(mesh, me, &mut self.specs);

            if self.projects.as_ref() != Some(&projects) || self.pending != pending {
                let started = Instant::now();
                log::info!(
                    "from the mesh: applying the sync set, {} projects, {} waiting for a machine",
                    projects.len(),
                    pending.len()
                );
                let applying = watchdog::enter(Step::SyncApply);
                let refused = transfers
                    .syncs()
                    .configure_from_mesh(projects.clone(), pending.clone())
                    .await;
                drop(applying);

                for refused in &refused {
                    log::warn!("not syncing {}: {}", refused.project, refused.message);
                }

                for project in &projects {
                    log::info!(
                        "from the mesh: sync {} with {} peers",
                        project.name,
                        project.peers.len()
                    );
                }

                log::info!(
                    "from the mesh: applied the sync set in {} ms, {} refused",
                    started.elapsed().as_millis(),
                    refused.len()
                );
                self.projects = Some(projects);
                self.pending = pending;
            }

            transfers.syncs().set_git_auto_recovery(
                &offers.agents,
                mesh.scan("project/")
                    .into_iter()
                    .filter_map(|(key, bytes)| {
                        let Key::Project(name) = key else {
                            return None;
                        };

                        if !offers.agents.reopened()
                            || !self.specs.contains_key(&Key::Sync(name.clone()))
                        {
                            return None;
                        }

                        let record = ProjectRecord::from_bytes(&bytes).ok()?;
                        record.git_auto_recovery.then_some(name)
                    })
                    .collect(),
            );
        }
    }
}

/// The values under `prefix` that read as `T`; one that does not is left
/// out and its key logged, never its content.
fn read<T: StateValue>(mesh: &Mesh, prefix: &str) -> Vec<(Key, T)> {
    mesh.scan(prefix)
        .into_iter()
        .filter_map(|(key, bytes)| match T::from_bytes(&bytes) {
            Ok(value) => Some((key, value)),
            Err(_) => {
                log::warn!("ignored {key} in the mesh: it does not read");
                None
            }
        })
        .collect()
}

/// The base URLs of the Forgejo and Gitea servers the mesh holds logins for.
pub(crate) fn forgejo_servers(mesh: &Mesh) -> Vec<String> {
    credentials(mesh)
        .forgejo
        .into_iter()
        .map(|login| login.url)
        .collect()
}

/// The credentials the node uses. An API key an older desktop put in the mesh is left out: API
/// keys are kept on the server only.
fn credentials(mesh: &Mesh) -> MachineCredentials {
    let mut out = MachineCredentials::default();

    for (_, credential) in read::<Credential>(mesh, "credential/") {
        match credential {
            Credential::Env(secret)
                if ApiProvider::all().any(|provider| provider.env_var() == secret.name) => {}
            Credential::Env(secret) => out.env.push(secret),
            Credential::Forgejo(login) => out.forgejo.push(login),
        }
    }

    out
}

fn settings(mesh: &Mesh) -> AgentSettings {
    let flsh = mesh
        .get(&Key::Flsh)
        .and_then(|bytes| match FlshSettings::from_bytes(&bytes) {
            Ok(flsh) => Some(flsh),
            Err(_) => {
                log::warn!("ignored flsh in the mesh: it does not read");
                None
            }
        });
    let named = read::<HarnessConfig>(mesh, "harness/")
        .into_iter()
        .filter_map(|(key, config)| match key {
            Key::Harness(name) => Some((name, config)),
            _ => None,
        })
        .collect();
    AgentSettings {
        harnesses: harnesses_for(&mesh.me().to_string(), named),
        flsh,
    }
}

/// The harness settings this device runs with. `harness/<agent>` is the same on every device;
/// `harness/<agent>/<device>` holds what is a path on one machine, the program and the home folder,
/// and only the one named for this device counts, so a Mac's path never reaches another machine.
fn harnesses_for(me: &str, named: Vec<(String, HarnessConfig)>) -> Vec<HarnessConfig> {
    let mut shared = Vec::new();
    let mut mine = BTreeMap::new();

    for (name, config) in named {
        match name.split_once('/') {
            None => shared.push(config),
            Some((agent, device)) if device == me => {
                mine.insert(agent.to_owned(), config);
            }
            Some(_) => {}
        }
    }

    for config in &mut shared {
        // Whatever a shared value says about paths is not for this machine; only its own key is.
        config.program = None;
        config.home = None;
        if let Some(own) = mine.remove(&config.agent) {
            config.program = own.program;
            config.home = own.home;
        }
    }

    shared.extend(mine.into_values());
    shared
}

/// The projects this device syncs: those whose spec names it, with the
/// other devices of the spec that are in the mesh now, and the names of those
/// that name a machine that is neither in the mesh nor removed from it. A
/// spec that stops reading keeps the last one that did, in `specs`.
fn projects(
    mesh: &Mesh,
    me: EndpointId,
    specs: &mut BTreeMap<Key, SyncSpec>,
) -> (Vec<Project>, BTreeSet<String>) {
    let devices = mesh.code_hosts();
    let mut now = BTreeMap::new();

    for (key, bytes) in mesh.scan("sync/") {
        match SyncSpec::from_bytes(&bytes) {
            Ok(spec) => {
                now.insert(key, spec);
            }
            Err(_) => {
                log::warn!("ignored {key} in the mesh: it does not read");
                if let Some(last) = specs.get(&key) {
                    now.insert(key, last.clone());
                }
            }
        }
    }
    *specs = now;
    let pending = specs
        .iter()
        .filter_map(|(key, spec)| match key {
            Key::Sync(name) => Some((name, spec)),
            _ => None,
        })
        .filter(|(_, spec)| {
            spec.folders
                .keys()
                .any(|id| *id != me && !mesh.is_member(*id) && !mesh.is_removed(*id))
        })
        .map(|(name, _)| name.clone())
        .collect();
    let projects = specs
        .iter()
        .filter_map(|(key, spec)| {
            let Key::Sync(name) = key else {
                return None;
            };
            let folder = spec.folders.get(&me)?.clone();
            let peers = spec
                .folders
                .keys()
                .filter(|id| **id != me)
                .filter_map(|id| {
                    let device = devices.iter().find(|status| status.device.id == *id)?;
                    Some(
                        device
                            .addr
                            .clone()
                            .unwrap_or_else(|| EndpointAddr::new(*id)),
                    )
                })
                .collect();
            let main = Project {
                name: name.clone(),
                folder,
                peers,
                paused: spec.paused,
            };
            let mut all = vec![main.clone()];

            if let Ok(config) = spec.ignored.clone().normalized()
                && config.enabled
            {
                all.extend(config.folders.iter().map(|path| {
                    Project {
                        name: ignored_project(name, path),
                        folder: std::path::Path::new(&main.folder)
                            .join(path)
                            .to_string_lossy()
                            .into_owned(),
                        peers: main.peers.clone(),
                        paused: main.paused,
                    }
                }));
            }

            Some(all)
        })
        .flatten()
        .collect();
    (projects, pending)
}

#[cfg(test)]
#[path = "../tests/link_from_mesh.rs"]
mod tests;
