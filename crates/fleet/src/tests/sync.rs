use super::*;
use crate::Peers;
use crate::direct_syncs::SYNC_FRESH;
use async_trait::async_trait;
use beep_mesh::{Mesh, PeerClient, transport_config};
use beep_proto::node::mesh_state::StateValue;
use beep_proto::{
    ModelId,
    node::harness::{AutoCompact, HarnessDriver},
};
use iroh::{Endpoint, endpoint::presets};
use std::{sync::Arc, time::Instant};
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
    release: tokio::sync::watch::Receiver<bool>,
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
            let _ = beep_proto::watched::wait_for(&mut self.release.clone(), |open| *open).await;
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
    let peers = Arc::new(DegradedSyncPeers {
        mesh: mesh.clone(),
        slow: node.me(),
        release: opened,
    });
    let session = Session::new(peers, Options::default(), |_| {}).unwrap();
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

/// Answers every direct status call with a file count that grows by one each time.
struct CountingPeers {
    mesh: Mesh,
    calls: Arc<std::sync::atomic::AtomicU64>,
    /// The file count the node pushes on its subscription; 0 when it pushes nothing.
    pushed: Arc<std::sync::atomic::AtomicU64>,
}

struct Pushed(Arc<std::sync::atomic::AtomicU64>);

impl beep_mesh::SnapshotReader for Pushed {
    fn snapshot(&self, read: beep_proto::node::events::Read) -> beep_mesh::SnapshotFuture<'_> {
        let files = self.0.load(std::sync::atomic::Ordering::SeqCst);
        Box::pin(async move {
            if files == 0 || read != beep_proto::node::events::Read::SyncList {
                return None;
            }

            Some(Ok(beep_proto::node::events::Value::Sync(Box::new(
                Reply::Status {
                    projects: vec![widget(files)],
                },
            ))))
        })
    }
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

#[async_trait]
impl Peers for CountingPeers {
    async fn mesh(&self) -> Result<Mesh> {
        Ok(self.mesh.clone())
    }

    fn me(&self) -> EndpointId {
        self.mesh.me()
    }

    async fn client(&self, _machine: EndpointId) -> Result<PeerClient> {
        let calls = self.calls.clone();
        let reader = Arc::new(Pushed(self.pushed.clone()));
        Ok(PeerClient::local(Arc::new(move |mut tx, mut rx| {
            let calls = calls.clone();
            Box::pin(async move {
                let _ = rx.recv::<beep_proto::node::Open>().await;
                let _ = tx.send(&beep_proto::node::Opened::Ready {}).await;
                let _ = rx.recv::<Request>().await;
                let files = calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
                let _ = tx
                    .send(&Reply::Status {
                        projects: vec![widget(files)],
                    })
                    .await;
            })
        }))
        .with_snapshot_reader(reader))
    }
}

#[tokio::test]
async fn a_node_without_the_topic_answers_from_its_last_call_and_redraws_when_it_changes() {
    use crate::session::{Changed, Options, Session};
    use std::sync::atomic::{AtomicU64, Ordering};

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

    let calls = Arc::new(AtomicU64::new(0));
    let peers = Arc::new(CountingPeers {
        mesh: mesh.clone(),
        calls: calls.clone(),
        pushed: Arc::default(),
    });
    let (tx, mut heard) = tokio::sync::mpsc::unbounded_channel();
    let session = Session::new(peers, Options::default(), move |changed| {
        let _ = tx.send(changed);
    })
    .unwrap();
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
    use std::sync::atomic::{AtomicU64, Ordering};

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

    let calls = Arc::new(AtomicU64::new(0));
    let pushed = Arc::new(AtomicU64::new(0));
    let peers = Arc::new(CountingPeers {
        mesh: mesh.clone(),
        calls: calls.clone(),
        pushed: pushed.clone(),
    });
    let session = Session::new(peers, Options::default(), |_| {}).unwrap();
    let desk = session.desk().await.unwrap();
    let files = |sync: &[ProjectSync]| sync[0].machines[0].files;

    // One slow stretch with no subscription keeps a direct answer.
    assert_eq!(files(&desk.sync().await), 1);

    pushed.store(50, Ordering::SeqCst);
    assert_eq!(files(&desk.sync().await), 50);

    // The subscription drops: the old direct answer is gone, so this asks again, and a node
    // that pushed before has no direct answer kept: each read asks.
    pushed.store(0, Ordering::SeqCst);
    assert_eq!(files(&desk.sync().await), 2);
    assert_eq!(files(&desk.sync().await), 3);

    endpoint.close().await;
}

/// What a node's subscription does to a `SyncList` read.
#[derive(Clone, Copy)]
enum Subscription {
    /// Pushes this many files.
    Pushes(u64),
    /// Does not answer in time.
    Silent,
    /// Answers with an error, as a node without the topic does.
    Refuses,
}

