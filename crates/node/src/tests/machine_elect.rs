use super::{ASK_LIMIT, QUIET, Reaches, Said, Streamed, elect, ranked};
use beep_mesh::Mesh;
use beep_proto::node::tools::{ForgeReach, HostReach};
use iroh::{Endpoint, endpoint::presets};
use iroh::{EndpointId, SecretKey};
use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

const HOST: &str = "git.example.com";

fn ids(n: usize) -> Vec<EndpointId> {
    let mut ids: Vec<EndpointId> = (0..n).map(|_| SecretKey::generate().public()).collect();
    ids.sort();
    ids
}

fn reach(reached: bool, failures: u32) -> HostReach {
    HostReach { reached, failures }
}

/// The machine each of `online` elects over `machines`, seeing the others.
fn elected(
    machines: &BTreeSet<EndpointId>,
    reaches: &BTreeMap<EndpointId, HostReach>,
    online: &BTreeSet<EndpointId>,
) -> BTreeSet<Option<EndpointId>> {
    online
        .iter()
        .map(|me| {
            let peers = online
                .iter()
                .filter(|peer| *peer != me)
                .map(|peer| {
                    let mut said = ForgeReach::default();
                    said.hosts.insert(HOST.to_owned(), reaches[peer]);
                    (*peer, said)
                })
                .collect();
            elect(machines, *me, reaches[me], &peers, HOST)
        })
        .collect()
}

#[test]
fn every_machine_elects_the_same_one_and_it_moves_when_that_one_goes() {
    let ids = ids(3);
    let machines: BTreeSet<EndpointId> = ids.iter().copied().collect();
    let mut reaches: BTreeMap<EndpointId, HostReach> =
        ids.iter().map(|id| (*id, reach(false, 0))).collect();
    let mut online = machines.clone();
    // Nobody tried the host yet: the smallest id, on every machine.
    assert_eq!(elected(&machines, &reaches, &online), [Some(ids[0])].into());
    // The first goes offline: the others both move to the next.
    online.remove(&ids[0]);
    assert_eq!(elected(&machines, &reaches, &online), [Some(ids[1])].into());
    // A machine that reaches the host comes before one never tried.
    online.insert(ids[0]);
    reaches.insert(ids[2], reach(true, 0));
    assert_eq!(elected(&machines, &reaches, &online), [Some(ids[2])].into());
    // It fails once and keeps the role; three in a row and it gives it up.
    reaches.insert(ids[2], reach(true, 1));
    assert_eq!(elected(&machines, &reaches, &online), [Some(ids[2])].into());
    reaches.insert(ids[2], reach(true, HostReach::GIVE_UP));
    assert_eq!(elected(&machines, &reaches, &online), [Some(ids[0])].into());
    // A machine outside the set never wins.
    let elsewhere: BTreeSet<EndpointId> = ids[1..].iter().copied().collect();
    reaches.insert(ids[0], reach(true, 0));
    assert_eq!(
        elected(&elsewhere, &reaches, &online),
        [Some(ids[1])].into()
    );
}

/// Pickup asks the next machine when the first refuses, as one with no
/// coding agent does.
#[test]
fn the_candidates_are_ranked_as_the_election_goes() {
    let ids = ids(4);
    let [me, tried, fresh, silent] = [ids[0], ids[1], ids[2], ids[3]];
    let machines: BTreeSet<EndpointId> = ids.iter().copied().collect();
    let said = |reach: HostReach| {
        let mut said = ForgeReach::default();
        said.hosts.insert(HOST.to_owned(), reach);
        said
    };
    let peers = [
        (tried, said(reach(true, 0))),
        (fresh, said(reach(false, 0))),
    ]
    .into();

    let order = ranked(&machines, me, reach(false, 0), &peers, HOST);

    // The one that reaches the host first, then by id; one that did not
    // answer is no candidate.
    assert_eq!(order, [tried, me, fresh]);
    assert!(!order.contains(&silent));
    assert_eq!(
        elect(&machines, me, reach(false, 0), &peers, HOST),
        Some(tried)
    );
}

