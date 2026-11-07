use super::*;
use beep_mesh::{Key, PeerStream};
use beep_proto::{
    AgentSteer,
    node::{
        Open, Opened,
        agent::{
            self, Entry, EntryKind, Event, Request, SessionInfo, SessionsReply, SessionsRequest,
            State, Status,
        },
        link,
        mesh_state::{Place, ProjectRecord, StateValue},
    },
};
use iroh_tickets::Ticket;
use std::{sync::Mutex, time::Duration};
use tempfile::TempDir;
use tokio::time::timeout;

async fn endpoint(key: &SecretKey) -> Endpoint {
    Endpoint::builder(presets::Minimal)
        .secret_key(key.clone())
        .bind()
        .await
        .unwrap()
}

fn router(endpoint: &Endpoint, mesh: &Mesh) -> Router {
    let (peer, join) = mesh.protocols();
    Router::builder(endpoint.clone())
        .accept(PEER_ALPN, peer)
        .accept(JOIN_ALPN, join)
        .spawn()
}

async fn until(mesh: &Mesh, ready: impl Fn() -> bool) {
    let mut changes = mesh.updates();
    timeout(Duration::from_secs(10), async {
        loop {
            changes.borrow_and_update();
            if ready() {
                return;
            }
            changes.changed().await.unwrap();
        }
    })
    .await
    .expect("mesh condition");
}

fn info() -> SessionInfo {
    SessionInfo {
        id: "session-one".into(),
        agent: "codex".into(),
        cwd: "/srv/project".into(),
        title: Some("Shared session".into()),
        started_at: 1,
        state: State {
            status: Status::Idle,
            ..State::default()
        },
        question: None,
        last_message: String::new(),
        turns: 0,
        context: None,
        tokens: 0,
        cost_usd: None,
        modes: vec![],
        mode: None,
        plan_ready: None,
        place: None,
        parent: None,
        last_activity: 1,
    }
}

async fn serve(stream: PeerStream, session: Arc<Mutex<SessionInfo>>) -> anyhow::Result<()> {
    let (mut tx, mut rx) = link(stream.send, stream.recv);
    let open = rx.recv::<Open>().await?.unwrap();
    tx.send(&Opened::Ready {}).await?;
    if open.protocol == beep_proto::node::status::PROTOCOL {
        use beep_proto::node::{EnvReport, StatsReport, status::Report};
        tx.send(&Report::Offers(vec![
            agent::protocol("codex"),
            agent::SESSIONS_PROTOCOL.into(),
        ]))
        .await?;
        tx.send(&Report::Stats(StatsReport {
            memory_total_mb: 4096,
            ..StatsReport::default()
        }))
        .await?;
        tx.send(&Report::Env(EnvReport {
            os: "linux".into(),
            home: "/srv".into(),
            ..EnvReport::default()
        }))
        .await?;
    } else if open.protocol == beep_proto::node::terminal::PROTOCOL {
        use beep_proto::node::{
            process::{Task, TaskId, TaskState},
            terminal::{Reply, Request},
        };
        let task = Task {
            id: TaskId(7),
            command: "shell".into(),
            state: TaskState::Running,
        };
        match rx.recv::<Request>().await?.unwrap() {
            Request::List {} => {
                tx.send(&Reply::Terminals {
                    terminals: vec![task],
                })
                .await?
            }
            Request::Stop { .. } => tx.send(&Reply::Stopped { task }).await?,
            Request::Attach { .. } => {
                tx.send(&Reply::Output {
                    at: 0,
                    data: b"ready".to_vec().into(),
                })
                .await?;
                assert!(matches!(
                    rx.recv::<Request>().await?,
                    Some(Request::Resize { .. })
                ));
                if let Some(Request::Input { data }) = rx.recv::<Request>().await? {
                    tx.send(&Reply::Output { at: 5, data }).await?;
                }
            }
            _ => anyhow::bail!("Unsupported test terminal request"),
        }
    } else if open.protocol == agent::SESSIONS_PROTOCOL {
        let reply = match rx.recv::<SessionsRequest>().await?.unwrap() {
            SessionsRequest::List {} => SessionsReply::Sessions {
                sessions: vec![session.lock().unwrap().clone()],
            },
            SessionsRequest::Transcript { .. } => SessionsReply::Transcript {
                entries: vec![Entry::new(
                    EntryKind::Agent,
                    session.lock().unwrap().last_message.clone(),
                )],
                earlier: 0,
            },
            SessionsRequest::External { .. } | SessionsRequest::InFolders { .. } => {
                SessionsReply::External { sessions: vec![] }
            }
            _ => SessionsReply::Error {
                message: "Unsupported test request".into(),
            },
        };
        tx.send(&reply).await?;
    } else if open.protocol == agent::protocol("codex") {
        assert!(matches!(
            rx.recv::<Request>().await?,
            Some(Request::Attach { .. })
        ));
        let snapshot = session.lock().unwrap().clone();
        tx.send(&Event::Attached {
            session: Box::new(snapshot),
        })
        .await?;
        if let Some(Request::Prompt { text }) = rx.recv::<Request>().await? {
            {
                let mut current = session.lock().unwrap();
                current.last_message = text;
                current.turns += 1;
            }
            tx.send(&Event::Ack { error: None }).await?;
        }
    }
    tx.finish().await?;
    Ok(())
}

