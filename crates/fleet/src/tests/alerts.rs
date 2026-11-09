use super::*;
use async_trait::async_trait;
use beep_mesh::{DeviceKind, JOIN_ALPN, PEER_ALPN, PeerStream, transport_config};
use beep_proto::node::{
    Open, Opened,
    agent::{Alert, AlertKind, OptionKind, Question},
    link,
};
use iroh::{Endpoint, SecretKey, endpoint::presets, protocol::Router};
use tempfile::TempDir;
use tokio::sync::Notify;

const LIMIT: Duration = Duration::from_secs(60);

fn key(question: Option<u32>, machine_name: &str) -> AgentNoticeKey {
    AgentNoticeKey {
        machine_id: "ab12".into(),
        agent: "claude".into(),
        session: "a3f9c1".into(),
        question,
        machine_name: machine_name.into(),
    }
}

#[test]
fn a_stream_that_is_accepted_and_closed_keeps_backing_off() {
    let secs = Duration::from_secs;
    assert!(!held(false, secs(1)));
    assert!(!held(false, HELD - secs(1)));
    assert!(held(true, secs(0)));
    assert!(held(false, HELD));
}

#[test]
fn same_alert_twice_is_delivered_once() {
    let mut seen = Seen::default();
    let start = Instant::now();
    let after = |secs| start + Duration::from_secs(secs);
    assert!(seen.fresh(&key(Some(3), "Studio"), start));
    assert!(!seen.fresh(&key(Some(3), "Studio"), after(1)));
    // Another question of the session is news, and so is a turn that ended, each time.
    assert!(seen.fresh(&key(Some(4), "Studio"), after(2)));
    assert!(seen.fresh(&key(None, "Studio"), after(3)));
    assert!(seen.fresh(&key(None, "Studio"), after(4)));
    // Long after, the same question is a new one.
    assert!(seen.fresh(
        &key(Some(3), "Studio"),
        start + TWICE + Duration::from_secs(1)
    ));
}

struct Dev {
    mesh: Mesh,
    id: EndpointId,
    _router: Router,
    _dir: TempDir,
}

async fn dev(from: Option<&Dev>, kind: DeviceKind, name: &str) -> Dev {
    join(from, Some(kind), name).await
}

/// A phone: joins with an observer ticket, so it is a member but not a code host.
async fn phone(from: &Dev, name: &str) -> Dev {
    join(Some(from), None, name).await
}

async fn join(from: Option<&Dev>, kind: Option<DeviceKind>, name: &str) -> Dev {
    let key = SecretKey::generate();
    let dir = TempDir::new().expect("a temp dir");
    let endpoint = Endpoint::builder(presets::Minimal)
        .secret_key(key.clone())
        .transport_config(transport_config())
        .bind()
        .await
        .expect("the endpoint binds");
    let mesh = match from {
        None => Mesh::create(dir.path(), endpoint.clone(), name)
            .await
            .expect("the mesh is created"),
        Some(from) => {
            let ttl = Duration::from_secs(3600);
            let ticket = match kind {
                Some(kind) => from.mesh.invite(kind, ttl).await,
                None => from.mesh.invite_observer(ttl).await,
            }
            .expect("a ticket");
            Mesh::join(dir.path(), endpoint.clone(), &ticket, name)
                .await
                .expect("the device joins")
        }
    };
    let (peer, join) = mesh.protocols();
    Dev {
        id: key.public(),
        _router: Router::builder(endpoint)
            .accept(PEER_ALPN, peer)
            .accept(JOIN_ALPN, join)
            .spawn(),
        mesh,
        _dir: dir,
    }
}

struct Test(Mesh);

#[async_trait]
impl Peers for Test {
    async fn mesh(&self) -> Result<Mesh> {
        Ok(self.0.clone())
    }
    fn me(&self) -> EndpointId {
        self.0.me()
    }
    async fn client(&self, machine: EndpointId) -> Result<PeerClient> {
        beep_mesh::with_deadline(machine, &self.0)
            .await
            .map_err(|e| FleetError::Offline(e.to_string()))
    }
}

fn asking() -> Alert {
    Alert {
        session: "s1".into(),
        agent: "claude".into(),
        title: None,
        cwd: "/srv/project".into(),
        kind: AlertKind::Asks {
            question: Question {
                id: 3,
                text: "Run cargo test".into(),
                detail: None,
                options: vec![
                    AnswerOption {
                        id: "allow".into(),
                        name: "Allow".into(),
                        kind: OptionKind::AllowOnce,
                    },
                    AnswerOption {
                        id: "deny".into(),
                        name: "Deny".into(),
                        kind: OptionKind::RejectOnce,
                    },
                ],
            },
        },
    }
}