/// A node whose subscription is set by the test, and whose client and snapshot wait `delay`.
struct SteppedPeers {
    mesh: Mesh,
    subscription: Arc<std::sync::Mutex<Subscription>>,
    delay: Arc<std::sync::Mutex<Duration>>,
    /// Direct status calls answered, each with its count as the file count.
    calls: Arc<std::sync::atomic::AtomicU64>,
    /// Whether a direct call never answers.
    hangs: bool,
}

struct SteppedReader {
    subscription: Arc<std::sync::Mutex<Subscription>>,
    delay: Arc<std::sync::Mutex<Duration>>,
}

impl beep_mesh::SnapshotReader for SteppedReader {
    fn snapshot(&self, read: beep_proto::node::events::Read) -> beep_mesh::SnapshotFuture<'_> {
        let subscription = *self.subscription.lock().unwrap();
        let delay = *self.delay.lock().unwrap();
        Box::pin(async move {
            if read != beep_proto::node::events::Read::SyncList {
                return None;
            }

            tokio::time::sleep(delay).await;
            match subscription {
                Subscription::Pushes(files) => Some(Ok(beep_proto::node::events::Value::Sync(
                    Box::new(Reply::Status {
                        projects: vec![widget(files)],
                    }),
                ))),
                Subscription::Silent => std::future::pending().await,
                Subscription::Refuses => Some(Err(beep_mesh::MeshError::Link(anyhow::anyhow!(
                    "no state has been published for this topic"
                )))),
            }
        })
    }
}

#[async_trait]
impl Peers for SteppedPeers {
    async fn mesh(&self) -> Result<Mesh> {
        Ok(self.mesh.clone())
    }

    fn me(&self) -> EndpointId {
        self.mesh.me()
    }

    async fn client(&self, _machine: EndpointId) -> Result<PeerClient> {
        let delay = *self.delay.lock().unwrap();
        tokio::time::sleep(delay).await;
        let calls = self.calls.clone();
        let hangs = self.hangs;
        let reader = Arc::new(SteppedReader {
            subscription: self.subscription.clone(),
            delay: self.delay.clone(),
        });
        Ok(PeerClient::local(Arc::new(move |mut tx, mut rx| {
            let calls = calls.clone();
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
        }))
        .with_snapshot_reader(reader))
    }
}

/// A session over one project that this machine's folder holds, read through `peers`.
async fn widget_desk(
    dir: &TempDir,
    endpoint: &Endpoint,
    peers: impl FnOnce(Mesh) -> SteppedPeers,
) -> (crate::Desk, Arc<SteppedPeers>) {
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

    let peers = Arc::new(peers(mesh));
    let session = Session::new(peers.clone(), Options::default(), |_| {}).unwrap();
    (session.desk().await.unwrap(), peers)
}

#[tokio::test]
async fn a_pushing_node_that_stalls_is_read_within_one_wait() {
    let dir = TempDir::new().unwrap();
    let endpoint = Endpoint::builder(presets::Minimal)
        .transport_config(transport_config())
        .bind()
        .await
        .unwrap();
    let (desk, peers) = widget_desk(&dir, &endpoint, |mesh| SteppedPeers {
        mesh,
        subscription: Arc::new(std::sync::Mutex::new(Subscription::Pushes(7))),
        delay: Arc::default(),
        calls: Arc::default(),
        hangs: true,
    })
    .await;
    let files = |sync: &[ProjectSync]| sync[0].machines[0].files;
    assert_eq!(files(&desk.sync().await), 7);

    // The subscription goes quiet and every step takes most of a wait: a read is cut at one.
    *peers.subscription.lock().unwrap() = Subscription::Silent;
    *peers.delay.lock().unwrap() = Duration::from_millis(300);
    tokio::time::pause();
    let before = tokio::time::Instant::now();
    desk.node_statuses(peers.mesh.me()).await;
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
    let (desk, peers) = widget_desk(&dir, &endpoint, |mesh| SteppedPeers {
        mesh,
        subscription: Arc::new(std::sync::Mutex::new(Subscription::Pushes(7))),
        delay: Arc::default(),
        calls: Arc::default(),
        hangs: false,
    })
    .await;
    let files = |sync: &[ProjectSync]| sync[0].machines[0].files;
    assert_eq!(files(&desk.sync().await), 7);

    // A node that pushed and now refuses is read directly, and a call's answer is kept.
    *peers.subscription.lock().unwrap() = Subscription::Refuses;
    assert_eq!(files(&desk.sync().await), 1);
    assert_eq!(files(&desk.sync().await), 1, "the direct answer is kept");
    assert_eq!(peers.calls.load(std::sync::atomic::Ordering::SeqCst), 1);

    endpoint.close().await;
}