async fn phone(directory: &TempDir, key: &SecretKey) -> Arc<BeepClient> {
    let client = BeepClient::new(
        "https://server.invalid".into(),
        key.to_bytes().to_vec(),
        "test".into(),
    )
    .unwrap();
    client
        .configure_workspace(
            directory.path().to_str().unwrap().into(),
            key.to_bytes().to_vec(),
        )
        .unwrap();
    client
        .workspace
        .get()
        .unwrap()
        .endpoint
        .set(endpoint(key).await)
        .unwrap();
    client
}

#[tokio::test]
async fn node_session_and_project_changes_survive_desktop_disconnect_and_phone_restart() {
    let desktop_dir = TempDir::new().unwrap();
    let desktop_key = SecretKey::generate();
    let desktop_endpoint = endpoint(&desktop_key).await;
    let desktop = Mesh::create(desktop_dir.path(), desktop_endpoint.clone(), "desktop")
        .await
        .unwrap();
    let desktop_router = router(&desktop_endpoint, &desktop);
    let node_dir = TempDir::new().unwrap();
    let node_endpoint = endpoint(&SecretKey::generate()).await;
    let ticket = desktop
        .invite(DeviceKind::Node, Duration::from_secs(60))
        .await
        .unwrap();
    let node = Mesh::join(node_dir.path(), node_endpoint.clone(), &ticket, "node")
        .await
        .unwrap();
    let session = Arc::new(Mutex::new(info()));
    let (peer, _) = node.protocols();
    peer.on_stream(Arc::new(move |stream| {
        let session = session.clone();
        Box::pin(async move {
            serve(stream, session).await.unwrap();
        })
    }));
    let node_router = router(&node_endpoint, &node);
    let project_key = Key::Project("example/project".into());
    desktop
        .set(
            project_key.clone(),
            Some(
                ProjectRecord {
                    url: "https://example.com/example/project".into(),
                    ..ProjectRecord::default()
                }
                .to_bytes()
                .unwrap(),
            ),
        )
        .await
        .unwrap();
    desktop
        .set(
            Key::Project(format!("example/project/{}", node.me())),
            Some(
                Place {
                    folder: "/srv/project".into(),
                    imported_ms: 1,
                    fork: None,
                }
                .to_bytes()
                .unwrap(),
            ),
        )
        .await
        .unwrap();

    let terminal_id = uuid::Uuid::new_v4();
    let terminal_key = Key::Terminal(terminal_id.to_string());
    desktop
        .set(
            terminal_key.clone(),
            Some(
                beep_proto::node::mesh_state::TerminalRecord {
                    project: "example/project".into(),
                    machine: node.me(),
                    task: 7,
                    pty: true,
                    name: "Shell".into(),
                    command: "shell".into(),
                    cwd: "/srv/project".into(),
                    started_at: 1,
                }
                .to_bytes()
                .unwrap(),
            ),
        )
        .await
        .unwrap();
    let phone_dir = TempDir::new().unwrap();
    let key = SecretKey::generate();
    let client = phone(&phone_dir, &key).await;
    let ticket = desktop
        .invite(DeviceKind::Desktop, Duration::from_secs(60))
        .await
        .unwrap();
    client
        .join_workspace(ticket.encode_string(), "phone".into())
        .await
        .unwrap();
    let connected = client.mesh().await.unwrap().unwrap();
    until(&connected.mesh, || connected.mesh.peer(node.me()).is_some()).await;
    let place_key = Key::Project(format!("example/project/{}", node.me()));
    until(&connected.mesh, || {
        connected.mesh.get(&project_key).is_some()
            && connected.mesh.get(&place_key).is_some()
            && connected.mesh.get(&terminal_key).is_some()
    })
    .await;
    let projects = client.list_projects().await.unwrap();
    assert_eq!(projects.len(), 1);
    assert_eq!(projects[0].name, "example/project");
    assert_eq!(
        client.coding_hosts().await.unwrap()[0].agents,
        vec!["codex"]
    );
    assert_eq!(
        client.list_machine_stats().await.unwrap()[0].memory_total_mb,
        4096
    );
    let listed = client.list_machines().await.unwrap();
    assert_eq!(
        listed
            .iter()
            .find(|machine| machine.id == node.me().to_string())
            .unwrap()
            .env
            .as_ref()
            .unwrap()
            .os,
        "linux"
    );
    let sessions = client.coding_sessions(None).await.unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].id, "session-one");

    // Another controller reads the same node session before it disconnects.
    let desktop_peer = beep_mesh::with_deadline(node.me(), &desktop).await.unwrap();
    let before = beep_mesh::machine::sessions::list(&desktop_peer)
        .await
        .unwrap();
    assert_eq!(before[0].id, sessions[0].id);
    drop(desktop_peer);
    desktop_router.shutdown().await.unwrap();
    drop(desktop_router);
    drop(desktop);
    desktop_endpoint.close().await;
    until(&connected.mesh, || {
        connected.mesh.peer(desktop_endpoint.id()).is_none()
    })
    .await;

    assert_eq!(client.all_terminals().await.unwrap()[0].id, terminal_id);
    let terminal = client
        .attach_terminal(terminal_id.to_string())
        .await
        .unwrap();
    assert_eq!(terminal.next().await.unwrap().unwrap().data, b"ready");
    terminal.keys("typed from phone".into()).await.unwrap();
    assert_eq!(
        terminal.next().await.unwrap().unwrap().data,
        b"typed from phone"
    );
    client
        .close_terminal(terminal_id.to_string())
        .await
        .unwrap();
    assert!(client.all_terminals().await.unwrap().is_empty());
    drop(terminal);
    client
        .steer_coding(
            node.me().to_string(),
            "codex".into(),
            "session-one".into(),
            AgentSteer::Prompt {
                text: "Updated on phone".into(),
            },
        )
        .await
        .unwrap();
    let view = client
        .coding_session(node.me().to_string(), "codex".into(), "session-one".into())
        .await
        .unwrap();
    assert_eq!(view.last_message, "Updated on phone");
    assert_eq!(view.entries[0].text, "Updated on phone");
    assert_eq!(
        client
            .coding_sessions(Some("example/project".into()))
            .await
            .unwrap()
            .len(),
        1
    );
    connected
        .desk()
        .await
        .unwrap()
        .pin("example/project", true)
        .await
        .unwrap();
    let pin = Key::Pin("example/project".into());
    until(&node, || node.get(&pin).is_some()).await;
    drop(connected);
    drop(client);

    let reopened = phone(&phone_dir, &key).await;
    let connected = reopened.mesh().await.unwrap().unwrap();
    until(&connected.mesh, || connected.mesh.peer(node.me()).is_some()).await;
    assert!(reopened.workspace_linked().await.unwrap());
    let view = reopened
        .coding_session(node.me().to_string(), "codex".into(), "session-one".into())
        .await
        .unwrap();
    assert_eq!(view.last_message, "Updated on phone");
    assert_eq!(
        connected.desk().await.unwrap().on_top(),
        vec!["example/project"]
    );

    let desktop_endpoint = endpoint(&desktop_key).await;
    let desktop = Mesh::open(desktop_dir.path(), desktop_endpoint.clone())
        .await
        .unwrap()
        .unwrap();
    let desktop_router = router(&desktop_endpoint, &desktop);
    until(&desktop, || desktop.get(&pin).is_some()).await;
    let desktop_peer = beep_mesh::with_deadline(node.me(), &desktop).await.unwrap();
    let mut attached = beep_mesh::machine::open_agent(
        &desktop_peer,
        "codex",
        Request::Attach {
            session: "session-one".into(),
        },
    )
    .await
    .unwrap();
    beep_mesh::machine::order(
        &mut attached,
        Request::Prompt {
            text: "Updated on desktop".into(),
        },
        Duration::from_secs(3),
    )
    .await
    .unwrap();
    let view = reopened
        .coding_session(node.me().to_string(), "codex".into(), "session-one".into())
        .await
        .unwrap();
    assert_eq!(view.last_message, "Updated on desktop");
    assert!(desktop.get(&terminal_key).is_none());
    desktop.remove(key.public()).await.unwrap();
    until(&connected.mesh, || !connected.mesh.is_member(key.public())).await;
    assert!(matches!(
        reopened.list_projects().await,
        Err(BeepError::Unauthorized)
    ));
    assert!(matches!(
        reopened.coding_sessions(None).await,
        Err(BeepError::Unauthorized)
    ));
    desktop_router.shutdown().await.unwrap();
    node_router.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_node_ticket_cannot_pair_a_phone_as_a_project_controller() {
    let dir = TempDir::new().unwrap();
    let ep = endpoint(&SecretKey::generate()).await;
    let mesh = Mesh::create(dir.path(), ep.clone(), "desktop")
        .await
        .unwrap();
    let ticket = mesh
        .invite(DeviceKind::Node, Duration::from_secs(60))
        .await
        .unwrap();
    let phone_dir = TempDir::new().unwrap();
    let client = phone(&phone_dir, &SecretKey::generate()).await;
    assert!(matches!(
        client
            .join_workspace(ticket.encode_string(), "phone".into())
            .await,
        Err(BeepError::BadRequest { .. })
    ));
    assert!(!client.workspace_linked().await.unwrap());
    assert!(
        !client
            .workspace
            .get()
            .unwrap()
            .directory
            .join("devices")
            .exists()
    );
    ep.close().await;
}

