use super::*;
use crate::direct_syncs::SYNC_FRESH;
use crate::{Machines, MeshState, StandIn};
use beep_mesh::{Mesh, transport_config};
use beep_proto::node::mesh_state::StateValue;
use beep_proto::{
    ModelId,
    node::harness::{AutoCompact, HarnessDriver},
};
use iroh::{Endpoint, endpoint::presets};
use std::{sync::Arc, time::Instant};
use tempfile::TempDir;

#[tokio::test]
async fn master_pause_preserves_preferences_and_resumes_new_projects() {
    let dir = TempDir::new().unwrap();
    let endpoint = Endpoint::builder(presets::Minimal)
        .transport_config(transport_config())
        .bind()
        .await
        .unwrap();
    let mesh = Mesh::create(dir.path(), endpoint.clone(), "desktop")
        .await
        .unwrap();
    let desk = Desk::open(Machines::for_tests(mesh.clone())).await.unwrap();
    for (name, paused) in [
        ("acme/one", false),
        ("acme/two", false),
        ("acme/paused", true),
    ] {
        mesh.set(
            Key::Sync(name.into()),
            Some(
                SyncSpec {
                    paused,
                    ..SyncSpec::default()
                }
                .to_bytes()
                .unwrap(),
            ),
        )
        .await
        .unwrap();
    }
    assert!(desk.project_sync_enabled());
    desk.set_project_sync_enabled(false).await.unwrap();
    assert!(!desk.project_sync_enabled());
    assert!(
        scan::<SyncSpec>(&mesh, "sync/")
            .iter()
            .all(|(_, spec)| spec.paused)
    );
    // Retrying disable must not treat the master pause as each project's preference.
    desk.set_project_sync_enabled(false).await.unwrap();
    mesh.set(
        Key::Sync("acme/new".into()),
        Some(
            SyncSpec {
                paused: true,
                ..SyncSpec::default()
            }
            .to_bytes()
            .unwrap(),
        ),
    )
    .await
    .unwrap();
    desk.pause_sync("acme/one", true).await.unwrap();
    desk.set_project_sync_enabled(true).await.unwrap();
    assert!(desk.project_sync_enabled());
    assert!(
        read::<SyncSpec>(&mesh, &Key::Sync("acme/one".into()))
            .unwrap()
            .paused
    );
    assert!(
        read::<SyncSpec>(&mesh, &Key::Sync("acme/paused".into()))
            .unwrap()
            .paused
    );
    assert!(
        !read::<SyncSpec>(&mesh, &Key::Sync("acme/two".into()))
            .unwrap()
            .paused
    );
    assert!(
        !read::<SyncSpec>(&mesh, &Key::Sync("acme/new".into()))
            .unwrap()
            .paused
    );
    endpoint.close().await;
}

#[tokio::test]
async fn reading_ignored_sync_does_not_wait_for_a_write() {
    use futures::FutureExt;

    let dir = TempDir::new().unwrap();
    let endpoint = Endpoint::builder(presets::Minimal)
        .transport_config(transport_config())
        .bind()
        .await
        .unwrap();
    let mesh = Mesh::create(dir.path(), endpoint.clone(), "desktop")
        .await
        .unwrap();
    let desk = Desk::open(Machines::for_tests(mesh)).await.unwrap();
    let _writing = WRITES.lock().await;

    let read = desk
        .project_ignored_sync("acme/missing", None)
        .now_or_never();
    assert!(matches!(read, Some(Err(FleetError::NotFound))));

    let write = desk
        .project_ignored_sync(
            "acme/missing",
            Some(beep_proto::node::sync::IgnoredSync::default()),
        )
        .now_or_never();
    assert!(write.is_none());
}

#[tokio::test]
async fn two_harness_saves_at_once_keep_both_fields() {
    let dir = TempDir::new().unwrap();
    let endpoint = Endpoint::builder(presets::Minimal)
        .transport_config(transport_config())
        .bind()
        .await
        .unwrap();
    let mesh = Mesh::create(dir.path(), endpoint.clone(), "desktop")
        .await
        .unwrap();
    let desk = Desk::open(Machines::for_tests(mesh)).await.unwrap();

    let model = ModelId::from("some-model");
    let compact = AutoCompact {
        enabled: true,
        percent: Some(50),
        tokens: None,
    };
    let (a, b) = tokio::join!(
        desk.save_harness_default_model("codex", Some(model.clone())),
        desk.save_harness_compact("codex", Some(compact)),
    );
    a.unwrap();
    b.unwrap();

    assert_eq!(desk.harness_default_model("codex").unwrap(), Some(model));
    assert_eq!(desk.harness_compact("codex").unwrap(), Some(compact));

    // Only Claude Code and Codex run directly; the choice keeps the other
    // settings.
    assert!(
        desk.save_harness_driver("agy", HarnessDriver::Direct)
            .await
            .is_err()
    );
    desk.save_harness_driver("codex", HarnessDriver::Direct)
        .await
        .unwrap();
    assert_eq!(desk.harness_driver("codex").unwrap(), HarnessDriver::Direct);
    assert_eq!(desk.harness_driver("claude").unwrap(), HarnessDriver::Acp);
    assert_eq!(desk.harness_compact("codex").unwrap(), Some(compact));
}

