use super::*;
use crate::Peers;
use async_trait::async_trait;
use beep_mesh::{Mesh, PeerClient, transport_config};
use beep_proto::node::mesh_state::StateValue;
use beep_proto::{
    ModelId,
    node::harness::{AutoCompact, HarnessDriver},
};
use iroh::{Endpoint, endpoint::presets};
use std::sync::Arc;
use tempfile::TempDir;

struct TestPeers(Mesh);

#[async_trait]
impl Peers for TestPeers {
    async fn mesh(&self) -> Result<Mesh> {
        Ok(self.0.clone())
    }

    fn me(&self) -> EndpointId {
        self.0.me()
    }

    async fn client(&self, _machine: EndpointId) -> Result<PeerClient> {
        Err(FleetError::Offline("offline".into()))
    }
}

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
    let desk = Desk::open(Arc::new(TestPeers(mesh.clone()))).await.unwrap();
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
    let desk = Desk::open(Arc::new(TestPeers(mesh))).await.unwrap();
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
    let desk = Desk::open(Arc::new(TestPeers(mesh))).await.unwrap();

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

struct DegradedSyncPeers {
    mesh: Mesh,
    slow: EndpointId,
    release: Arc<tokio::sync::Notify>,
}

#[async_trait]
impl Peers for DegradedSyncPeers {
    async fn mesh(&self) -> Result<Mesh> {
        Ok(self.mesh.clone())
    }

    fn me(&self) -> EndpointId {
        self.mesh.me()
    }

    async fn client(&self, machine: EndpointId) -> Result<PeerClient> {
        if machine == self.slow {
            self.release.notified().await;
            let slow_projects = vec![ProjectStatus {
                name: "acme/widget".into(),
                folder: "/srv/widget".into(),
                state: State::Idle,
                paused: false,
                git_only: false,
                files: 4,
                bytes: 128,
                pending: 0,
                waiting: Vec::new(),
                progress: None,
                conflicts: Vec::new(),
                rescan_secs: None,
                hint: None,
                error: None,
            }];
            return Ok(PeerClient::local(Arc::new(move |mut tx, mut rx| {
                let slow_projects = slow_projects.clone();
                Box::pin(async move {
                    let _ = rx.recv::<beep_proto::node::Open>().await;
                    let _ = tx.send(&beep_proto::node::Opened::Ready {}).await;
                    let _ = rx.recv::<Request>().await;
                    let _ = tx
                        .send(&Reply::Status {
                            projects: slow_projects,
                        })
                        .await;
                })
            })));
        }

        Ok(PeerClient::local(Arc::new(|mut tx, mut rx| {
            Box::pin(async move {
                let _ = rx.recv::<beep_proto::node::Open>().await;
                let _ = tx.send(&beep_proto::node::Opened::Ready {}).await;
                let _ = rx.recv::<Request>().await;
                let _ = tx
                    .send(&Reply::Status {
                        projects: vec![ProjectStatus {
                            name: "acme/widget".into(),
                            folder: "/srv/widget".into(),
                            state: State::Idle,
                            paused: false,
                            git_only: false,
                            files: 10,
                            bytes: 512,
                            pending: 0,
                            waiting: Vec::new(),
                            progress: None,
                            conflicts: Vec::new(),
                            rescan_secs: None,
                            hint: None,
                            error: None,
                        }],
                    })
                    .await;
            })
        })))
    }
}

#[tokio::test]
async fn degraded_node_does_not_stall_sync_and_updates_in_background() {
    use crate::session::{Changed, Options, Session};
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

    let release = Arc::new(tokio::sync::Notify::new());
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let peers = Arc::new(DegradedSyncPeers {
        mesh: mesh.clone(),
        slow: node.me(),
        release: release.clone(),
    });
    let session = Session::new(peers, Options::default(), move |c| {
        let _ = tx.send(c);
    })
    .unwrap();
    let desk = session.desk().await.unwrap();

    let started = Instant::now();
    let initial = desk.sync().await;
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "sync returned without waiting 30s on the stalled node"
    );
    let fast_row = initial[0]
        .machines
        .iter()
        .find(|m| m.machine_id == mesh.me().to_string())
        .unwrap();
    assert_eq!(fast_row.status, SyncStatus::InSync {});

    // Subsequent read while the node is still degraded returns in ~0ms from cache.
    let cached_start = Instant::now();
    let _ = desk.sync().await;
    assert!(cached_start.elapsed() < Duration::from_millis(100));

    // Once the degraded node answers, the background actor updates cache and emits Changed::Sync.
    release.notify_waiters();
    let heard = tokio::time::timeout(Duration::from_secs(2), rx.recv())
        .await
        .expect("heard sync update");
    assert_eq!(heard, Some(Changed::Sync));

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
