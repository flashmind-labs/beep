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
//! included. Until then the node keeps what the server set and keeps taking
//! the server's pushes for it, so a running node that joins a mesh with
//! nothing in it loses nothing.

use crate::link::Offers;
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
    sync::{Arc, Mutex, MutexGuard, PoisonError},
};
use tokio::sync::{broadcast::error::RecvError, watch};

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

    /// Who the node takes this family from. Until the mesh has held a key of it, the server. Once
    /// it has, the mesh, unless the node has a server and a record says a value is still only the
    /// server's: then the mesh's keys are laid over what the server set, until every one is
    /// entered again. A node with no server has nothing else to keep, so the mesh's apply at once.
    pub(crate) fn holds(self, mesh: &Mesh, has_server: bool) -> Holds {
        if !mesh.has_held(self.prefix()) {
            return Holds::Server;
        }
        let waiting = self
            .unmoved()
            .is_some_and(|prefix| !mesh.scan(prefix).is_empty());
        if has_server && waiting {
            Holds::Layered
        } else {
            Holds::Mesh
        }
    }
}

/// Where a family comes from on the node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Holds {
    /// The server's, as it always was.
    Server,
    /// The server's with the mesh's keys over it.
    Layered,
    /// The mesh's alone.
    Mesh,
}

/// What the server last sent for the families that can be layered, kept so the mesh's keys go over
/// it again when they change, and so a dropped link leaves it as it is.
#[derive(Default)]
pub(crate) struct ServerLayer {
    pub(crate) credentials: MachineCredentials,
    pub(crate) harnesses: Vec<HarnessConfig>,
}

/// The mesh the node belongs to, once it does.
#[derive(Clone)]
pub(crate) struct Membership {
    mesh: Arc<watch::Sender<Option<Mesh>>>,
    server: Arc<Mutex<ServerLayer>>,
}

