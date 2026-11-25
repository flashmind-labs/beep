use super::{Reaches, elect, ranked};
use beep_mesh::Mesh;
use iroh::{Endpoint, endpoint::presets};
use std::time::Duration;
use beep_proto::node::tools::{ForgeReach, HostReach};
use iroh::{EndpointId, SecretKey};
use std::collections::{BTreeMap, BTreeSet};

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
    assert_eq!(elect(&machines, me, reach(false, 0), &peers, HOST), Some(tried));
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
    other.follow(SecretKey::generate().public());
    drop(other);
    drop(reaches);

    tokio::time::timeout(Duration::from_secs(10), mesh.released())
        .await
        .expect("a follow still holds the mesh");
    mesh.close().await.unwrap();
}