/// A node that tells `alert` to whoever follows its alerts and holds every follow open, and
/// says when a followed alerts stream ends.
fn node_of(dev: &Dev, alert: Alert, closed: Arc<Notify>) {
    let (peer, _) = dev.mesh.protocols();
    peer.on_stream(Arc::new(move |stream: PeerStream| {
        let alert = alert.clone();
        let closed = closed.clone();
        Box::pin(async move {
            let (mut tx, mut rx) = link(stream.send, stream.recv);
            let Ok(Some(open)) = rx.recv::<Open>().await else {
                return;
            };

            if open.protocol != SESSIONS_PROTOCOL || tx.send(&Opened::Ready {}).await.is_err() {
                return;
            }

            let alerts = match rx.recv::<SessionsRequest>().await {
                Ok(Some(SessionsRequest::FollowAlerts {})) => true,
                Ok(Some(SessionsRequest::FollowRuns {})) => false,
                _ => return,
            };

            if alerts && tx.send(&SessionsReply::Alert { alert }).await.is_err() {
                return;
            }
            // Held open until the follower goes.
            while let Ok(Some(_)) = rx.recv::<Open>().await {}
            if alerts {
                closed.notify_one();
            }
        })
    }));
}

async fn two() -> (Dev, Dev) {
    let desktop = dev(None, DeviceKind::Desktop, "Studio").await;
    let node = dev(Some(&desktop), DeviceKind::Node, "build-box").await;
    let mut updates = desktop.mesh.updates();
    timeout(LIMIT, async {
        loop {
            updates.borrow_and_update();

            if desktop
                .mesh
                .devices()
                .iter()
                .any(|d| d.device.id == node.id && d.online)
            {
                return;
            }

            updates.changed().await.expect("the mesh is alive");
        }
    })
    .await
    .expect("the node comes online");
    (desktop, node)
}

#[tokio::test]
async fn a_machines_question_arrives_with_its_options() {
    let (desktop, node) = two().await;
    node_of(&node, asking(), Arc::new(Notify::new()));
    let (following, mut heard) = follow(Arc::new(Test(desktop.mesh.clone())), desktop.mesh.clone());
    let following = tokio::spawn(following);
    let (key, notice, options) = timeout(LIMIT, async {
        loop {
            if let Heard::Notice {
                key,
                notice,
                options,
            } = heard.recv().await.expect("the feed follows")
            {
                return (key, notice, options);
            }
        }
    })
    .await
    .expect("a notice came in time");
    assert_eq!(key.question, Some(3));
    assert_eq!(key.machine_id, node.id.to_string());
    assert_eq!(key.machine_name, "build-box");
    assert_eq!(notice.title, "Claude Code on build-box asks");
    assert_eq!(options.len(), 2);
    drop(heard);
    timeout(LIMIT, following)
        .await
        .expect("following ends once nobody listens")
        .expect("the task ends cleanly");
}

#[tokio::test]
async fn machine_leaving_stops_its_stream() {
    let (desktop, node) = two().await;
    let closed = Arc::new(Notify::new());
    node_of(&node, asking(), closed.clone());
    let (following, mut heard) = follow(Arc::new(Test(desktop.mesh.clone())), desktop.mesh.clone());
    let _following = tokio::spawn(following);
    timeout(LIMIT, async {
        while !matches!(heard.recv().await, Some(Heard::Notice { .. })) {}
    })
    .await
    .expect("the stream is followed");
    desktop
        .mesh
        .remove(node.id)
        .await
        .expect("the node is removed");
    timeout(LIMIT, closed.notified())
        .await
        .expect("the stream to the removed machine ends");
}

#[tokio::test]
async fn a_paired_phone_gets_no_stream() {
    let (desktop, node) = two().await;
    let phone = phone(&desktop, "pocket").await;
    let phone_asked = Arc::new(Notify::new());
    let asked = phone_asked.clone();
    let (peer, _) = phone.mesh.protocols();
    peer.on_stream(Arc::new(move |_stream: PeerStream| {
        let asked = asked.clone();
        Box::pin(async move { asked.notify_one() })
    }));
    node_of(&node, asking(), Arc::new(Notify::new()));
    let (following, mut heard) = follow(Arc::new(Test(desktop.mesh.clone())), desktop.mesh.clone());
    let _following = tokio::spawn(following);
    // The node's notice proves the pass over the members ran.
    timeout(LIMIT, async {
        while !matches!(heard.recv().await, Some(Heard::Notice { .. })) {}
    })
    .await
    .expect("the node is followed");
    assert!(
        desktop
            .mesh
            .devices()
            .iter()
            .any(|d| d.device.id == phone.id),
        "the phone is a member"
    );
    assert!(
        timeout(Duration::from_secs(2), phone_asked.notified())
            .await
            .is_err(),
        "no stream is opened to the phone"
    );
}