impl Default for Membership {
    fn default() -> Self {
        Self {
            mesh: Arc::new(watch::Sender::new(None)),
            server: Arc::default(),
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

    /// Where `family` comes from now.
    pub(crate) fn holds(&self, family: Family, has_server: bool) -> Holds {
        self.get()
            .map_or(Holds::Server, |mesh| family.holds(&mesh, has_server))
    }

    /// Whether the node is in a mesh that owns `family` alone now.
    pub(crate) fn owns(&self, family: Family, has_server: bool) -> bool {
        self.holds(family, has_server) == Holds::Mesh
    }

    /// What the server last sent.
    pub(crate) fn server(&self) -> MutexGuard<'_, ServerLayer> {
        self.server.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The server's credentials with the mesh's laid over them, as the node holds them while some
    /// are still only the server's.
    pub(crate) fn layered_credentials(&self, server: &MachineCredentials) -> MachineCredentials {
        match self.get() {
            Some(mesh) => lay_credentials(server, credentials(&mesh)),
            None => server.clone(),
        }
    }

    /// The same for the harness settings.
    pub(crate) fn layered_harnesses(&self, server: &[HarnessConfig]) -> Vec<HarnessConfig> {
        match self.get() {
            Some(mesh) => lay_harnesses(server, settings(&mesh).harnesses),
            None => server.to_vec(),
        }
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
        let has_server = offers.has_server();
        let holds = |family: Family| family.holds(mesh, has_server);
        let owned: Vec<Family> = Family::ALL
            .into_iter()
            .filter(|family| holds(*family) != Holds::Server)
            .collect();
        if self.owned.as_ref() != Some(&owned) {
            log::info!("from the mesh: owns {owned:?}");
            self.owned = Some(owned.clone());
        }
        if let Some(provision) = &offers.provision
            && owned.contains(&Family::Credentials)
        {
            let credentials = if holds(Family::Credentials) == Holds::Layered {
                lay_credentials(&offers.mesh.server().credentials, credentials(mesh))
            } else {
                credentials(mesh)
            };
            if self.credentials.as_ref() != Some(&credentials) {
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
            if holds(Family::Harnesses) == Holds::Layered {
                settings.harnesses =
                    lay_harnesses(&offers.mesh.server().harnesses, settings.harnesses);
            }
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
        if let Some(transfers) = &offers.transfers
            && owned.contains(&Family::Sync)
        {
            let (projects, pending) = projects(mesh, me, &mut self.specs);
            if self.projects.as_ref() != Some(&projects) || self.pending != pending {
                for refused in transfers
                    .syncs()
                    .configure_from_mesh(projects.clone(), pending.clone())
                    .await
                {
                    log::warn!("not syncing {}: {}", refused.project, refused.message);
                }
                for project in &projects {
                    log::info!(
                        "from the mesh: sync {} with {} peers",
                        project.name,
                        project.peers.len()
                    );
                }
                self.projects = Some(projects);
                self.pending = pending;
            }
        }
    }
}

/// `server`'s credentials with `mesh`'s over them: a value the mesh has replaces the server's of
/// the same env var or Forgejo server.
fn lay_credentials(server: &MachineCredentials, mesh: MachineCredentials) -> MachineCredentials {
    let mut out = MachineCredentials {
        env: server
            .env
            .iter()
            .filter(|old| !mesh.env.iter().any(|new| new.name == old.name))
            .cloned()
            .collect(),
        forgejo: server
            .forgejo
            .iter()
            .filter(|old| !mesh.forgejo.iter().any(|new| new.url == old.url))
            .cloned()
            .collect(),
    };
    out.env.extend(mesh.env);
    out.forgejo.extend(mesh.forgejo);
    out
}

/// `server`'s harness settings with `mesh`'s over them: an agent the mesh has takes the mesh's
/// switch, arguments and paths, and its env vars over the server's of other names.
fn lay_harnesses(server: &[HarnessConfig], mesh: Vec<HarnessConfig>) -> Vec<HarnessConfig> {
    let mut out: Vec<HarnessConfig> = server
        .iter()
        .filter(|old| !mesh.iter().any(|new| new.agent == old.agent))
        .cloned()
        .collect();
    for mut config in mesh {
        if let Some(old) = server.iter().find(|old| old.agent == config.agent) {
            config.program = config.program.or_else(|| old.program.clone());
            config.home = config.home.or_else(|| old.home.clone());
            let own = config.env.clone();
            config.env.extend(
                old.env
                    .iter()
                    .filter(|var| !own.iter().any(|mine| mine.name == var.name))
                    .cloned(),
            );
        }
        out.push(config);
    }
    out
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

    async fn put(mesh: &Mesh, key: Key) {
        mesh.set(key, Some(vec![1])).await.expect("written");
    }

    async fn drop_key(mesh: &Mesh, key: Key) {
        mesh.set(key, None).await.expect("deleted");
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

    #[test]
    fn credentials_entered_again_go_over_the_servers() {
        use beep_proto::{SecretText, node::EnvSecret};
        let var = |name: &str, value: &str| EnvSecret {
            name: name.to_owned(),
            value: SecretText::new(value),
        };
        let server = MachineCredentials {
            env: vec![var("A", "server-a"), var("B", "server-b")],
            forgejo: Vec::new(),
        };
        let mesh = MachineCredentials {
            env: vec![var("B", "mesh-b")],
            forgejo: Vec::new(),
        };
        let got = lay_credentials(&server, mesh);
        let pairs: Vec<(&str, &str)> = got
            .env
            .iter()
            .map(|v| (v.name.as_str(), v.value.expose()))
            .collect();
        assert_eq!(pairs, [("A", "server-a"), ("B", "mesh-b")]);
    }

    #[tokio::test]
    async fn a_partly_reentered_credential_list_keeps_the_servers_until_all_are_back() {
        let (mesh, _dir) = lone_mesh().await;
        put(&mesh, Key::Credential("api.1.anthropic".into())).await;
        put(&mesh, Key::Unmoved("credential/a".into())).await;
        put(&mesh, Key::Unmoved("credential/b".into())).await;
        // The mesh holds a credential, but two are still only the server's.
        assert!(!(Family::Credentials.holds(&mesh, true) == Holds::Mesh));
        drop_key(&mesh, Key::Unmoved("credential/a".into())).await;
        assert!(!(Family::Credentials.holds(&mesh, true) == Holds::Mesh));
        drop_key(&mesh, Key::Unmoved("credential/b".into())).await;
        assert!((Family::Credentials.holds(&mesh, true) == Holds::Mesh));
        // Harness variables count for the harness family only.
        put(&mesh, Key::Harness("codex".into())).await;
        put(&mesh, Key::Unmoved("harness/codex/OPENAI_API_KEY".into())).await;
        assert!((Family::Credentials.holds(&mesh, true) == Holds::Mesh));
        assert!(!(Family::Harnesses.holds(&mesh, true) == Holds::Mesh));
        drop_key(&mesh, Key::Unmoved("harness/codex/OPENAI_API_KEY".into())).await;
        assert!((Family::Harnesses.holds(&mesh, true) == Holds::Mesh));
    }

    #[tokio::test]
    async fn the_mesh_takes_over_once_nothing_is_unmoved() {
        let (mesh, _dir) = lone_mesh().await;
        put(&mesh, Key::Credential("api.1.anthropic".into())).await;
        put(&mesh, Key::Unmoved("credential/a".into())).await;
        // A node with no server has no values to keep: the mesh's apply at once.
        assert!((Family::Credentials.holds(&mesh, false) == Holds::Mesh));
        assert!(!(Family::Credentials.holds(&mesh, true) == Holds::Mesh));
        drop_key(&mesh, Key::Unmoved("credential/a".into())).await;
        assert!((Family::Credentials.holds(&mesh, true) == Holds::Mesh));
        assert!((Family::Credentials.holds(&mesh, false) == Holds::Mesh));
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
