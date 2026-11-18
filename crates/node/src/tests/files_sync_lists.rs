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
        rules: String::new(),
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
