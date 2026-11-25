use super::*;
use crate::team::home::TeamForge;
use crate::team::link::{
    TeamLink,
    board_io::{Followed, Heard, follow},
};
use beep_proto::team::{Card, ForgeLogin, Lane, Member, TeamMsg, TeamRecord};
use futures::future::BoxFuture;
use iroh::{
    Endpoint, EndpointAddr, SecretKey, address_lookup::memory::MemoryLookup, endpoint::presets,
    protocol::Router,
};
use std::{sync::Arc, time::{Duration, SystemTime}};
use iroh::EndpointId;
use tokio::sync::mpsc;
use tokio::time::timeout;
use tokio_util::task::AbortOnDropHandle;

pub(crate) const URL: &str = "https://git.example.com:3000/org/repo";

pub(crate) fn repo() -> TeamRepo {
    URL.parse().unwrap()
}

pub(crate) fn login(name: &str) -> ForgeLogin {
    name.parse().unwrap()
}

/// A forge that says carol may not push, anyone else may, and lists no issues.
pub(crate) struct Forge;

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

    fn open_issues<'a>(
        &'a self,
        _repo: &'a TeamRepo,
    ) -> BoxFuture<'a, Result<Vec<beep_projects::commands::OpenIssue>, String>> {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn assigned_issues<'a>(
        &'a self,
        _repo: &'a TeamRepo,
        _login: &'a ForgeLogin,
    ) -> BoxFuture<'a, Result<Vec<beep_projects::commands::OpenIssue>, String>> {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn self_assigned<'a>(
        &'a self,
        _repo: &'a TeamRepo,
        _number: u64,
        _login: &'a ForgeLogin,
    ) -> BoxFuture<'a, Result<bool, String>> {
        Box::pin(async { Ok(false) })
    }

    fn pull_heads<'a>(
        &'a self,
        _repo: &'a TeamRepo,
        _branch: &'a str,
    ) -> BoxFuture<'a, Result<Vec<String>, String>> {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn stamp<'a>(
        &'a self,
        _repo: &'a TeamRepo,
        _sha: &'a str,
        _context: &'a str,
    ) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async { Ok(()) })
    }

    fn stampers<'a>(
        &'a self,
        _repo: &'a TeamRepo,
        _sha: &'a str,
        _context: &'a str,
    ) -> BoxFuture<'a, Result<Vec<String>, String>> {
        Box::pin(async { Ok(Vec::new()) })
    }
}

pub(crate) fn member(name: &str, device: EndpointId) -> Member {
    Member {
        login: login(name),
        mesh: SecretKey::generate().public(),
        devices: vec![EndpointAddr::new(device)],
        joined_at: SystemTime::now(),
        admitted_by: None,
        proof: None,
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

/// A gateway serving `repo()`'s board with alice and bob on the team, and
/// bob's device, which finds it. Alice's laptop, a device of hers that does
/// not serve the board, comes first in her devices.
pub(crate) struct Pair {
    pub(crate) gateway: TeamLink,
    pub(crate) alice: Member,
    pub(crate) bob: Member,
    pub(crate) bob_link: TeamLink,
    _routers: [Router; 2],
}

/// A device of the team on its own endpoint, served.
pub(crate) async fn device() -> (TeamLink, Router) {
    let endpoint = Endpoint::builder(presets::Minimal).bind().await.unwrap();
    let link = TeamLink::new(endpoint.clone(), Arc::new(Forge));
    let router = Router::builder(endpoint)
        .accept(beep_proto::team::ALPN, link.handler())
        .spawn();

    (link, router)
}

impl Pair {
    pub(crate) async fn new() -> Self {
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
        let team = TeamRecord::new(repo(), vec![alice.clone(), bob.clone()]);

        for link in [&gateway, &laptop] {
            link.with(|teams| teams.rosters.refresh([(repo(), team.clone())].into()));
            link.edit_home(|home| home.mesh = Some(alice.mesh));
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
        self.gateway.publish(|served| served.boards.set(&repo(), cards));
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
    let (mut tx, mut rx) = pair.bob_link.dial(&pair.alice.devices[..1]).await.unwrap();

    tx.send(&TeamMsg::Subscribe { repo: repo() }).await.unwrap();

    let answer = rx.recv::<TeamMsg>().await.unwrap();
    assert!(
        matches!(answer, Some(TeamMsg::Refused { .. })),
        "{answer:?}"
    );
}

/// Takes a team stream and never answers on it.
#[derive(Debug)]
struct Stalls;

impl iroh::protocol::ProtocolHandler for Stalls {
    async fn accept(
        &self,
        connection: iroh::endpoint::Connection,
    ) -> Result<(), iroh::protocol::AcceptError> {
        let _stream = connection.accept_bi().await;
        std::future::pending().await
    }
}

#[tokio::test]
async fn a_device_that_stalls_gives_way_to_the_next() {
    let pair = Pair::new().await;
    pair.set(vec![card("s1@build-box", "alice", Lane::Working {})]);
    let stalling = Endpoint::builder(presets::Minimal).bind().await.unwrap();
    let addr = stalling.addr();
    let _stalls = Router::builder(stalling)
        .accept(beep_proto::team::ALPN, Stalls)
        .spawn();
    let mut alice = pair.alice.clone();
    alice.devices.insert(0, addr);
    let (tx, mut heard) = mpsc::channel(8);
    let _follow = AbortOnDropHandle::new(tokio::spawn(follow(
        pair.bob_link.clone(),
        repo(),
        alice,
        tx,
    )));

    assert!(matches!(next(&mut heard).await, Heard::Board(_)));
}
