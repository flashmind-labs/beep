use super::*;
use beep_mesh::{DeviceKind, JOIN_ALPN, PEER_ALPN, transport_config};
use beep_proto::node::{
    Opened,
    agent::{Alert, AlertKind, OptionKind, Question},
    commands, link,
};
use beep_proto::watched;
use iroh::{
    Endpoint, SecretKey,
    endpoint::{Connection, RecvStream, SendStream, presets},
    protocol::{AcceptError, ProtocolHandler, Router},
};
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
    key: SecretKey,
    router: Router,
    dir: TempDir,
}

async fn dev(from: Option<&Dev>, kind: DeviceKind, name: &str, serves: Serves) -> Dev {
    join(from, Some(kind), name, serves).await
}

/// A phone: joins with an observer ticket, so it is a member but not a code host.
async fn phone(from: &Dev, name: &str, asked: Arc<Notify>) -> Dev {
    join(Some(from), None, name, Serves::Asked(asked)).await
}

async fn join(from: Option<&Dev>, kind: Option<DeviceKind>, name: &str, serves: Serves) -> Dev {
    let key = SecretKey::generate();
    let dir = TempDir::new().expect("a temp dir");
    let endpoint = bind(&key).await;
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
    serve(key, dir, endpoint, mesh, serves)
}

fn serve(key: SecretKey, dir: TempDir, endpoint: Endpoint, mesh: Mesh, serves: Serves) -> Dev {
    let (peer, join) = mesh.protocols();
    Dev {
        id: key.public(),
        key,
        router: Router::builder(endpoint)
            .accept(PEER_ALPN, peer)
            .accept(JOIN_ALPN, join)
            .accept(commands::ALPN, serves)
            .spawn(),
        mesh,
        dir,
    }
}

async fn bind(key: &SecretKey) -> Endpoint {
    Endpoint::builder(presets::Minimal)
        .secret_key(key.clone())
        .transport_config(transport_config())
        .bind()
        .await
        .expect("the endpoint binds")
}

/// Stops `dev`, keeping what it needs to start again.
async fn stop(dev: Dev) -> (SecretKey, TempDir) {
    let Dev {
        mesh,
        key,
        router,
        dir,
        ..
    } = dev;
    router.shutdown().await.expect("the router stops");
    drop(mesh);
    (key, dir)
}

/// Starts a stopped device again from its folder, as a restart does.
async fn reopen(key: SecretKey, dir: TempDir) -> Dev {
    let endpoint = bind(&key).await;
    let mesh = Mesh::open(dir.path(), endpoint.clone())
        .await
        .expect("the mesh opens")
        .expect("the folder is in a mesh");
    serve(
        key,
        dir,
        endpoint,
        mesh,
        Serves::Asked(Arc::new(Notify::new())),
    )
}

