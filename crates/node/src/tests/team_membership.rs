use super::*;
use crate::team::board::tests::{Pair, member, repo};
use iroh::{SecretKey, protocol::Router};
use tokio::time::Duration;

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

fn projects(devices: Vec<EndpointAddr>) -> BTreeMap<TeamRepo, Project> {
    let project = Project {
        name: "org/repo".to_owned(),
        checkout: None,
        devices,
    };

    [(repo(), project)].into()
}

/// Bob's link takes the team's record as the gateway's mesh wrote it.
fn bob_knows_the_team(pair: &Pair) {
    let record = pair.gateway.with(|teams| teams.rosters.written(&repo()).unwrap().clone());
    pair.bob_link.with(|teams| {
        teams.rosters.refresh([(repo(), record)].into());
        teams.home.mesh = Some(pair.bob.mesh);
    });
}

/// Waits until the last send on `repo()` ended, and takes what it says.
async fn sent(membership: &mut Membership, pair: &Pair, team: &Team, projects: &BTreeMap<TeamRepo, Project>) {
    for _ in 0..500 {
        let announced = &membership.announced[&repo()];

        if announced.sending.as_ref().is_none_or(|sending| sending.is_finished()) {
            membership.announce(&pair.gateway, &BTreeSet::new(), |_| true, std::slice::from_ref(team), projects);
            return;
        }

        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    panic!("the send did not end");
}

#[tokio::test]
async fn a_device_added_after_the_join_is_announced_and_taken() {
    let pair = Pair::new().await;
    let gateway = pair.gateway.endpoint().id();
    let added = SecretKey::generate().public();
    let mut devices = pair.alice.devices.clone();
    devices.push(EndpointAddr::new(added));
    let projects = projects(devices);
    let team = Team {
        repo: repo(),
        project: "org/repo".to_owned(),
        mine: pair.alice.clone(),
        others: vec![pair.bob.clone()],
    };
    bob_knows_the_team(&pair);

    let mut membership = Membership::new(gateway);
    membership.announce(&pair.gateway, &[added].into(), |_| true, std::slice::from_ref(&team), &projects);

    // Here it waits for a desktop to write it.
    timeout(Duration::from_secs(5), pair.gateway.waiting())
        .await
        .expect("the change waits for a desktop");
    assert!(pair.gateway.with(|teams| teams.rosters.allows(added, &repo())));

    // A teammate takes it from the gateway, a device already on the entry.
    let entry = membership.announced[&repo()].entry.clone();
    let roster = TeamMsg::Roster {
        repo: repo(),
        members: vec![entry],
    };
    assert_eq!(pair.bob_link.answer(gateway, &roster).await, None);
    assert!(pair.bob_link.with(|teams| teams.rosters.allows(added, &repo())));
}

#[tokio::test]
async fn a_teammate_offline_at_the_announcement_gets_it_later() {
    let pair = Pair::new().await;
    let gateway = pair.gateway.endpoint().id();
    let added = SecretKey::generate().public();
    let mut devices = pair.alice.devices.clone();
    devices.push(EndpointAddr::new(added));
    let projects = projects(devices);
    // Bob's device takes no team links yet.
    let mut team = Team {
        repo: repo(),
        project: "org/repo".to_owned(),
        mine: pair.alice.clone(),
        others: vec![pair.bob.clone()],
    };
    bob_knows_the_team(&pair);
    let mut membership = Membership::new(gateway);
    membership.announce(&pair.gateway, &BTreeSet::new(), |_| true, std::slice::from_ref(&team), &projects);
    sent(&mut membership, &pair, &team, &projects).await;
    assert_eq!(membership.announced[&repo()].unsent, [pair.bob.mesh].into());

    // Bob comes online; meanwhile the mesh wrote another change, which
    // dropped the one pending here.
    let bob_endpoint = pair.bob_link.endpoint().clone();
    let _router = Router::builder(bob_endpoint.clone())
        .accept(beep_proto::team::ALPN, pair.bob_link.handler())
        .spawn();
    team.others[0].devices = vec![bob_endpoint.addr()];
    pair.gateway.with(|teams| {
        let mut record = teams.rosters.written(&repo()).unwrap().clone();
        record.asks = false;
        teams.rosters.refresh([(repo(), record)].into());
    });
    assert!(!pair.gateway.with(|teams| teams.rosters.allows(added, &repo())));
    let announced = membership.announced.get_mut(&repo()).unwrap();
    announced.at = Instant::now().checked_sub(RESEND).unwrap();

    membership.announce(&pair.gateway, &BTreeSet::new(), |_| true, std::slice::from_ref(&team), &projects);

    assert!(pair.gateway.with(|teams| teams.rosters.allows(added, &repo())));
    sent(&mut membership, &pair, &team, &projects).await;
    assert!(membership.announced[&repo()].unsent.is_empty());
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
