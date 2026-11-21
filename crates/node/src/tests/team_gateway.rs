use super::*;
use crate::{
    machine::ci::projects::pinned,
    team::{
        board::tests::{URL, member, repo},
        home::{Project, in_team, picks_up, read},
        link::Teams,
    },
};
use beep_mesh::Key;
use beep_proto::node::{
    mesh_state::{Place, ProjectRecord, StateValue},
    tools::HostReach,
};
use beep_proto::team::{TeamMsg, TeamRecord};
use iroh::{Endpoint, EndpointAddr, SecretKey, endpoint::presets};

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
    mesh.set(
        Key::Project("org/repo".into()),
        Some(record.to_bytes().unwrap()),
    )
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
    // This mesh's entry names every machine with the project: both agree.
    let mine = member("alice", me);
    assert_eq!(watched.elected(me, &ci, &peers), Some(me));
    assert_eq!(elected(project, &mine, &repo(), me, &ci, &peers), Some(me));

    // A machine with the project that is not on the entry never acts for
    // the team, though it watches CI.
    let elsewhere = member("alice", SecretKey::generate().public());
    assert_eq!(elected(project, &elsewhere, &repo(), me, &ci, &peers), None);
}

#[test]
fn a_machine_not_on_the_entry_is_never_elected() {
    let mut ids = [
        SecretKey::generate().public(),
        SecretKey::generate().public(),
    ];
    ids.sort();
    let [first, second] = ids;
    let project = Project {
        name: "org/repo".to_owned(),
        checkout: None,
        devices: vec![EndpointAddr::new(first), EndpointAddr::new(second)],
    };
    let peers = [(second, ForgeReach::default())].into();
    let ci = Ci::default();

    // By id the first would win; only the second is on the entry.
    assert_eq!(
        elected(
            &project,
            &member("alice", first),
            &repo(),
            first,
            &ci,
            &peers
        ),
        Some(first)
    );
    assert_eq!(
        elected(
            &project,
            &member("alice", second),
            &repo(),
            first,
            &ci,
            &peers
        ),
        Some(second)
    );
}

#[test]
fn a_pending_announcement_moves_no_election() {
    let mut ids = [
        SecretKey::generate().public(),
        SecretKey::generate().public(),
    ];
    ids.sort();
    // The added machine would win by id once the written entry names it.
    let [added, old] = ids;
    let alice = member("alice", old);
    let bob = member("bob", SecretKey::generate().public());
    let written = TeamRecord::new(repo(), vec![alice.clone(), bob]);
    let machine = |me: EndpointId| {
        let mut teams = Teams::default();
        teams.rosters.refresh([(repo(), written.clone())].into());
        teams.home.mesh = Some(alice.mesh);
        let project = Project {
            name: "org/repo".to_owned(),
            checkout: None,
            devices: vec![EndpointAddr::new(added), EndpointAddr::new(old)],
        };
        teams.home.projects.insert(repo(), project);
        (me, teams)
    };
    let (mut on_old, on_added) = (machine(old), machine(added));
    // The old gateway announced the added machine; no desktop wrote it yet.
    let mut entry = alice.clone();
    entry.devices.push(EndpointAddr::new(added));
    let roster = TeamMsg::Roster {
        repo: repo(),
        members: vec![entry],
    };
    let applied = on_old
        .1
        .rosters
        .apply(old, Some(alice.mesh), &roster)
        .unwrap();
    assert!(applied.changed.is_some());

    let ci = Ci::default();
    let chosen: Vec<Option<EndpointId>> = [&on_old, &on_added]
        .into_iter()
        .map(|(me, teams)| {
            let other = if *me == old { added } else { old };
            let peers = [(other, ForgeReach::default())].into();
            let team = &membership::teams(teams)[0];
            elected(
                &teams.home.projects[&repo()],
                &team.mine,
                &repo(),
                *me,
                &ci,
                &peers,
            )
        })
        .collect();

    // Both pick the old gateway: exactly one machine acts.
    assert_eq!(chosen, [Some(old), Some(old)]);
}

