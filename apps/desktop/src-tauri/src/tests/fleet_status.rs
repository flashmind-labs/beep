use super::{reap, retry_due};
use crate::fleet::tests::{LIMIT, join, online, start, until};
use beep_mesh::DeviceKind;
use std::time::Instant;
use tokio::task::JoinSet;
use tokio::time::timeout;

use super::Status;

#[test]
fn telemetry_does_not_invalidate_machine_metadata() {
    use super::{Peer, Revision, Shared};
    use beep_mesh::{Device, DeviceKind};
    use beep_proto::node::{EnvReport, StatsReport, status::Report};
    use std::{collections::BTreeMap, sync::Mutex};
    let id = iroh::SecretKey::generate().public();
    let shared = Shared {
        peers: Mutex::new(BTreeMap::from([(
            id,
            Peer::new(Device {
                id,
                name: "build-box".into(),
                kind: DeviceKind::Node,
                added_at_ms: 0,
            }),
        )])),
        changes: tokio::sync::watch::channel(Revision::default()).0,
    };
    shared.record(id, Report::Stats(StatsReport::default()));
    assert_eq!(shared.changes.borrow().machines, 0);
    assert_eq!(shared.changes.borrow().telemetry, 1);
    shared.record(id, Report::Env(EnvReport::default()));
    assert_eq!(shared.changes.borrow().machines, 1);
    shared.record(id, Report::Env(EnvReport::default()));
    assert_eq!(shared.changes.borrow().machines, 1);
    shared.record(id, Report::Offers(vec!["events/1".into()]));
    shared.record(id, Report::Offers(vec!["events/1".into()]));
    assert_eq!(shared.changes.borrow().machines, 2);
}

/// Waits until `ok` holds for what the machines have reported.
async fn settled(status: &Status, ok: impl Fn(&Status) -> bool) {
    let mut changes = status.changes();
    timeout(LIMIT, async {
        loop {
            changes.borrow_and_update();

            if ok(status) {
                return;
            }

            changes.changed().await.expect("the status is alive");
        }
    })
    .await
    .expect("the status settled in time");
}

#[tokio::test]
async fn ended_follows_leave_the_set() {
    let mut follows = JoinSet::new();

    for _ in 0..8 {
        follows.spawn(async {});
    }
    // Finished, but not collected until the set is polled.
    while !follows.is_empty() {
        tokio::task::yield_now().await;
        reap(&mut follows);
    }

    assert_eq!(follows.len(), 0);
    follows.spawn(std::future::pending());
    reap(&mut follows);
    assert_eq!(follows.len(), 1, "a live follow stays");
}

#[tokio::test]
async fn a_machine_list_shows_who_is_on_and_off() {
    let mac = start("Studio").await;
    let host = mac.fleet.mesh().expect("the mesh is open");
    let (mesh, router, endpoint) = join(&mac.fleet, DeviceKind::Node, "Server").await;
    let (this, other) = (mac.endpoint.id().to_string(), endpoint.id().to_string());
    let status = mac.fleet.status();
    // This Mac's node says what it is; the other machine is on.
    settled(status, |status| {
        let machines = status.machines();
        let on = |id: &str| machines.iter().any(|m| m.id == id && m.online);
        let versioned = machines
            .iter()
            .any(|m| m.id == this && !m.version.is_empty());
        on(&this) && on(&other) && versioned && status.details().len() == 2
    })
    .await;
    let machines = status.machines();
    let studio = machines.iter().find(|m| m.id == this).expect("listed");
    assert_eq!(studio.name, "Studio");
    // The node said its version, and a joined device that runs no node has none.
    assert!(studio.version.starts_with(|c: char| c.is_ascii_digit()));
    // Switched off: its endpoint closes, and with it the link.
    let on_until = std::time::SystemTime::now();
    router.shutdown().await.expect("the router stops");
    drop(mesh);
    endpoint.close().await;
    until(&host, |mesh| !online(mesh, endpoint.id())).await;
    settled(status, |status| {
        status.machines().iter().any(|m| m.id == other && !m.online)
    })
    .await;
    let machines = status.machines();
    assert!(machines.iter().any(|m| m.id == this && m.online));
    let off = machines.iter().find(|m| m.id == other).expect("listed");
    assert!(!off.online);
    // Seen when its link went, not when it joined (#329).
    assert!(
        off.seen_at >= on_until,
        "seen when it joined, not when it went"
    );
}

