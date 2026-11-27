use super::*;
use crate::team::board::tests::{Pair, device, member, repo};
use iroh::{SecretKey, protocol::Router};
use beep_proto::team::TeamRecord;
use tokio::time::{Duration, timeout};

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
    let record = pair
        .gateway
        .with(|teams| teams.rosters.written(&repo()).unwrap().clone());
    pair.bob_link.with(|teams| teams.rosters.refresh([(repo(), record)].into()));
    pair.bob_link.edit_home(|home| home.mesh = Some(pair.bob.mesh));
}

/// Waits for the last send on `repo()` to end, and keeps whom it missed.
async fn sent(membership: &mut Membership) -> BTreeSet<EndpointId> {
    let announced = membership.announced.get_mut(&repo()).unwrap();
    let sending = announced.sending.take().expect("a send under way");
    announced.unsent = timeout(Duration::from_secs(30), sending)
        .await
        .unwrap()
        .unwrap();

    announced.unsent.clone()
}

/// Bob, found at his endpoint's addresses, which take no team links until
/// `online` serves them.
fn reachable_bob(pair: &Pair) -> Member {
    Member {
        devices: vec![pair.bob_link.endpoint().addr()],
        ..pair.bob.clone()
    }
}

fn online(pair: &Pair) -> Router {
    Router::builder(pair.bob_link.endpoint().clone())
        .accept(
            beep_proto::team::ALPN,
            crate::tools::team_link::TeamAlpn::new(crate::tools::Node::of_link(&pair.bob_link)),
        )
        .spawn()
}

/// Lets `RESEND` pass for the last send on `repo()`.
fn later(membership: &mut Membership) {
    let announced = membership.announced.get_mut(&repo()).unwrap();
    announced.at = Instant::now().checked_sub(RESEND).unwrap();
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

    let mut membership = Membership::default();
    membership.announce(
        &pair.gateway,
        &[added].into(),
        |_| true,
        std::slice::from_ref(&team),
        &projects,
    );

    // Here it waits for a desktop to write it.
    timeout(Duration::from_secs(5), pair.gateway.waiting())
        .await
        .expect("the change waits for a desktop");
    assert!(
        pair.gateway
            .with(|teams| teams.rosters.allows(added, &repo()))
    );

    // A teammate takes it from the gateway, a device already on the entry.
    let entry = membership.announced[&repo()].entry.clone();
    let roster = TeamMsg::Roster {
        repo: repo(),
        members: vec![entry],
    };
    assert_eq!(pair.bob_link.answer(gateway, &roster).await.sent(), None);
    assert!(
        pair.bob_link
            .with(|teams| teams.rosters.allows(added, &repo()))
    );
}

#[tokio::test]
async fn a_teammate_offline_at_the_announcement_gets_it_later() {
    let pair = Pair::new().await;
    let added = SecretKey::generate().public();
    let mut devices = pair.alice.devices.clone();
    devices.push(EndpointAddr::new(added));
    let projects = projects(devices);
    let team = Team {
        repo: repo(),
        project: "org/repo".to_owned(),
        mine: pair.alice.clone(),
        others: vec![reachable_bob(&pair)],
    };
    let teams = std::slice::from_ref(&team);
    bob_knows_the_team(&pair);
    let mut membership = Membership::default();
    membership.announce(&pair.gateway, &BTreeSet::new(), |_| true, teams, &projects);
    assert_eq!(sent(&mut membership).await, [pair.bob.mesh].into());

    // Bob comes online; meanwhile the mesh wrote another change, which
    // keeps the one pending here.
    let _bob = online(&pair);
    pair.gateway.with(|teams| {
        let mut record = teams.rosters.written(&repo()).unwrap().clone();
        record.asks = false;
        teams.rosters.refresh([(repo(), record)].into());
    });
    later(&mut membership);

    membership.announce(&pair.gateway, &BTreeSet::new(), |_| true, teams, &projects);

    assert!(
        pair.gateway
            .with(|teams| teams.rosters.allows(added, &repo()))
    );
    assert!(sent(&mut membership).await.is_empty());
    assert!(
        pair.bob_link
            .with(|teams| teams.rosters.allows(added, &repo()))
    );
}

