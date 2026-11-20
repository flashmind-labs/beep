use super::*;
use crate::{
    machine::ci::projects::pinned,
    team::{
        board::tests::{Pair, URL, member, repo},
        home::{Project, read},
        roster::{MAX_DEVICES, with_devices},
    },
};
use beep_mesh::Key;
use beep_proto::node::{
    mesh_state::{Place, ProjectRecord, StateValue},
    tools::HostReach,
};
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
    let mut ids = [SecretKey::generate().public(), SecretKey::generate().public()];
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
        elected(&project, &member("alice", first), &repo(), first, &ci, &peers),
        Some(first)
    );
    assert_eq!(
        elected(&project, &member("alice", second), &repo(), first, &ci, &peers),
        Some(second)
    );
}

#[test]
fn the_entry_names_the_mesh_machines_with_the_project() {
    let [laptop, gateway, added, offline, gone] =
        std::array::from_fn(|_| SecretKey::generate().public());
    let mut mine = member("alice", laptop);
    mine.devices.push(EndpointAddr::new(gateway));
    mine.devices.push(EndpointAddr::new(gone));
    let project = [laptop, gateway, offline, added].map(EndpointAddr::new);
    let online = [laptop, gateway, added].into();
    let in_mesh = |id: EndpointId| id != gone;

    let entry = with_devices(&mine, &project, &online, in_mesh).unwrap();
    let ids: Vec<EndpointId> = entry.devices.iter().map(|device| device.id).collect();
    assert_eq!(ids, [laptop, gateway, added, offline]);
    assert_eq!((&entry.login, entry.mesh), (&mine.login, mine.mesh));

    // Once it names them, there is nothing to announce.
    assert!(with_devices(&entry, &project, &online, in_mesh).is_none());

    // At most 32 devices.
    let many: Vec<EndpointAddr> = (0..40)
        .map(|_| EndpointAddr::new(SecretKey::generate().public()))
        .collect();
    let full = with_devices(&entry, &many, &online, in_mesh).unwrap();
    assert_eq!(full.devices.len(), MAX_DEVICES);
    assert_eq!(full.devices[..4], entry.devices[..]);
}

#[tokio::test]
async fn a_device_added_after_the_join_is_announced_and_taken() {
    let pair = Pair::new().await;
    let gateway = pair.gateway.endpoint().id();
    let added = SecretKey::generate().public();
    let mut project: Vec<EndpointAddr> = pair.alice.devices.clone();
    project.push(EndpointAddr::new(added));
    let team = Team {
        repo: repo(),
        project: "org/repo".to_owned(),
        mine: pair.alice.clone(),
        others: vec![pair.bob.clone()],
    };
    let record = pair.gateway.with(|teams| teams.rosters.get(&repo()).unwrap().clone());
    pair.bob_link.with(|teams| {
        teams.rosters.refresh([(repo(), record)].into());
        teams.home.mesh = Some(pair.bob.mesh);
    });

    let entry = with_devices(&pair.alice, &project, &[added].into(), |_| true).unwrap();
    announce(&pair.gateway, gateway, &team, entry.clone());

    // Here it waits for a desktop to write it.
    timeout(Duration::from_secs(5), pair.gateway.waiting())
        .await
        .expect("the change waits for a desktop");
    assert!(pair.gateway.with(|teams| teams.rosters.allows(added, &repo())));

    // A teammate takes it from the gateway, a device already on the entry.
    let roster = TeamMsg::Roster {
        repo: repo(),
        members: vec![entry],
    };
    assert_eq!(pair.bob_link.answer(gateway, &roster).await, None);
    assert!(pair.bob_link.with(|teams| teams.rosters.allows(added, &repo())));
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
        mine: pair.alice.clone(),
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