#[tokio::test]
async fn removing_a_machine_cuts_it_off_from_every_other() {
    let mac = start("Studio").await;
    let host = mac.fleet.mesh().expect("the mesh is open");
    let (first, _first_router, first_end) = join(&mac.fleet, DeviceKind::Node, "Box").await;
    let (second, _second_router, second_end) = join(&mac.fleet, DeviceKind::Node, "Rig").await;
    let (gone, kept) = (first_end.id(), second_end.id());
    until(&host, |mesh| online(mesh, gone) && online(mesh, kept)).await;
    until(&second, |mesh| online(mesh, gone)).await;
    mac.fleet.remove(gone).await.expect("a desktop removes it");
    // The others drop it from their lists and lose its link.
    until(&second, |mesh| !mesh.is_member(gone)).await;
    assert!(!host.is_member(gone));
    assert!(!second.machines().iter().any(|d| d.device.id == gone));
    until(&first, |mesh| !mesh.is_member(gone)).await;
    let status = mac.fleet.status();
    settled(status, |status| {
        status.machines().iter().all(|m| m.id != gone.to_string())
    })
    .await;
    assert!(
        mac.fleet.client(gone).await.is_err(),
        "a removed machine is not reachable"
    );
}

#[test]
fn a_follow_that_ended_waits_before_it_starts_again() {
    assert!(retry_due(None));
    assert!(!retry_due(Some(Instant::now())));
    let long_ago = Instant::now()
        .checked_sub(super::RETRY)
        .expect("the clock has run that long");
    assert!(retry_due(Some(long_ago)));
}

#[tokio::test]
async fn what_a_machine_reported_survives_a_swap_of_the_mesh() {
    let mac = start("Studio").await;
    let status = mac.fleet.status();
    let this = mac.endpoint.id().to_string();
    settled(status, |status| {
        status
            .machines()
            .iter()
            .any(|m| m.id == this && m.env.is_some())
    })
    .await;
    status
        .watch(mac.fleet.mesh().expect("the mesh is open"))
        .await;
    let machines = status.machines();
    assert!(machines.iter().any(|m| m.id == this && m.env.is_some()));
}

#[tokio::test]
async fn a_stalled_version_probe_does_not_delay_status_reports() {
    use super::{Peer, Revision, Shared, follow};
    use beep_mesh::{Device, PeerClient, SessionFn};
    use beep_proto::node::{
        Open, Opened, StatsReport,
        status::{PROTOCOL, Report},
        version,
    };
    use std::{
        collections::BTreeMap,
        sync::{Arc, Mutex},
        time::Duration,
    };
    use tokio::sync::{oneshot, watch};

    let id = iroh::SecretKey::generate().public();
    let shared = Arc::new(Shared {
        peers: Mutex::new(BTreeMap::from([(
            id,
            Peer::new(Device {
                id,
                name: "build-box".into(),
                kind: DeviceKind::Node,
                added_at_ms: 0,
            }),
        )])),
        changes: watch::channel(Revision::default()).0,
    });
    let (started, version_started) = oneshot::channel();
    let started = Arc::new(Mutex::new(Some(started)));
    let session: SessionFn = Arc::new(move |mut tx, mut rx| {
        let started = started.clone();
        Box::pin(async move {
            let Some(Open { protocol }) = rx.recv::<Open>().await.unwrap() else {
                return;
            };
            tx.send(&Opened::Ready {}).await.unwrap();
            if protocol == version::PROTOCOL {
                let sender = started.lock().unwrap().take().unwrap();
                sender.send(()).unwrap();
                std::future::pending::<()>().await;
            } else {
                assert_eq!(protocol, PROTOCOL);
                tx.send(&Report::Stats(StatsReport::default()))
                    .await
                    .unwrap();
                let _ = rx.recv::<u8>().await;
            }
        })
    });
    let mut changes = shared.changes.subscribe();
    let mut follows = JoinSet::new();
    follows.spawn(follow(PeerClient::local(session), id, shared));
    timeout(Duration::from_secs(1), version_started)
        .await
        .unwrap()
        .unwrap();
    timeout(
        Duration::from_secs(1),
        changes.wait_for(|revision| revision.telemetry > 0),
    )
    .await
    .unwrap()
    .unwrap();
}