#[tokio::test]
async fn a_folder_is_in_a_team_once_its_project_has_one() {
    let dir = tempfile::tempdir().unwrap();
    let endpoint = Endpoint::builder(presets::Minimal)
        .secret_key(SecretKey::generate())
        .bind()
        .await
        .unwrap();
    let mesh = Mesh::create(&dir.path().join("mesh"), endpoint, "build-box")
        .await
        .unwrap();
    let project = ProjectRecord {
        url: URL.to_owned(),
        ..ProjectRecord::default()
    };
    mesh.set(
        Key::Project("org/repo".into()),
        Some(project.to_bytes().unwrap()),
    )
    .await
    .unwrap();
    let place = Place {
        folder: "/srv/repo".into(),
        imported_ms: 0,
        fork: None,
    };
    mesh.set(
        Key::Project(format!("org/repo/{}", mesh.me())),
        Some(place.to_bytes().unwrap()),
    )
    .await
    .unwrap();
    let worktree = std::path::Path::new("/srv/repo/.worktrees/52-fees");

    assert_eq!(read(&mesh).team_of(worktree), Some(repo()));
    assert!(!in_team(&mesh, worktree));

    // A team a teammate removed this mesh from is as good as left.
    let mut team = TeamRecord::new(repo(), vec![member("bob", SecretKey::generate().public())]);
    let write = |team: &TeamRecord| Some(rmp_serde::to_vec(team).unwrap());
    mesh.set(Key::Team(repo()), write(&team)).await.unwrap();
    assert!(!in_team(&mesh, worktree));

    let mut mine = member("alice", mesh.me());
    mine.mesh = mesh.id();
    team.members.push(mine);
    mesh.set(Key::Team(repo()), write(&team)).await.unwrap();

    assert!(in_team(&mesh, worktree));
    assert!(!in_team(&mesh, std::path::Path::new("/srv/other")));
}

#[tokio::test]
async fn only_a_team_this_mesh_is_on_with_pickup_on_picks_up() {
    let dir = tempfile::tempdir().unwrap();
    let endpoint = Endpoint::builder(presets::Minimal)
        .secret_key(SecretKey::generate())
        .bind()
        .await
        .unwrap();
    let mesh = Mesh::create(&dir.path().join("mesh"), endpoint, "build-box")
        .await
        .unwrap();
    let write = |team: &TeamRecord| Some(rmp_serde::to_vec(team).unwrap());
    assert!(!picks_up(&mesh, &repo()));

    let mut team = TeamRecord::new(repo(), vec![member("bob", SecretKey::generate().public())]);
    mesh.set(Key::Team(repo()), write(&team)).await.unwrap();
    assert!(!picks_up(&mesh, &repo()));

    let mut mine = member("alice", mesh.me());
    mine.mesh = mesh.id();
    team.members.push(mine);
    mesh.set(Key::Team(repo()), write(&team)).await.unwrap();
    assert!(picks_up(&mesh, &repo()));

    team.pickup = false;
    mesh.set(Key::Team(repo()), write(&team)).await.unwrap();
    assert!(!picks_up(&mesh, &repo()));
}

/// A desktop-only mesh has no other place to start an issue's session.
#[test]
fn issue_sessions_start_on_any_online_machine_with_the_checkout() {
    let (desktop, node, offline) = (
        SecretKey::generate().public(),
        SecretKey::generate().public(),
        SecretKey::generate().public(),
    );
    let project = Project {
        name: "org/repo".to_owned(),
        checkout: None,
        devices: [desktop, node, offline].map(EndpointAddr::new).to_vec(),
    };
    let elsewhere = SecretKey::generate().public();
    let online = [desktop, node, elsewhere].into();

    assert_eq!(starters(&project, &online), [desktop, node].into());
}
