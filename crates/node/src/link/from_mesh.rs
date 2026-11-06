//! A node in a mesh takes its synced projects, credentials and agent
//! settings from the mesh's replicated state (`sync/*`, `credential/*`,
//! `harness/*`, `flsh`), and ignores what a server pushes for them. The
//! state file the mesh keeps is the only copy on disk, so the credentials
//! come back after a restart with no second file to guard.
//!
//! `follow` runs while the node is in the mesh. Each change to the state or
//! the device list applies what differs from what was applied before.
//!
//! The mesh takes over one family of state (sync, credentials, harness
//! settings, flsh) only once it has held a key of that family, a deleted one
//! included, or an `unmoved/...` record of it: a moved account cuts the
//! server's credentials and harness values off at once. Until then the node
//! keeps what the server set and keeps taking the server's pushes for it, so a
//! running node that joins a mesh with nothing in it loses nothing.

use crate::{
    AsyncMutex,
    link::Offers,
    link::watchdog::{self, Step},
};
use beep_mesh::{Key, Mesh};
use beep_proto::{
    FlshSettings,
    node::{
        MachineCredentials,
        harness::{AgentSettings, HarnessConfig},
        mesh_state::{Credential, StateValue, SyncSpec},
        sync::Project,
    },
};
use iroh::{EndpointAddr, EndpointId};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
    time::Instant,
};
use tokio::sync::{MutexGuard, broadcast::error::RecvError, watch};

/// A kind of state the mesh can own on the node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Family {
    Sync,
    Credentials,
    Harnesses,
    Flsh,
}

impl Family {
    const ALL: [Family; 4] = [
        Family::Sync,
        Family::Credentials,
        Family::Harnesses,
        Family::Flsh,
    ];

    /// What the text form of every key of this family starts with.
    fn prefix(self) -> &'static str {
        match self {
            Family::Sync => "sync/",
            Family::Credentials => "credential/",
            Family::Harnesses => "harness/",
            Family::Flsh => "flsh",
        }
    }

    /// What the text form of a record of a value the account move could not bring starts with,
    /// for the families that hold secrets.
    fn unmoved(self) -> Option<&'static str> {
        match self {
            Family::Credentials => Some("unmoved/credential/"),
            Family::Harnesses => Some("unmoved/harness/"),
            Family::Sync | Family::Flsh => None,
        }
    }

    /// Whether the mesh owns this family: it has held a key of it, or says by an `unmoved/...`
    /// record that the account moved and the server's values are not to be used. Until then the
    /// server's.
    pub(crate) fn held(self, mesh: &Mesh) -> bool {
        mesh.has_held(self.prefix())
            || self
                .unmoved()
                .is_some_and(|prefix| !mesh.scan(prefix).is_empty())
    }
}

/// The mesh the node belongs to, once it does.
#[derive(Clone)]
pub(crate) struct Membership {
    mesh: Arc<watch::Sender<Option<Mesh>>>,
    /// Held while credentials are applied, so a server's push cannot land after the mesh's.
    applying: Arc<AsyncMutex<()>>,
}

impl Default for Membership {
    fn default() -> Self {
        Self {
            mesh: Arc::new(watch::Sender::new(None)),
            applying: Arc::default(),
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

    pub(crate) fn get(&self) -> Option<Mesh> {
        self.mesh.borrow().clone()
    }

    /// Waits for the turn to apply credentials: whoever holds it has the node's credentials to
    /// themselves, and checks `owns` under it.
    pub(crate) async fn applying(&self) -> MutexGuard<'_, ()> {
        self.applying.lock().await
    }

    /// Whether the node is in a mesh that owns `family` now.
    pub(crate) fn owns(&self, family: Family) -> bool {
        self.get().is_some_and(|mesh| family.held(&mesh))
    }
}

/// Applies the mesh's state to the node now and on every change, until the
/// mesh stops.
pub(crate) async fn follow(mesh: Mesh, me: EndpointId, offers: Offers) {
    // Subscribed before the first scan, so no change falls between the two.
    let mut changes = mesh.changes();
    let mut updates = mesh.updates();
    let mut applied = Applied::default();
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
        }
    }
}

