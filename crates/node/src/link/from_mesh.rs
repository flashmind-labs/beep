//! A node in a mesh takes its synced projects, credentials and agent
//! settings from the mesh's replicated state (`sync/*`, `credential/*`,
//! `harness/*`, `flsh`). The state file the mesh keeps is the only copy on
//! disk, so the credentials come back after a restart with no second file to
//! guard.
//!
//! `follow` runs while the node is in the mesh. Each change to the state or
//! the device list applies what differs from what was applied before.

use crate::{
    membership::{credentials, read},
    files::{sync::Syncs, syncthing::Syncthing},
    link::Offers,
    link::watchdog::{self, Step},
};
use beep_mesh::{Key, Mesh};
use beep_proto::watched;
use beep_proto::{
    FlshSettings,
    node::{
        MachineCredentials,
        harness::{AgentSettings, HarnessConfig},
        mesh_state::{
            Place, ProjectRecord, ProjectSyncSettings, SavedVersion, StateValue,
            SyncSpec,
        },
        sync::{Project, ignored_project},
    },
};
use iroh::{EndpointAddr, EndpointId};
use std::{collections::BTreeMap, sync::Arc, time::Instant};
use tokio::sync::{broadcast::error::RecvError, watch};
use tokio_util::task::AbortOnDropHandle;

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
        let mut transfers = preferences_offers.transfers.subscribe();
        // Syncthing's own event stream says when it is back, or changed a folder.
        let syncthing_moved = Arc::new(tokio::sync::Notify::new());
        let _events = AbortOnDropHandle::new(tokio::spawn(syncthing_events(syncthing_moved.clone())));

        loop {
            super::project_remotes::apply(placed_projects(&preferences_mesh, me)).await;
            apply_syncthing(&preferences_mesh, me, &preferences_offers).await;
            tokio::select! {
                change = changes.recv() => if matches!(change, Err(RecvError::Closed)) { return; },
                Ok(()) = transfers.changed() => {}
                () = syncthing_moved.notified() => {}
            }
        }
    }));
    let mut transfers = offers.transfers.subscribe();
    let reopened = offers.agents.until_reopened();
    tokio::pin!(reopened);
    let mut waiting_reopen = true;

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
            // The sync set waits for the node's transfers to start.
            Ok(()) = transfers.changed() => {}
            // Git recovery waits for the sessions open before a restart.
            () = &mut reopened, if waiting_reopen => waiting_reopen = false,
        }
    }
}

/// Rings `moved` whenever Syncthing's event stream has news, which includes
/// Syncthing coming back after being away.
async fn syncthing_events(moved: Arc<tokio::sync::Notify>) {
    let syncthing = Syncthing::local();
    let mut since = 0;

    loop {
        let before = since;
        syncthing.changed(&mut since).await;

        if since > before {
            moved.notify_one();
        }
    }
}

/// The projects in the mesh that `me` has a checkout of, with where it is.
pub(crate) fn placed_projects(
    mesh: &Mesh,
    me: EndpointId,
) -> impl Iterator<Item = (String, ProjectRecord, Place)> {
    mesh.scan("project/")
        .into_iter()
        .filter_map(move |(key, bytes)| {
            let Key::Project(name) = key else {
                return None;
            };
            let record = ProjectRecord::from_bytes(&bytes).ok()?;
            let place = mesh.get(&Key::Project(format!("{name}/{me}")))?;
            let place = Place::from_bytes(&place).ok()?;
            Some((name, record, place))
        })
}