/// Waits until `mesh` sees `id` as `online`.
async fn until_online(mesh: &Mesh, id: EndpointId, online: bool) {
    let mut updates = mesh.updates();
    timeout(LIMIT, async {
        loop {
            watched::read_and_update(&mut updates, |_| ());

            if mesh
                .devices()
                .iter()
                .any(|d| d.device.id == id && d.online == online)
            {
                return;
            }

            updates.changed().await.expect("the mesh is alive");
        }
    })
    .await
    .expect("the machine's presence changed in time");
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

/// What a device does with the session streams a peer opens on the commands ALPN.
#[derive(Clone, Debug)]
enum Serves {
    /// A node: tells `alert` to whoever follows its alerts, holds every follow open, and says
    /// when a followed alerts stream ends.
    Alert { alert: Alert, closed: Arc<Notify> },
    /// Says that a stream was opened at all.
    Asked(Arc<Notify>),
}

impl ProtocolHandler for Serves {
    async fn accept(&self, connection: Connection) -> Result<(), AcceptError> {
        while let Ok((send, recv)) = connection.accept_bi().await {
            tokio::spawn(self.clone().serve(send, recv));
        }

        Ok(())
    }
}

impl Serves {
    async fn serve(self, send: SendStream, recv: RecvStream) {
        let (alert, closed) = match self {
            Serves::Alert { alert, closed } => (alert, closed),
            Serves::Asked(asked) => return asked.notify_one(),
        };
        let (mut tx, mut rx) = link(send, recv);
        let Ok(Some(commands::Open::Session { protocol, .. })) = rx.recv().await else {
            return;
        };

        if protocol != SESSIONS_PROTOCOL || tx.send(&Opened::Ready {}).await.is_err() {
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
        while let Ok(Some(_)) = rx.recv::<commands::Open>().await {}

        if alerts {
            closed.notify_one();
        }
    }
}

/// A desktop and a node that asks `asking()`, telling `closed` when a followed alerts stream ends.
async fn two(closed: Arc<Notify>) -> (Dev, Dev) {
    let unused = Serves::Asked(Arc::new(Notify::new()));
    let desktop = dev(None, DeviceKind::Desktop, "Studio", unused).await;
    let node = Serves::Alert {
        alert: asking(),
        closed,
    };
    let node = dev(Some(&desktop), DeviceKind::Node, "build-box", node).await;
    let mut updates = desktop.mesh.updates();
    timeout(LIMIT, async {
        loop {
            watched::read_and_update(&mut updates, |_| ());

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
    let (desktop, node) = two(Arc::new(Notify::new())).await;
    let (following, mut heard) = follow(
        Machines::for_tests(desktop.mesh.clone()),
        desktop.mesh.clone(),
    );
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
    assert_eq!(notice.title, "project");
    assert_eq!(options.len(), 2);
    drop(heard);
    timeout(LIMIT, following)
        .await
        .expect("following ends once nobody listens")
        .expect("the task ends cleanly");
}

#[tokio::test]
async fn a_notice_carries_the_title_the_user_gave_its_session() {
    let (desktop, node) = two(Arc::new(Notify::new())).await;
    session_titles::save(
        &desktop.mesh,
        &node.id.to_string(),
        "claude",
        &"s1".into(),
        "Renamed",
    )
    .await
    .expect("the title is saved");
    let (following, mut heard) = follow(
        Machines::for_tests(desktop.mesh.clone()),
        desktop.mesh.clone(),
    );
    let _following = tokio::spawn(following);
    let notice = timeout(LIMIT, async {
        loop {
            if let Heard::Notice { notice, .. } = heard.recv().await.expect("the feed follows") {
                return notice;
            }
        }
    })
    .await
    .expect("a notice came in time");
    assert_eq!(notice.title, "Renamed");
}

#[tokio::test]
async fn machine_leaving_stops_its_stream() {
    let closed = Arc::new(Notify::new());
    let (desktop, node) = two(closed.clone()).await;
    let (following, mut heard) = follow(
        Machines::for_tests(desktop.mesh.clone()),
        desktop.mesh.clone(),
    );
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
    let (desktop, _node) = two(Arc::new(Notify::new())).await;
    let phone_asked = Arc::new(Notify::new());
    let phone = phone(&desktop, "pocket", phone_asked.clone()).await;
    let (following, mut heard) = follow(
        Machines::for_tests(desktop.mesh.clone()),
        desktop.mesh.clone(),
    );
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

#[tokio::test]
async fn an_offline_machine_is_not_followed_until_it_is_back() {
    let (desktop, node) = two(Arc::new(Notify::new())).await;
    let (started, mut starts) = mpsc::unbounded_channel();
    let mesh = desktop.mesh.clone();
    let _watching = tokio::spawn(async move {
        each_machine(&mesh, |id| {
            started.send(id).ok();
            tokio::spawn(std::future::pending())
        })
        .await;
    });
    let node_id = node.id;
    timeout(LIMIT, async {
        while starts.recv().await != Some(node_id) {}
    })
    .await
    .expect("the online node is followed");

    let (key, dir) = stop(node).await;
    until_online(&desktop.mesh, node_id, false).await;
    let mut again = Vec::new();

    while let Ok(id) = starts.try_recv() {
        again.push(id);
    }

    assert!(!again.contains(&node_id), "no task while it is offline");

    let _node = reopen(key, dir).await;
    until_online(&desktop.mesh, node_id, true).await;
    timeout(LIMIT, async {
        while starts.recv().await != Some(node_id) {}
    })
    .await
    .expect("the node is followed again once back");
}

#[tokio::test]
async fn a_machine_that_links_again_ends_the_wait_at_once() {
    let (desktop, node) = two(Arc::new(Notify::new())).await;
    let node_id = node.id;
    let link = link_of(&desktop.mesh, node_id);
    assert!(link.is_some(), "the node is linked");

    // Nothing changed: the whole pause runs.
    assert!(!relinked(&desktop.mesh, node_id, link, Duration::from_millis(50)).await);

    let (key, dir) = stop(node).await;
    until_online(&desktop.mesh, node_id, false).await;
    let mesh = desktop.mesh.clone();
    let waiting =
        tokio::spawn(async move { relinked(&mesh, node_id, link, Duration::from_secs(60)).await });
    let _node = reopen(key, dir).await;

    let relinked = timeout(Duration::from_secs(20), waiting)
        .await
        .expect("the wait ended well before the pause")
        .expect("the wait ran");
    assert!(relinked);
}

#[tokio::test]
async fn a_notice_names_the_project_its_session_is_in() {
    use crate::records::{Place, ProjectRecord, place_key, project_key, write};

    let unused = Serves::Asked(Arc::new(Notify::new()));
    let desktop = dev(None, DeviceKind::Desktop, "Studio", unused).await;
    let mesh = &desktop.mesh;
    let other = SecretKey::generate().public();
    let record = ProjectRecord {
        url: "https://example.com/org/web".into(),
        syncthing_files: None,
        git_auto_recovery: false,
        ..ProjectRecord::default()
    };
    let place = |folder: &str| Place {
        folder: folder.into(),
        imported_ms: 0,
        fork: None,
    };
    for name in ["org/web", "org/docs"] {
        write(mesh, project_key(name), Some(&record))
            .await
            .expect("written");
    }
    write(
        mesh,
        place_key("org/web", desktop.id),
        Some(&place("/srv/web")),
    )
    .await
    .expect("written");
    write(
        mesh,
        place_key("org/docs", other),
        Some(&place("/srv/web/docs")),
    )
    .await
    .expect("written");

    let at = |cwd| project_at(mesh, desktop.id, cwd);
    assert_eq!(at("/srv/web/.worktrees/fix").as_deref(), Some("org/web"));
    assert_eq!(at("/srv/web/docs").as_deref(), Some("org/web"));
    assert_eq!(at("/srv/website"), None);
}