/// What the node holds from the mesh now, to apply only what changed.
#[derive(Default)]
struct Applied {
    owned: Option<Vec<Family>>,
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
        let owned: Vec<Family> = Family::ALL
            .into_iter()
            .filter(|family| family.held(mesh))
            .collect();
        if self.owned.as_ref() != Some(&owned) {
            log::info!("from the mesh: owns {owned:?}");
            self.owned = Some(owned.clone());
        }
        if let Some(provision) = &offers.provision
            && owned.contains(&Family::Credentials)
        {
            let credentials = credentials(mesh);
            if self.credentials.as_ref() != Some(&credentials) {
                let _applying = offers.mesh.applying().await;
                provision.apply(credentials.clone()).await;
                log::info!(
                    "from the mesh: {} credentials",
                    credentials.env.len() + credentials.forgejo.len()
                );
                self.credentials = Some(credentials);
            }
        }
        let harnesses = owned.contains(&Family::Harnesses);
        let flsh = owned.contains(&Family::Flsh);
        if harnesses || flsh {
            let now = offers.agents.settings();
            let mut settings = settings(mesh);
            if !harnesses {
                settings.harnesses = now.harnesses;
            }
            if !flsh {
                settings.flsh = now.flsh;
            }
            if self.settings.as_ref() != Some(&settings) {
                offers.agents.set_settings(settings.clone());
                offers.agents.settings_arrived();
                log::info!(
                    "from the mesh: settings for {} agents",
                    settings.harnesses.len()
                );
                self.settings = Some(settings);
            }
        }
        if let Some(transfers) = offers.transfers.get()
            && owned.contains(&Family::Sync)
        {
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

fn credentials(mesh: &Mesh) -> MachineCredentials {
    let mut out = MachineCredentials::default();
    for (_, credential) in read::<Credential>(mesh, "credential/") {
        match credential {
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
    let devices = mesh.devices();
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
            Some(Project {
                name: name.clone(),
                folder,
                peers,
                paused: spec.paused,
            })
        })
        .collect();
    (projects, pending)
}

#[cfg(test)]
mod tests {
    use super::*;
    use beep_mesh::transport_config;
    use iroh::{Endpoint, SecretKey, endpoint::presets};
    use tempfile::TempDir;

    async fn lone_mesh() -> (Mesh, TempDir) {
        let dir = TempDir::new().expect("a temp folder");
        let endpoint = Endpoint::builder(presets::Minimal)
            .secret_key(SecretKey::generate())
            .transport_config(transport_config())
            .bind()
            .await
            .expect("the endpoint binds");
        let mesh = Mesh::create(dir.path(), endpoint, "desktop")
            .await
            .expect("the mesh is made");
        (mesh, dir)
    }

    #[tokio::test]
    async fn a_project_naming_a_machine_that_has_not_joined_is_pending() {
        let (mesh, _dir) = lone_mesh().await;
        let (me, other) = (mesh.me(), SecretKey::generate().public());
        let spec = |ids: &[EndpointId]| SyncSpec {
            folders: ids.iter().map(|id| (*id, "/srv/app".to_owned())).collect(),
            paused: false,
        };
        for (name, ids) in [("alone", vec![me]), ("waiting", vec![me, other])] {
            let bytes = spec(&ids).to_bytes().expect("a spec encodes");
            mesh.set(Key::Sync(name.to_owned()), Some(bytes))
                .await
                .expect("written");
        }
        let (projects, pending) = projects(&mesh, me, &mut BTreeMap::new());
        assert_eq!(projects.len(), 2);
        assert_eq!(pending, BTreeSet::from(["waiting".to_owned()]));
        assert!(projects.iter().all(|project| project.peers.is_empty()));
    }

    fn config(agent: &str, program: Option<&str>) -> HarnessConfig {
        HarnessConfig {
            program: program.map(ToOwned::to_owned),
            ..HarnessConfig::new(agent)
        }
    }

    #[test]
    fn a_program_path_reaches_only_the_device_it_is_for() {
        let named = || {
            vec![
                ("codex".to_owned(), config("codex", None)),
                (
                    "codex/mac".to_owned(),
                    config("codex", Some("/Users/a/bin/codex")),
                ),
                (
                    "codex/linux".to_owned(),
                    config("codex", Some("/usr/local/bin/codex")),
                ),
            ]
        };
        let on_linux = harnesses_for("linux", named());
        assert_eq!(on_linux.len(), 1);
        assert_eq!(on_linux[0].program.as_deref(), Some("/usr/local/bin/codex"));
        let elsewhere = harnesses_for("windows", named());
        assert_eq!(elsewhere.len(), 1);
        assert_eq!(elsewhere[0].program, None);
        // A path left in the shared value is nobody's.
        let stray = vec![(
            "codex".to_owned(),
            config("codex", Some("/Users/a/bin/codex")),
        )];
        assert_eq!(harnesses_for("linux", stray)[0].program, None);
    }
}
