use super::*;
use crate::{
    machine::ci::projects::pinned,
    team::{home::read, link::TeamForge, roster::record},
};
use beep_mesh::Key;
use beep_proto::{
    node::{
        mesh_state::{Place, ProjectRecord, StateValue},
        tools::HostReach,
    },
    team::{Card, Lane},
};
use futures::future::BoxFuture;
use iroh::{
    Endpoint, EndpointAddr, SecretKey, address_lookup::memory::MemoryLookup,
    endpoint::presets, protocol::Router,
};
use std::sync::Arc;

const URL: &str = "https://git.example.com:3000/org/repo";

fn repo() -> TeamRepo {
    URL.parse().unwrap()
}

fn login(name: &str) -> ForgeLogin {
    name.parse().unwrap()
}

/// A forge that says carol may not push, anyone else may, and lists no issues.
struct Forge;

impl TeamForge for Forge {
    fn collaborator<'a>(
        &'a self,
        _repo: &'a TeamRepo,
        login: &'a ForgeLogin,
    ) -> BoxFuture<'a, Result<bool, String>> {
        Box::pin(async move { Ok(login.to_string() != "carol") })
    }

    fn login<'a>(&'a self, _repo: &'a TeamRepo) -> BoxFuture<'a, Result<ForgeLogin, String>> {
        Box::pin(async { Ok(login("alice")) })
    }

    fn issue_titles<'a>(&'a self, _repo: &'a TeamRepo) -> BoxFuture<'a, Result<IssueTitles, String>> {
        Box::pin(async { Ok(IssueTitles::default()) })
    }
}

fn member(name: &str, device: EndpointId) -> Member {
    Member {
        login: login(name),
        mesh: SecretKey::generate().public(),
        devices: vec![EndpointAddr::new(device)],
        joined_at: SystemTime::now(),
    }
}

fn card(id: &str, owner: &str, lane: Lane) -> Card {
    Card {
        id: id.to_owned(),
        title: "Fix the fee model".to_owned(),
        lane,
        login: login(owner),
        machine: "Build box".to_owned(),
        issue: Some(52),
        pr: None,
        updated_at: SystemTime::now(),
    }
}

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

/// A gateway serving `repo()`'s board with alice and bob on the team, and
/// bob's device, which finds it. Alice's laptop, a device of hers that does
/// not serve the board, comes first in her devices.
struct Pair {
    gateway: TeamLink,
    alice: Member,
    bob: Member,
    bob_link: TeamLink,
    _routers: [Router; 2],
}

/// A device of the team on its own endpoint, served.
async fn device() -> (TeamLink, Router) {
    let endpoint = Endpoint::builder(presets::Minimal).bind().await.unwrap();
    let link = TeamLink::new(endpoint.clone(), Arc::new(Forge));
    let router = Router::builder(endpoint)
        .accept(beep_proto::team::ALPN, link.handler())
        .spawn();

    (link, router)
}

impl Pair {
    async fn new() -> Self {
        let (gateway, gateway_router) = device().await;
        let (laptop, laptop_router) = device().await;
        let lookup = MemoryLookup::new();
        lookup.add_endpoint_info(gateway.endpoint().addr());
        lookup.add_endpoint_info(laptop.endpoint().addr());
        let bob_endpoint = Endpoint::builder(presets::Minimal)
            .address_lookup(lookup)
            .bind()
            .await
            .unwrap();
        let mut alice = member("alice", laptop.endpoint().id());
        alice
            .devices
            .push(EndpointAddr::new(gateway.endpoint().id()));
        let bob = member("bob", bob_endpoint.id());
        let team = record(repo(), vec![alice.clone(), bob.clone()]);

        for link in [&gateway, &laptop] {
            link.with(|teams| {
                teams.rosters.refresh([(repo(), team.clone())].into());
                teams.home.mesh = Some(alice.mesh);
            });
        }

        Self {
            gateway,
            alice,
            bob,
            bob_link: TeamLink::new(bob_endpoint, Arc::new(Forge)),
            _routers: [gateway_router, laptop_router],
        }
    }

    fn set(&self, cards: Vec<Card>) {
        self.gateway
            .with(|teams| teams.boards.set(&repo(), cards));
    }
}

async fn next(heard: &mut mpsc::Receiver<Followed>) -> Heard {
    timeout(Duration::from_secs(10), heard.recv())
        .await
        .unwrap()
        .unwrap()
        .heard
}

#[tokio::test]
async fn a_teammate_follows_the_board_until_removed() {
    let pair = Pair::new().await;
    let working = card("s1@build-box", "alice", Lane::Working {});
    pair.set(vec![working.clone()]);
    let (tx, mut heard) = mpsc::channel(8);
    let _follow = AbortOnDropHandle::new(tokio::spawn(follow(
        pair.bob_link.clone(),
        repo(),
        pair.alice.clone(),
        tx,
    )));

    let Heard::Board(cards) = next(&mut heard).await else {
        panic!("the whole board comes first");
    };
    assert_eq!(cards, vec![working.clone()]);

    let mut done = working.clone();
    done.lane = Lane::ReadyToReview {};
    pair.set(vec![done.clone()]);
    let Heard::Change(card, false) = next(&mut heard).await else {
        panic!("a change follows");
    };
    assert_eq!(card, done);

    pair.set(Vec::new());
    let Heard::Change(card, true) = next(&mut heard).await else {
        panic!("a card that went is gone");
    };
    assert_eq!(card.id, done.id);

    // Once bob is off the team, the next change ends his subscription.
    pair.gateway
        .with(|teams| teams.rosters.remove_member(&repo(), &pair.bob.login));
    pair.set(vec![working]);
    assert!(matches!(next(&mut heard).await, Heard::Gone));
}

#[tokio::test]
async fn a_device_that_does_not_serve_the_board_refuses() {
    let pair = Pair::new().await;
    let (mut tx, mut rx) = pair
        .bob_link
        .dial(&pair.alice.devices[..1])
        .await
        .unwrap();

    tx.send(&TeamMsg::Subscribe { repo: repo() }).await.unwrap();

    let answer = rx.recv::<TeamMsg>().await.unwrap();
    assert!(matches!(answer, Some(TeamMsg::Refused { .. })), "{answer:?}");
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