/// The follows of a machine's reach end with the last `Reaches`, so a node that
/// follows a peer can still leave its mesh and join another.
#[tokio::test]
async fn dropping_the_reaches_ends_the_follows_and_lets_go_of_the_mesh() {
    let dir = tempfile::tempdir().unwrap();
    let endpoint = Endpoint::builder(presets::Minimal).bind().await.unwrap();
    let mesh = Mesh::create(&dir.path().join("mesh"), endpoint, "build-box")
        .await
        .unwrap();
    let reaches = Reaches::new(mesh.clone());
    let other = reaches.clone();
    other.track([SecretKey::generate().public()]);
    drop(other);
    drop(reaches);

    tokio::time::timeout(Duration::from_secs(10), mesh.released())
        .await
        .expect("a follow still holds the mesh");
    mesh.close().await.unwrap();
}

/// A machine whose follow ended no longer pushes, so the watcher is woken to
/// ask it again instead of waiting for some other event.
#[tokio::test]
async fn a_machine_that_stops_pushing_rings_the_watcher() {
    let dir = tempfile::tempdir().unwrap();
    let endpoint = Endpoint::builder(presets::Minimal).bind().await.unwrap();
    let mesh = Mesh::create(&dir.path().join("mesh"), endpoint, "build-box")
        .await
        .unwrap();
    let reaches = Reaches::new(mesh.clone());
    let peer = SecretKey::generate().public();
    let mut changes = reaches.changes();
    changes.mark_unchanged();

    // Nothing was pushed, so nothing is lost.
    reaches.hear.forget(peer);
    assert!(!changes.has_changed().unwrap());

    reaches.hear.put(peer, Said::Reach(ForgeReach::default()));
    assert!(reaches.view().contains_key(&peer));
    changes.mark_unchanged();
    reaches.hear.forget(peer);
    assert!(crate::lock(&reaches.hear.said)[&peer] == Said::Pending);
    assert!(changes.has_changed().unwrap());

    drop(reaches);
    mesh.close().await.unwrap();
}

/// An election waits for a machine to say something, and takes what it said.
#[tokio::test]
async fn tracking_a_peer_is_pending_until_it_says_or_stays_silent() {
    let dir = tempfile::tempdir().unwrap();
    let endpoint = Endpoint::builder(presets::Minimal).bind().await.unwrap();
    let mesh = Mesh::create(&dir.path().join("mesh"), endpoint, "build-box")
        .await
        .unwrap();
    let reaches = Reaches::new(mesh.clone());
    let peer = SecretKey::generate().public();
    let wanted: BTreeSet<EndpointId> = [peer].into();
    let mut changes = reaches.changes();

    reaches.track([peer]);
    assert!(!reaches.settled(&wanted));
    assert!(reaches.view().is_empty());
    changes.mark_unchanged();

    reaches.hear.put(peer, Said::Reach(ForgeReach::default()));
    assert!(changes.has_changed().unwrap());
    assert!(reaches.settled(&wanted));
    assert!(reaches.view().contains_key(&peer));

    drop(reaches);
    tokio::time::timeout(Duration::from_secs(10), mesh.released())
        .await
        .expect("a follow still holds the mesh");
    mesh.close().await.unwrap();
}

/// A machine that did not answer in time is no candidate, but it does not
/// hold the election up.
#[tokio::test]
async fn a_silent_peer_settles_the_election_without_it() {
    let dir = tempfile::tempdir().unwrap();
    let endpoint = Endpoint::builder(presets::Minimal).bind().await.unwrap();
    let mesh = Mesh::create(&dir.path().join("mesh"), endpoint, "build-box")
        .await
        .unwrap();
    let reaches = Reaches::new(mesh.clone());
    let peer = SecretKey::generate().public();
    let wanted: BTreeSet<EndpointId> = [peer].into();
    let mut changes = reaches.changes();

    reaches.track([peer]);
    assert!(!reaches.settled(&wanted));
    changes.mark_unchanged();

    reaches.hear.put(peer, Said::Silent);
    assert!(changes.has_changed().unwrap());
    assert!(reaches.settled(&wanted));
    assert!(reaches.view().is_empty());

    drop(reaches);
    tokio::time::timeout(Duration::from_secs(10), mesh.released())
        .await
        .expect("a follow still holds the mesh");
    mesh.close().await.unwrap();
}

