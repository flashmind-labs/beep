use super::*;
use crate::team::{
    board::tests::{Pair, member, repo},
    gateway::Team,
};
use iroh::SecretKey;

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
