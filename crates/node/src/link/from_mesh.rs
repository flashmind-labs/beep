//! A node in a mesh takes its synced projects, credentials and agent
//! settings from the mesh's replicated state (`sync/*`, `credential/*`,
//! `harness/*`, `flsh`), and ignores what a server pushes for them. The
//! state file the mesh keeps is the only copy on disk, so the credentials
//! come back after a restart with no second file to guard.
//!
//! `follow` runs while the node is in the mesh. Each change to the state or
//! the device list applies what differs from what was applied before.

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
use std::sync::Arc;
use tokio::sync::{broadcast::error::RecvError, watch};

/// The mesh the node belongs to, once it does.
#[derive(Clone)]
pub(crate) struct Membership(Arc<watch::Sender<Option<Mesh>>>);

impl Default for Membership {
    fn default() -> Self {
        Self(Arc::new(watch::Sender::new(None)))
    }
}

impl Membership {
    pub(crate) fn set(&self, mesh: Mesh) {
        self.0.send_replace(Some(mesh));
    }

    pub(crate) fn get(&self) -> Option<Mesh> {
        self.0.borrow().clone()
    }

    pub(crate) fn joined(&self) -> bool {
        self.0.borrow().is_some()
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
    credentials: Option<MachineCredentials>,
    settings: Option<AgentSettings>,
    projects: Option<Vec<Project>>,
}

impl Applied {
    async fn apply(&mut self, mesh: &Mesh, me: EndpointId, offers: &Offers) {
        if let Some(provision) = &offers.provision {
            let credentials = credentials(mesh);
            if self.credentials.as_ref() != Some(&credentials) {
                log::info!(
                    "from the mesh: {} credentials",
                    credentials.env.len() + credentials.forgejo.len()
                );
                provision.apply(credentials.clone()).await;
                self.credentials = Some(credentials);
            }
        }
        let settings = settings(mesh);
        if self.settings.as_ref() != Some(&settings) {
            log::info!(
                "from the mesh: settings for {} agents",
                settings.harnesses.len()
            );
            offers.agents.set_settings(settings.clone());
            offers.agents.settings_arrived();
            self.settings = Some(settings);
        }
        if let Some(transfers) = &offers.transfers {
            let projects = projects(mesh, me);
            if self.projects.as_ref() != Some(&projects) {
                for project in &projects {
                    log::info!(
                        "from the mesh: sync {} with {} peers",
                        project.name,
                        project.peers.len()
                    );
                }
                for refused in transfers
                    .syncs()
                    .configure_from_mesh(projects.clone())
                    .await
                {
                    log::warn!("not syncing {}: {}", refused.project, refused.message);
                }
                self.projects = Some(projects);
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
    AgentSettings {
        harnesses: read::<HarnessConfig>(mesh, "harness/")
            .into_iter()
            .map(|(_, config)| config)
            .collect(),
        flsh,
    }
}

/// The projects this device syncs: those whose spec names it, with the
/// other devices of the spec that are in the mesh now.
fn projects(mesh: &Mesh, me: EndpointId) -> Vec<Project> {
    let devices = mesh.devices();
    read::<SyncSpec>(mesh, "sync/")
        .into_iter()
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
                name,
                folder,
                peers,
                paused: spec.paused,
            })
        })
        .collect()
}
