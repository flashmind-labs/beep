//! The mesh the node has joined, once it has, and what it holds for the node
//! to use: credentials and the servers they name.

use beep_mesh::{Key, Mesh};
use beep_proto::node::{
    MachineCredentials,
    mesh_state::{Credential, Place, ProjectRecord, StateValue},
};
use beep_proto::{
    lenient,
    team::{TeamRecord, TeamRepo},
    watched,
};
use iroh::EndpointId;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::sync::watch;

/// The mesh the node belongs to, once it does.
#[derive(Clone)]
pub struct Membership {
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
        watched::read_sent(&self.mesh, Clone::clone)
    }
}

/// The values under `prefix` that read as `T`; one that does not is left
/// out and its key logged, never its content.
pub(crate) fn read<T: StateValue>(mesh: &Mesh, prefix: &str) -> Vec<(Key, T)> {
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

/// The credentials the node uses.
pub(crate) fn credentials(mesh: &Mesh) -> MachineCredentials {
    let mut out = MachineCredentials::default();

    for (_, credential) in read::<Credential>(mesh, "credential/") {
        match credential {
            Credential::Env(secret) => out.env.push(secret),
            Credential::Forgejo(login) => out.forgejo.push(login),
            Credential::Agent(login) => out.agents.push(login),
        }
    }

    out
}

/// Whether `record` names this mesh: one a teammate removed it from does
/// not, and is as good as left until a desktop deletes it.
pub(crate) fn on_team(record: &TeamRecord, mesh: &Mesh) -> bool {
    record.members.iter().any(|member| member.mesh == mesh.id())
}

/// `repo`'s team as the mesh holds it, when this mesh is on it.
pub(crate) fn team_record(mesh: &Mesh, repo: &TeamRepo) -> Option<TeamRecord> {
    let bytes = mesh.get(&Key::Team(repo.clone()))?;
    let record = lenient::from_slice::<TeamRecord>(&bytes).ok()?;

    on_team(&record, mesh).then_some(record)
}

/// The repository of the team whose project has the checkout `folder` is
/// in here: the project whose place on this machine holds `folder`, the
/// deepest one when several do, and only while this mesh is on its team.
pub(crate) fn team_repo_of(mesh: &Mesh, folder: &Path) -> Option<TeamRepo> {
    let me = mesh.me();
    let mut records = BTreeMap::new();
    let mut here: BTreeMap<String, PathBuf> = BTreeMap::new();

    for (key, bytes) in mesh.scan("project/") {
        let Key::Project(name) = key else { continue };

        if let Some((project, machine)) = name.rsplit_once('/')
            && let Ok(machine) = machine.parse::<EndpointId>()
            && let Ok(place) = Place::from_bytes(&bytes)
        {
            if machine == me {
                here.insert(project.to_owned(), PathBuf::from(&place.folder));
            }
        } else if let Ok(record) = ProjectRecord::from_bytes(&bytes) {
            records.insert(name, record);
        }
    }

    // As the team link's home reads them: of two projects on one repository the last
    // by name wins, whether or not it has a checkout here.
    let projects: BTreeMap<TeamRepo, Option<PathBuf>> = records
        .into_iter()
        .filter_map(|(name, record)| {
            let repo = record.url.parse::<TeamRepo>().ok()?;
            Some((repo, here.remove(&name)))
        })
        .collect();
    deepest(
        folder,
        projects
            .iter()
            .map(|(repo, checkout)| (repo, checkout.as_deref())),
    )
    .filter(|repo| team_record(mesh, repo).is_some())
}

/// The repository whose checkout holds `folder`, the deepest checkout when several do.
/// The team link's home and `team_repo_of` both pick by this rule.
pub(crate) fn deepest<'a>(
    folder: &Path,
    projects: impl Iterator<Item = (&'a TeamRepo, Option<&'a Path>)>,
) -> Option<TeamRepo> {
    projects
        .filter_map(|(repo, checkout)| Some((repo, checkout?)))
        .filter(|(_, checkout)| folder.starts_with(checkout))
        .map(|(repo, checkout)| (checkout.as_os_str().len(), repo))
        .max()
        .map(|(_, repo)| repo.clone())
}