/// A node that answers `Status` with `projects`.
fn answering(projects: Vec<ProjectStatus>) -> beep_mesh::SessionFn {
    Arc::new(move |mut tx, mut rx| {
        let projects = projects.clone();
        Box::pin(async move {
            let _ = rx.recv::<beep_proto::node::Open>().await;
            let _ = tx.send(&beep_proto::node::Opened::Ready {}).await;
            let _ = rx.recv::<Request>().await;
            let _ = tx.send(&Reply::Status { projects }).await;
        })
    })
}

#[tokio::test]
async fn degraded_node_does_not_stall_sync_and_shows_its_answer_on_the_next_read() {
    use crate::session::{Options, Session};
    use beep_mesh::{DeviceKind, JOIN_ALPN, PEER_ALPN};

    let serve = |mesh: &Mesh, endpoint: &Endpoint| {
        let (peer, join) = mesh.protocols();
        beep_mesh::router(endpoint)
            .accept(PEER_ALPN, peer)
            .accept(JOIN_ALPN, join)
            .spawn()
    };

    let dir = TempDir::new().unwrap();
    let other_dir = TempDir::new().unwrap();
    let ep1 = Endpoint::builder(presets::Minimal)
        .transport_config(transport_config())
        .bind()
        .await
        .unwrap();
    let ep2 = Endpoint::builder(presets::Minimal)
        .transport_config(transport_config())
        .bind()
        .await
        .unwrap();
    let mesh = Mesh::create(dir.path(), ep1.clone(), "desktop")
        .await
        .unwrap();
    let r1 = serve(&mesh, &ep1);
    let ticket = mesh
        .invite(DeviceKind::Node, Duration::from_secs(60))
        .await
        .unwrap();
    let node = Mesh::join(other_dir.path(), ep2.clone(), &ticket, "slow-node")
        .await
        .unwrap();
    let r2 = serve(&node, &ep2);

    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while !mesh
        .code_hosts()
        .iter()
        .any(|s| s.device.id == node.me() && s.online)
    {
        assert!(tokio::time::Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    let mut folders = std::collections::BTreeMap::new();
    folders.insert(mesh.me(), "/srv/widget".to_owned());
    folders.insert(node.me(), "/srv/widget".to_owned());
    mesh.set(
        Key::Sync("acme/widget".into()),
        Some(
            SyncSpec {
                folders,
                ..SyncSpec::default()
            }
            .to_bytes()
            .unwrap(),
        ),
    )
    .await
    .unwrap();

    let (release, opened) = tokio::sync::watch::channel(false);
    let sized = |files, bytes| ProjectStatus {
        files,
        bytes,
        ..widget(files)
    };
    let machines = Arc::new(Machines::new(
        mesh.me(),
        Some(answering(vec![sized(10, 512)])),
    ));
    machines.set_opened(MeshState::Open(mesh.clone()));
    machines.stand_in(
        node.me(),
        StandIn::NodeWhen(answering(vec![sized(4, 128)]), opened),
    );
    let session = Session::new(machines, Options::default()).unwrap();
    let desk = session.desk().await.unwrap();

    let started = Instant::now();
    let initial = desk.sync().await;
    assert!(
        started.elapsed() < Duration::from_millis(500),
        "sync returned without waiting on the stalled node"
    );
    let fast_row = initial[0]
        .machines
        .iter()
        .find(|m| m.machine_id == mesh.me().to_string())
        .unwrap();
    assert_eq!(fast_row.status, SyncStatus::InSync {});

    let slow_row = initial[0]
        .machines
        .iter()
        .find(|m| m.machine_id == node.me().to_string())
        .unwrap();
    assert_eq!(slow_row.status, SyncStatus::Failed {});

    // Nothing is kept from the unanswered read: the next one shows what the node says now.
    release.send(true).unwrap();
    let updated = desk.sync().await;
    let slow_row = updated[0]
        .machines
        .iter()
        .find(|m| m.machine_id == node.me().to_string())
        .unwrap();
    assert_eq!(slow_row.status, SyncStatus::InSync {});
    assert_eq!(slow_row.files, 4);

    let _ = r2.shutdown().await;
    let _ = r1.shutdown().await;
    drop(node);
    ep1.close().await;
    ep2.close().await;
}

fn widget(files: u64) -> ProjectStatus {
    ProjectStatus {
        name: "acme/widget".into(),
        folder: "/srv/widget".into(),
        state: State::Idle,
        paused: false,
        git_only: false,
        files,
        bytes: 0,
        pending: 0,
        waiting: Vec::new(),
        progress: None,
        conflicts: Vec::new(),
        rescan_secs: None,
        hint: None,
        error: None,
    }
}

fn status_of(files: u64) -> beep_proto::node::events::Value {
    beep_proto::node::events::Value::Sync(Box::new(Reply::Status {
        projects: vec![widget(files)],
    }))
}

/// This machine's own node, which answers every direct status call with a file count that
/// grows by one each time, or never when `hangs`.
fn counting(mesh: &Mesh, hangs: bool) -> (Arc<Machines>, Arc<std::sync::atomic::AtomicU64>) {
    let calls = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let counted = calls.clone();
    let node: beep_mesh::SessionFn = Arc::new(move |mut tx, mut rx| {
        let calls = counted.clone();
        Box::pin(async move {
            let _ = rx.recv::<beep_proto::node::Open>().await;
            let _ = tx.send(&beep_proto::node::Opened::Ready {}).await;
            let _ = rx.recv::<Request>().await;
            if hangs {
                std::future::pending::<()>().await;
            }

            let files = calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
            let _ = tx
                .send(&Reply::Status {
                    projects: vec![widget(files)],
                })
                .await;
        })
    });
    let machines = Arc::new(Machines::new(mesh.me(), Some(node)));
    machines.set_opened(MeshState::Open(mesh.clone()));

    (machines, calls)
}

#[tokio::test]
async fn a_node_without_the_topic_answers_from_its_last_call_and_redraws_when_it_changes() {
    use crate::{
        changed::Changed,
        session::{Options, Session},
    };
    use std::sync::atomic::Ordering;

    let dir = TempDir::new().unwrap();
    let endpoint = Endpoint::builder(presets::Minimal)
        .transport_config(transport_config())
        .bind()
        .await
        .unwrap();
    let mesh = Mesh::create(dir.path(), endpoint.clone(), "desktop")
        .await
        .unwrap();
    let mut folders = std::collections::BTreeMap::new();
    folders.insert(mesh.me(), "/srv/widget".to_owned());
    mesh.set(
        Key::Sync("acme/widget".into()),
        Some(
            SyncSpec {
                folders,
                ..SyncSpec::default()
            }
            .to_bytes()
            .unwrap(),
        ),
    )
    .await
    .unwrap();

    let (machines, calls) = counting(&mesh, false);
    let session = Session::new(machines, Options::default()).unwrap();
    let mut heard = session.changes().expect("the feed is taken once");
    let desk = session.desk().await.unwrap();
    let files = |sync: &[ProjectSync]| sync[0].machines[0].files;

    assert_eq!(files(&desk.sync().await), 1);
    assert_eq!(files(&desk.sync().await), 1);
    assert_eq!(calls.load(Ordering::SeqCst), 1, "a fresh answer is kept");

    // Past the freshness, a read still answers at once from the kept copy.
    tokio::time::pause();
    tokio::time::advance(SYNC_FRESH + Duration::from_secs(1)).await;
    assert_eq!(files(&desk.sync().await), 1);
    let said = tokio::time::timeout(Duration::from_secs(1), heard.recv()).await;
    assert_eq!(said, Ok(Some(Changed::Sync)));
    assert_eq!(files(&desk.sync().await), 2);

    endpoint.close().await;
}

#[tokio::test]
async fn a_node_that_pushes_is_not_answered_from_an_old_direct_call() {
    use crate::session::{Options, Session};

    let dir = TempDir::new().unwrap();
    let endpoint = Endpoint::builder(presets::Minimal)
        .transport_config(transport_config())
        .bind()
        .await
        .unwrap();
    let mesh = Mesh::create(dir.path(), endpoint.clone(), "desktop")
        .await
        .unwrap();
    let mut folders = std::collections::BTreeMap::new();
    folders.insert(mesh.me(), "/srv/widget".to_owned());
    mesh.set(
        Key::Sync("acme/widget".into()),
        Some(
            SyncSpec {
                folders,
                ..SyncSpec::default()
            }
            .to_bytes()
            .unwrap(),
        ),
    )
    .await
    .unwrap();

    let (machines, _calls) = counting(&mesh, false);
    let pushed = machines.snapshots().clone();
    let session = Session::new(machines, Options::default()).unwrap();
    let desk = session.desk().await.unwrap();
    let files = |sync: &[ProjectSync]| sync[0].machines[0].files;

    // One slow stretch with no subscription keeps a direct answer.
    assert_eq!(files(&desk.sync().await), 1);

    let generation = pushed.open_stream(mesh.me(), Read::SyncList.topic().unwrap());
    pushed.push(mesh.me(), Read::SyncList, status_of(50));
    assert_eq!(files(&desk.sync().await), 50);

    // The subscription drops: the old direct answer is gone, so this asks again, and a node
    // that pushed before has no direct answer kept: each read asks.
    pushed.end(mesh.me(), generation);
    assert_eq!(files(&desk.sync().await), 2);
    assert_eq!(files(&desk.sync().await), 3);

    endpoint.close().await;
}

/// A session over one project that this machine's folder holds, read from `counting`.
async fn widget_desk(
    dir: &TempDir,
    endpoint: &Endpoint,
    hangs: bool,
) -> (
    crate::Desk,
    Mesh,
    Arc<Machines>,
    Arc<std::sync::atomic::AtomicU64>,
) {
    use crate::session::{Options, Session};

    let mesh = Mesh::create(dir.path(), endpoint.clone(), "desktop")
        .await
        .unwrap();
    let mut folders = std::collections::BTreeMap::new();
    folders.insert(mesh.me(), "/srv/widget".to_owned());
    mesh.set(
        Key::Sync("acme/widget".into()),
        Some(
            SyncSpec {
                folders,
                ..SyncSpec::default()
            }
            .to_bytes()
            .unwrap(),
        ),
    )
    .await
    .unwrap();

    let (machines, calls) = counting(&mesh, hangs);
    let session = Session::new(machines.clone(), Options::default()).unwrap();
    (session.desk().await.unwrap(), mesh, machines, calls)
}

#[tokio::test]
async fn a_pushing_node_that_stalls_is_read_within_one_wait() {
    let dir = TempDir::new().unwrap();
    let endpoint = Endpoint::builder(presets::Minimal)
        .transport_config(transport_config())
        .bind()
        .await
        .unwrap();
    let (desk, mesh, machines, _calls) = widget_desk(&dir, &endpoint, true).await;
    let id = mesh.me();
    let snapshots = machines.snapshots();
    let topic = Read::SyncList.topic().unwrap();
    snapshots.open_stream(id, topic.clone());
    snapshots.push(id, Read::SyncList, status_of(7));
    let files = |sync: &[ProjectSync]| sync[0].machines[0].files;
    assert_eq!(files(&desk.sync().await), 7);

    // The subscription goes quiet and the direct call never answers: a read is cut at one wait.
    snapshots.connected(id, topic, uuid::Uuid::new_v4());
    tokio::time::pause();
    let before = tokio::time::Instant::now();
    desk.node_statuses(id).await;
    assert!(
        before.elapsed() < SYNC_FIRST_WAIT + Duration::from_millis(10),
        "{:?}",
        before.elapsed()
    );

    endpoint.close().await;
}

#[tokio::test]
async fn a_node_that_refuses_the_topic_after_pushing_is_read_directly_again() {
    let dir = TempDir::new().unwrap();
    let endpoint = Endpoint::builder(presets::Minimal)
        .transport_config(transport_config())
        .bind()
        .await
        .unwrap();
    let (desk, mesh, machines, calls) = widget_desk(&dir, &endpoint, false).await;
    let id = mesh.me();
    let snapshots = machines.snapshots().clone();
    snapshots.open_stream(id, Read::SyncList.topic().unwrap());
    snapshots.push(id, Read::SyncList, status_of(7));
    let files = |sync: &[ProjectSync]| sync[0].machines[0].files;
    assert_eq!(files(&desk.sync().await), 7);

    // A node that pushed and now refuses is read directly, and a call's answer is kept.
    let refusal = "no state has been published for this topic";
    snapshots.refuse(id, Read::SyncList, refusal);
    let first = snapshots.queries(id)[0].id;
    let (synced, ()) = tokio::join!(desk.sync(), async {
        let mut changes = snapshots.changes();

        // The read asks again, and the node refuses it.
        while snapshots.queries(id).iter().all(|query| query.id == first) {
            changes.changed().await.unwrap();
        }
        snapshots.refuse(id, Read::SyncList, refusal);
    });
    assert_eq!(files(&synced), 1);
    assert_eq!(files(&desk.sync().await), 1, "the direct answer is kept");
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);

    endpoint.close().await;
}
