//! Which projects sync and with whom when both the mesh and the server say
//! something: the mesh names the projects and its devices, and while a
//! project still names a machine that has not joined the mesh, the peers the
//! server set for it stay, so joining machines one at a time never cuts the
//! sync between the ones left.

use beep_proto::node::sync::Project;
use iroh::{EndpointAddr, EndpointId};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default)]
pub(super) struct Lists {
    /// The projects the mesh names for this node, once it has been read.
    mesh: Option<Vec<Project>>,
    /// Those of them with a machine that has not joined the mesh yet.
    pending: BTreeSet<String>,
    /// The peers the server set for each project.
    server: BTreeMap<String, Vec<EndpointAddr>>,
}

impl Lists {
    /// Takes the mesh's projects, and which of them wait for a machine.
    pub(super) fn set_mesh(&mut self, projects: Vec<Project>, pending: BTreeSet<String>) {
        self.mesh = Some(projects);
        self.pending = pending;
    }

    pub(super) fn clear_mesh(&mut self) {
        self.mesh = None;
        self.pending.clear();
    }

    /// Takes the peers of each project of the server's list.
    pub(super) fn set_server(&mut self, projects: &[Project]) {
        self.server = projects
            .iter()
            .map(|project| (project.name.clone(), project.peers.clone()))
            .collect();
    }

    /// Whether the mesh's projects have been read.
    pub(super) fn has_mesh(&self) -> bool {
        self.mesh.is_some()
    }

    /// The names in `server` the mesh does not name: they do not sync here.
    pub(super) fn not_in_mesh<'a>(&self, server: &'a [Project]) -> Vec<&'a str> {
        let mesh = self.mesh.as_deref().unwrap_or_default();
        server
            .iter()
            .map(|project| project.name.as_str())
            .filter(|name| !mesh.iter().any(|project| project.name == *name))
            .collect()
    }

    /// What to sync: the mesh's projects, each still waiting for a machine
    /// with the peers the server set added, but for those `removed` says left
    /// the mesh.
    pub(super) fn effective(
        &self,
        me: EndpointId,
        removed: impl Fn(&EndpointId) -> bool,
    ) -> Vec<Project> {
        let mut projects = self.mesh.clone().unwrap_or_default();
        for project in &mut projects {
            if !self.pending.contains(&project.name) {
                continue;
            }
            for peer in self.server.get(&project.name).into_iter().flatten() {
                if peer.id != me
                    && !removed(&peer.id)
                    && !project.peers.iter().any(|p| p.id == peer.id)
                {
                    project.peers.push(peer.clone());
                }
            }
        }
        projects
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iroh::SecretKey;

    fn id() -> EndpointId {
        SecretKey::generate().public()
    }

    fn project(name: &str, peers: &[EndpointId]) -> Project {
        Project {
            name: name.to_owned(),
            folder: format!("/srv/{name}"),
            peers: peers.iter().map(|id| EndpointAddr::new(*id)).collect(),
            paused: false,
        }
    }

    fn peers(project: &Project) -> Vec<EndpointId> {
        project.peers.iter().map(|peer| peer.id).collect()
    }

    #[test]
    fn a_project_waiting_for_a_machine_keeps_the_servers_peers() {
        let (me, a, b) = (id(), id(), id());
        let mut lists = Lists::default();
        // A joined and B has not: the mesh knows A, the server knows both.
        lists.set_mesh(
            vec![project("app", &[a]), project("done", &[a])],
            ["app".to_owned()].into(),
        );
        lists.set_server(&[project("app", &[a, b]), project("done", &[a, b])]);
        let effective = lists.effective(me, |_| false);
        assert_eq!(peers(&effective[0]), [a, b]);
        // Every machine of `done` joined: the mesh alone.
        assert_eq!(peers(&effective[1]), [a]);
        // The server's own project the mesh does not name stays out.
        assert_eq!(
            lists.not_in_mesh(&[project("app", &[]), project("phone-only", &[])]),
            ["phone-only"]
        );
    }

    #[test]
    fn a_removed_machine_is_not_kept_from_the_servers_peers() {
        let (me, a, b) = (id(), id(), id());
        let mut lists = Lists::default();
        lists.set_mesh(vec![project("app", &[a])], ["app".to_owned()].into());
        lists.set_server(&[project("app", &[a, b, me])]);
        let effective = lists.effective(me, |peer| *peer == b);
        assert_eq!(peers(&effective[0]), [a]);
    }

    #[test]
    fn nothing_syncs_before_the_mesh_is_read() {
        let mut lists = Lists::default();
        lists.set_server(&[project("app", &[id()])]);
        assert!(!lists.has_mesh());
        assert!(lists.effective(id(), |_| false).is_empty());
    }
}