/// Each machine applies the replicated preference to its own local share.
async fn apply_syncthing(mesh: &Mesh, me: EndpointId, offers: &Offers) {
    let Some(transfers) = offers.transfers.get() else {
        return;
    };
    let syncs = transfers.syncs();
    let sync = mesh
        .get(&Key::ProjectSync)
        .and_then(|bytes| ProjectSyncSettings::from_bytes(&bytes).ok())
        .unwrap_or_default();

    // Each folder's check and change run on their own, all together.
    let preferences = placed_projects(mesh, me).filter_map(|(_, record, place)| {
        let enabled = record.syncthing_choice(&sync)?;
        let syncs = &syncs;

        Some(async move {
            match syncs.syncthing_files(&place.folder, None).await {
                Ok(files) if !files.available || files.enabled == enabled => {}
                Ok(_) => {
                    if let Err(e) = syncs.syncthing_files(&place.folder, Some(enabled)).await {
                        log::warn!("project Syncthing preference is pending: {e:#}");
                    }
                }
                Err(e) => log::debug!("project Syncthing preference could not be checked: {e:#}"),
            }
        })
    });
    futures::future::join_all(preferences).await;
}

/// Match the desktop's newest-version ordering, including concurrent saves.
fn instructions(mesh: &Mesh) -> Option<String> {
    mesh.scan("instructions/")
        .into_iter()
        .filter_map(|(key, bytes)| {
            let saved = SavedVersion::from_bytes(&bytes).ok()?;
            Some((saved.saved_ms, key, saved.text))
        })
        .max_by(|a, b| (&a.0, &a.1).cmp(&(&b.0, &b.1)))
        .map(|(_, _, text)| text)
}

/// What the node holds from the mesh now, to apply only what changed.
#[derive(Default)]
struct Applied {
    credentials: Option<MachineCredentials>,
    settings: Option<AgentSettings>,
    projects: Option<Vec<Project>>,
    /// Applies the sync set on its own task, once the node's transfers run.
    sync_set: Option<SyncSet>,
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

        offers.agents.set_instructions(instructions(mesh));

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
            let projects = projects(mesh, me, &mut self.specs);

            if self.projects.as_ref() != Some(&projects) {
                // The set applies on its own task: credentials and settings
                // do not wait for projects to halt and load.
                let sync_set = self
                    .sync_set
                    .get_or_insert_with(|| SyncSet::start(transfers.syncs()));
                sync_set.set(projects.clone());
                self.projects = Some(projects);
            }

            offers.agents.follow_recoveries(&transfers.syncs());
            transfers.syncs().set_git_auto_recovery(
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

/// The task that applies the mesh's sync set. A set that arrives while one
/// applies replaces any set still waiting: only the newest applies next.
struct SyncSet {
    sets: watch::Sender<Option<Vec<Project>>>,
    _task: AbortOnDropHandle<()>,
}

impl SyncSet {
    fn start(syncs: Syncs) -> Self {
        let (sets, rx) = watch::channel(None);
        let task = AbortOnDropHandle::new(tokio::spawn(apply_sync_sets(syncs, rx)));

        Self { sets, _task: task }
    }

    fn set(&self, projects: Vec<Project>) {
        self.sets.send_replace(Some(projects));
    }
}

async fn apply_sync_sets(syncs: Syncs, mut sets: watch::Receiver<Option<Vec<Project>>>) {
    while sets.changed().await.is_ok() {
        let Some(projects) = watched::read_and_update(&mut sets, Clone::clone) else {
            continue;
        };
        let started = Instant::now();
        log::info!(
            "from the mesh: applying the sync set, {} projects",
            projects.len(),
        );
        let applying = watchdog::enter(Step::SyncApply);
        let refused = syncs.configure_from_mesh(projects.clone()).await;
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
    }
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
/// other devices of the spec that are in the mesh now. A spec that stops
/// reading keeps the last one that did, in `specs`.
fn projects(mesh: &Mesh, me: EndpointId, specs: &mut BTreeMap<Key, SyncSpec>) -> Vec<Project> {
    let devices = mesh.code_hosts();
    let sync_enabled = mesh
        .get(&Key::ProjectSync)
        .and_then(|bytes| ProjectSyncSettings::from_bytes(&bytes).ok())
        .unwrap_or_default()
        .enabled;
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

    specs
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
                paused: spec.paused || !sync_enabled,
                rules: spec.rules.clone(),
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
                        rules: String::new(),
                    }
                }));
            }

            Some(all)
        })
        .flatten()
        .collect()
}

#[cfg(test)]
#[path = "../tests/link_from_mesh.rs"]
mod tests;