#[tokio::test]
async fn a_teammate_offline_at_the_announcement_gets_it_after_the_gateway_moves() {
    let pair = Pair::new().await;
    let (added_link, _added) = device().await;
    let added = added_link.endpoint().id();
    let mut devices = pair.alice.devices.clone();
    devices.push(EndpointAddr::new(added));
    let projects = projects(devices);
    let team = Team {
        repo: repo(),
        project: "org/repo".to_owned(),
        mine: pair.alice.clone(),
        others: vec![reachable_bob(&pair)],
    };
    bob_knows_the_team(&pair);
    let mut old = Membership::default();
    old.announce(
        &pair.gateway,
        &BTreeSet::new(),
        |_| true,
        std::slice::from_ref(&team),
        &projects,
    );
    assert_eq!(sent(&mut old).await, [pair.bob.mesh].into());

    // A desktop writes the entry with the added machine, which takes the
    // gateway: the old one no longer holds the team.
    let entry = old.announced[&repo()].entry.clone();
    let mut written = pair
        .gateway
        .with(|teams| teams.rosters.written(&repo()).unwrap().clone());
    written.members[0] = entry.clone();
    for link in [&pair.gateway, &added_link] {
        link.with(|teams| teams.rosters.refresh([(repo(), written.clone())].into()));
        link.edit_home(|home| home.mesh = Some(pair.alice.mesh));
    }
    old.announce(&pair.gateway, &BTreeSet::new(), |_| true, &[], &projects);
    let moved = Team {
        mine: entry,
        ..team
    };
    let mut new = Membership::default();
    new.announce(
        &added_link,
        &BTreeSet::new(),
        |_| true,
        std::slice::from_ref(&moved),
        &projects,
    );
    // Taking the team sends the written entry, which bob, offline, misses.
    assert_eq!(sent(&mut new).await, [pair.bob.mesh].into());

    // Bob comes online. The old gateway, a device he knows, sends it again.
    let _bob = online(&pair);
    later(&mut old);
    old.announce(&pair.gateway, &BTreeSet::new(), |_| true, &[], &projects);
    assert!(sent(&mut old).await.is_empty());
    assert!(
        pair.bob_link
            .with(|teams| teams.rosters.allows(added, &repo()))
    );

    // From then on he takes the new gateway's sends too.
    later(&mut new);
    new.announce(
        &added_link,
        &BTreeSet::new(),
        |_| true,
        std::slice::from_ref(&moved),
        &projects,
    );
    assert!(sent(&mut new).await.is_empty());
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
    let (record, pending) = pair.gateway.with(|teams| {
        let record = teams.rosters.get(&repo()).unwrap().clone();
        (record, teams.rosters.pending())
    });
    let logins: Vec<&ForgeLogin> = record
        .members
        .iter()
        .map(|member| &member.login)
        .collect();
    assert_eq!(logins, [&pair.alice.login, &pair.bob.login]);
    assert_eq!(pending[0].removed[0].login, carol.login);
}

/// Carol's own forge account may only read now: her gateway takes her mesh
/// off the team, for a desktop to write, and tells the others she left.
#[tokio::test]
async fn a_member_who_may_only_read_leaves() {
    let pair = Pair::new().await;
    let carol = member("carol", pair.gateway.endpoint().id());
    pair.gateway.with(|teams| {
        let record = TeamRecord::new(repo(), vec![pair.bob.clone(), carol.clone()]);
        teams.rosters.refresh([(repo(), record)].into());
    });
    pair.gateway.edit_home(|home| home.mesh = Some(carol.mesh));
    let team = Team {
        repo: repo(),
        project: "org/repo".to_owned(),
        mine: carol.clone(),
        others: vec![pair.bob.clone()],
    };

    check_members(&pair.gateway, &team).await;

    timeout(Duration::from_secs(5), pair.gateway.waiting())
        .await
        .expect("the change waits for a desktop");
    let pending = pair.gateway.with(|teams| teams.rosters.pending());
    assert!(pending[0].members.iter().all(|member| member.mesh != carol.mesh));
    assert_eq!(pending[0].removed[0].login, carol.login);
}