/// A mesh of this machine alone.
async fn lone_mesh(dir: &std::path::Path) -> Mesh {
    let endpoint = Endpoint::builder(presets::Minimal).bind().await.unwrap();

    Mesh::create(&dir.join("mesh"), endpoint, "build-box")
        .await
        .unwrap()
}

fn said_of(host: &str) -> ForgeReach {
    let mut said = ForgeReach::default();
    said.hosts.insert(host.to_owned(), reach(true, 0));

    said
}

/// A node that refused the stream, as before 0.8.28 or while it was not
/// lending, is dialled again every `QUIET`: once it pushes, its pushes are heard.
#[tokio::test(start_paused = true)]
async fn a_node_that_refused_is_followed_once_it_accepts() {
    let dir = tempfile::tempdir().unwrap();
    let mesh = lone_mesh(dir.path()).await;
    let reaches = Reaches::scripted(
        mesh,
        [Streamed::Refuse, Streamed::Push(said_of("git.example.com"))],
    );
    let peer = SecretKey::generate().public();

    reaches.track([peer]);
    tokio::time::sleep(ASK_LIMIT + Duration::from_secs(1)).await;
    assert!(reaches.settled(&[peer].into()), "silent after the ask");
    assert!(reaches.view().is_empty());

    tokio::time::sleep(QUIET + ASK_LIMIT).await;
    assert_eq!(reaches.view()[&peer], said_of("git.example.com"));
}

/// A stream that fails at once leaves the machine silent, not pending, so an
/// election does not wait on it for ever.
#[tokio::test(start_paused = true)]
async fn a_stream_that_fails_leaves_the_machine_silent() {
    let dir = tempfile::tempdir().unwrap();
    let mesh = lone_mesh(dir.path()).await;
    let reaches = Reaches::scripted(mesh, [Streamed::Fail]);
    let peer = SecretKey::generate().public();

    reaches.track([peer]);
    assert!(!reaches.settled(&[peer].into()));
    tokio::time::sleep(ASK_LIMIT + Duration::from_secs(1)).await;

    assert!(reaches.settled(&[peer].into()));
    assert!(reaches.view().is_empty());
}

/// A machine tracked as its follow ends is followed again, not left pending
/// with nothing to follow it.
#[tokio::test(start_paused = true)]
async fn a_machine_tracked_as_its_follow_ends_is_followed_again() {
    let dir = tempfile::tempdir().unwrap();
    let mesh = lone_mesh(dir.path()).await;
    let first = said_of("one.example.com");
    let second = said_of("two.example.com");
    let reaches = Reaches::scripted(
        mesh,
        [
            Streamed::Push(first.clone()),
            Streamed::Push(second.clone()),
        ],
    );
    let peer = SecretKey::generate().public();

    reaches.track([peer]);
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert_eq!(reaches.view()[&peer], first);

    // The follow takes its entry away; the next track starts another.
    reaches.hear.leave(peer);
    assert!(reaches.view().is_empty());
    reaches.track([peer]);
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert_eq!(reaches.view()[&peer], second);
}

/// Every holder of the `Reaches` sees what a machine said, silence included.
#[tokio::test]
async fn every_holder_sees_the_same_view_after_a_failed_ask() {
    let dir = tempfile::tempdir().unwrap();
    let mesh = lone_mesh(dir.path()).await;
    let gateway = Reaches::new(mesh);
    let watcher = gateway.clone();
    let silent = SecretKey::generate().public();
    let heard = SecretKey::generate().public();
    let wanted: BTreeSet<EndpointId> = [silent, heard].into();

    gateway.hear.put(silent, Said::Silent);
    gateway
        .hear
        .put(heard, Said::Reach(said_of("git.example.com")));

    assert!(gateway.settled(&wanted) && watcher.settled(&wanted));
    assert_eq!(gateway.view(), watcher.view());
    assert_eq!(watcher.view().keys().copied().collect::<Vec<_>>(), [heard]);
}
