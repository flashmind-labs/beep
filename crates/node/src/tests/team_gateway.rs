use super::*;
use crate::{
    machine::ci::projects::pinned,
    team::{
        board::tests::{Pair, URL, member, repo},
        home::read,
    },
};
use beep_mesh::Key;
use beep_proto::node::{
    mesh_state::{Place, ProjectRecord, StateValue},
    tools::HostReach,
};
use iroh::{Endpoint, SecretKey, endpoint::presets};

#[tokio::test]
async fn gateway_and_ci_watcher_elect_the_same_machine() {
    let dir = tempfile::tempdir().unwrap();
    let endpoint = Endpoint::builder(presets::Minimal)
        .secret_key(SecretKey::generate())
        .bind()
        .await
        .unwrap();
    let mesh = Mesh::create(&dir.path().join("mesh"), endpoint, "build-box")
        .await
        .unwrap();
    let me = mesh.me();
    // A machine that is not in the mesh, and that would win if it counted.
    let stranger = SecretKey::generate().public();
    let record = ProjectRecord {
        url: URL.to_owned(),
        ..ProjectRecord::default()
    };
    mesh.set(Key::Project("org/repo".into()), Some(record.to_bytes().unwrap()))
        .await
        .unwrap();
    for machine in [me, stranger] {
        let place = Place {
            folder: "/srv/repo".into(),
            imported_ms: 0,
            fork: None,
        };
        mesh.set(
            Key::Project(format!("org/repo/{machine}")),
            Some(place.to_bytes().unwrap()),
        )
        .await
        .unwrap();
    }
    mesh.set(Key::Pin("org/repo".into()), Some(Vec::new()))
        .await
        .unwrap();
    let ci = Ci::default();
    let mut said = ForgeReach::default();
    let reached = HostReach {
        reached: true,
        failures: 0,
    };
    said.hosts.insert(repo().host, reached);
    let peers = [(stranger, said)].into();

    let watched = pinned(&mesh).into_iter().next().unwrap();
    let home = read(&mesh);
    let project = &home.projects[&repo()];
    let machines: BTreeSet<EndpointId> = project.devices.iter().map(|device| device.id).collect();

    assert_eq!(machines, watched.machines);
    assert_eq!(watched.target.host_name(), repo().host);
    assert_eq!(project.name, watched.name);
    assert_eq!(watched.elected(me, &ci, &peers), Some(me));
    assert_eq!(elected(project, &repo(), me, &ci, &peers), Some(me));
}

#[tokio::test]
async fn a_member_who_lost_access_is_removed() {
    let pair = Pair::new().await;
    let carol = member("carol", SecretKey::generate().public());
    pair.gateway.with(|teams| {
        let mut team = teams.rosters.get(&repo()).unwrap().clone();
        team.members.push(carol.clone());
        teams.rosters.refresh([(repo(), team)].into());
    });
    let team = Team {
        repo: repo(),
        project: "org/repo".to_owned(),
        login: pair.alice.login.clone(),
        others: vec![pair.bob.clone(), carol.clone()],
    };

    check_members(&pair.gateway, &team).await;

    timeout(Duration::from_secs(5), pair.gateway.waiting())
        .await
        .expect("the change waits for a desktop");
    let pending = pair.gateway.with(|teams| teams.rosters.pending());
    let logins: Vec<&ForgeLogin> = pending[0].members.iter().map(|member| &member.login).collect();
    assert_eq!(logins, [&pair.alice.login, &pair.bob.login]);
    assert_eq!(pending[0].removed[0].login, carol.login);
}