#[tokio::test]
async fn pairing_requires_the_desktops_one_use_secret_and_records_the_authenticated_key() {
    let dir = TempDir::new().unwrap();
    let desktop_key = SecretKey::generate();
    let ep = endpoint(&desktop_key).await;
    let desktop = Mesh::create(dir.path(), ep.clone(), "desktop")
        .await
        .unwrap();
    let serving = router(&ep, &desktop);
    let ticket = desktop
        .invite(DeviceKind::Desktop, Duration::from_secs(300))
        .await
        .unwrap();
    let other_ticket = desktop
        .invite(DeviceKind::Desktop, Duration::from_secs(300))
        .await
        .unwrap();
    let phone_dir = TempDir::new().unwrap();
    let phone_key = SecretKey::generate();
    let client = phone(&phone_dir, &phone_key).await;
    assert!(
        client
            .join_workspace(String::new(), "phone".into())
            .await
            .is_err()
    );
    assert!(!desktop.is_member(phone_key.public()));
    let mut wrong = ticket.clone();
    wrong.secret = beep_mesh::JoinSecret::generate();
    assert!(
        client
            .join_workspace(wrong.encode_string(), "phone".into())
            .await
            .is_err()
    );
    assert!(!desktop.is_member(phone_key.public()));
    assert_eq!(desktop.joined_with(&ticket).await.unwrap(), None);
    assert!(client.mesh().await.unwrap().is_none());

    client
        .join_workspace(ticket.encode_string(), "phone".into())
        .await
        .unwrap();
    assert_eq!(
        desktop.joined_with(&ticket).await.unwrap(),
        Some(phone_key.public())
    );
    assert_eq!(desktop.joined_with(&other_ticket).await.unwrap(), None);

    let stranger_dir = TempDir::new().unwrap();
    let stranger_key = SecretKey::generate();
    let stranger = phone(&stranger_dir, &stranger_key).await;
    assert!(
        stranger
            .join_workspace(ticket.encode_string(), "stranger".into())
            .await
            .is_err()
    );
    assert!(!desktop.is_member(stranger_key.public()));
    assert_eq!(
        desktop.joined_with(&ticket).await.unwrap(),
        Some(phone_key.public())
    );
    let mut expired = desktop
        .invite(DeviceKind::Desktop, Duration::ZERO)
        .await
        .unwrap();
    expired.expires_ms = u64::MAX;
    assert!(
        stranger
            .join_workspace(expired.encode_string(), "stranger".into())
            .await
            .is_err()
    );
    assert!(!desktop.is_member(stranger_key.public()));
    serving.shutdown().await.unwrap();
    drop(serving);
    drop(desktop);
    ep.close().await;

    let ep = endpoint(&desktop_key).await;
    let desktop = Mesh::open(dir.path(), ep.clone()).await.unwrap().unwrap();
    assert_eq!(
        desktop.joined_with(&ticket).await.unwrap(),
        Some(phone_key.public())
    );
    ep.close().await;
}

#[tokio::test]
async fn server_features_stay_disabled_with_or_without_a_workspace() {
    let key = SecretKey::generate();
    let client = BeepClient::new(String::new(), key.to_bytes().to_vec(), "test".into()).unwrap();
    assert!(!crate::server_enabled());
    assert!(matches!(
        client.health().await,
        Err(BeepError::BadRequest { .. })
    ));
    assert!(client.follow.get().is_none());
    assert!(matches!(
        client.list_projects().await,
        Err(BeepError::BadRequest { .. })
    ));
    assert!(client.follow.get().is_none());
}