/// Dana was removed, then let back in and admitted again by Alice's mesh
/// while Bob was offline. Alice's gateway tells him of her with its own
/// entry once he is back, and his old removal of her ends.
#[tokio::test]
async fn a_teammate_offline_at_a_rejoin_takes_the_member_later() {
    let pair = Pair::new().await;
    let dana = member("dana", SecretKey::generate().public());
    let mut bobs = TeamRecord::new(repo(), vec![pair.alice.clone(), pair.bob.clone(), dana.clone()]);
    bobs.remove(&dana.login);
    pair.bob_link.with(|teams| teams.rosters.refresh([(repo(), bobs)].into()));
    pair.bob_link.edit_home(|home| home.mesh = Some(pair.bob.mesh));
    let again = Member {
        joined_at: dana.joined_at + Duration::from_secs(60),
        admitted_by: Some(pair.alice.mesh),
        ..dana
    };
    pair.gateway.with(|teams| {
        let mut team = teams.rosters.get(&repo()).unwrap().clone();
        team.members.push(again.clone());
        teams.rosters.refresh([(repo(), team)].into());
    });
    let team = Team {
        repo: repo(),
        project: "org/repo".to_owned(),
        mine: pair.alice.clone(),
        others: vec![reachable_bob(&pair), again.clone()],
    };
    let teams = std::slice::from_ref(&team);
    let projects = projects(pair.alice.devices.clone());
    let mut membership = Membership::default();
    membership.announce(&pair.gateway, &BTreeSet::new(), |_| true, teams, &projects);
    assert!(sent(&mut membership).await.contains(&pair.bob.mesh));

    let _bob = online(&pair);
    later(&mut membership);
    membership.announce(&pair.gateway, &BTreeSet::new(), |_| true, teams, &projects);
    assert!(!sent(&mut membership).await.contains(&pair.bob.mesh));

    let (allowed, removed) = pair.bob_link.with(|teams| {
        let record = teams.rosters.get(&repo()).unwrap();
        (teams.rosters.allows(again.devices[0].id, &repo()), record.removed.clone())
    });
    assert!(allowed);
    assert!(removed.is_empty());
}

/// Carol is removed while Bob is offline: once he is back, the removal
/// reaches him and he turns her away too.
#[tokio::test]
async fn a_teammate_offline_at_a_removal_gets_it_later() {
    let pair = Pair::new().await;
    let carol = member("carol", SecretKey::generate().public());
    pair.gateway.with(|teams| {
        let mut team = teams.rosters.get(&repo()).unwrap().clone();
        team.members.push(carol.clone());
        teams.rosters.refresh([(repo(), team)].into());
    });
    bob_knows_the_team(&pair);
    let team = Team {
        repo: repo(),
        project: "org/repo".to_owned(),
        mine: pair.alice.clone(),
        others: vec![reachable_bob(&pair), carol.clone()],
    };
    let carol_device = carol.devices[0].id;

    // The first send to him gives up after the 5 s a dial gets.
    remove(&pair.gateway, &team, &carol.login);
    tokio::time::sleep(Duration::from_secs(6)).await;
    let _bob = online(&pair);

    timeout(Duration::from_secs(30), async {
        while pair
            .bob_link
            .with(|teams| teams.rosters.allows(carol_device, &repo()))
        {
            pair.bob_link.waiting().await;
        }
    })
    .await
    .expect("the removal reaches bob");
}

/// A gateway whose entry names none of the project's machines, the last
/// checkout having moved, still takes the entry naming itself.
#[tokio::test]
async fn a_gateway_not_on_the_entry_takes_the_entry_naming_it() {
    let pair = Pair::new().await;
    let gateway = pair.gateway.endpoint().id();
    let old = SecretKey::generate().public();
    let mine = member("alice", old);
    let record = TeamRecord::new(repo(), vec![mine.clone(), pair.bob.clone()]);
    pair.gateway.with(|teams| teams.rosters.refresh([(repo(), record)].into()));
    pair.gateway.edit_home(|home| home.mesh = Some(mine.mesh));
    let team = Team {
        repo: repo(),
        project: "org/repo".to_owned(),
        mine,
        others: vec![pair.bob.clone()],
    };
    let projects = projects(vec![EndpointAddr::new(gateway)]);

    let mut membership = Membership::default();
    membership.announce(
        &pair.gateway,
        &[gateway].into(),
        |id| id != old,
        std::slice::from_ref(&team),
        &projects,
    );

    timeout(Duration::from_secs(5), pair.gateway.waiting())
        .await
        .expect("the change waits for a desktop");
    assert!(
        pair.gateway
            .with(|teams| teams.rosters.allows(gateway, &repo()))
    );
}
