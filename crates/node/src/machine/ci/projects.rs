//! The pinned projects as the mesh's replicated state holds them: each
//! one's host and repository, and the machines with its checkout.

use super::forge::{Host, Target};
use crate::link::from_mesh;
use beep_mesh::{Key, Mesh};
use beep_projects::{
    imports::{
        ImportError,
        repo::{Host as RepoHost, RepoRef},
    },
    model::ForgejoUrl,
};
use beep_proto::node::mesh_state::{ProjectRecord, StateValue};
use iroh::EndpointId;
use std::collections::BTreeSet;

/// A pinned project the watch may take.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Project {
    /// `org/repo`, as the mesh names it.
    pub(super) name: String,
    pub(super) target: Target,
    /// The members of the mesh with a checkout of it.
    pub(super) machines: BTreeSet<EndpointId>,
}

/// The web address of `host`, the server of the repository at `url`: its
/// own scheme and port when `url` is a web one, else https.
fn server_of(url: &str, host: &str) -> Option<ForgejoUrl> {
    let web = ["https://", "http://"]
        .iter()
        .find_map(|scheme| url.strip_prefix(scheme).map(|rest| (*scheme, rest)));
    let base = match web {
        Some((scheme, rest)) => format!("{scheme}{}", rest.split('/').next()?),
        None => format!("https://{host}"),
    };
    base.parse().ok()
}

/// Every pinned project with its repository on GitHub, or on any other
/// host, taken as a Forgejo or Gitea server.
pub(super) fn pinned(mesh: &Mesh) -> Vec<Project> {
    let servers: Vec<ForgejoUrl> = from_mesh::forgejo_servers(mesh)
        .iter()
        .filter_map(|url| url.parse().ok())
        .collect();
    mesh.scan("pin/")
        .into_iter()
        .filter_map(|(key, _)| match key {
            Key::Pin(name) => Some(name),
            _ => None,
        })
        .filter_map(|name| project(mesh, name, &servers))
        .collect()
}

fn project(mesh: &Mesh, name: String, servers: &[ForgejoUrl]) -> Option<Project> {
    let bytes = mesh.get(&Key::Project(name.clone()))?;
    let record = ProjectRecord::from_bytes(&bytes).ok()?;
    let repo = match RepoRef::parse(&record.url, servers) {
        // Any host that is not GitHub is a Forgejo or Gitea server, which
        // teacli may know without a login in the mesh.
        Err(ImportError::UnknownHost(host)) => {
            RepoRef::parse(&record.url, &[server_of(&record.url, &host)?]).ok()?
        }
        parsed => parsed.ok()?,
    };
    let host = match &repo.host {
        RepoHost::GitHub => Host::GitHub,
        RepoHost::Forgejo(url) => Host::Forgejo {
            url: url.as_str().to_owned(),
            name: url.host(),
        },
    };
    let prefix = format!("project/{name}/");
    let machines = mesh
        .scan(&prefix)
        .into_iter()
        .filter_map(|(key, _)| match key {
            Key::Project(place) => place.rsplit_once('/')?.1.parse::<EndpointId>().ok(),
            _ => None,
        })
        .filter(|machine| mesh.is_member(*machine))
        .collect();
    Some(Project {
        name,
        target: Target {
            repo: repo.project(),
            host,
        },
        machines,
    })
}
