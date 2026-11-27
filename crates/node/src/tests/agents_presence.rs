use super::*;
use crate::agents::Parts;
use crate::occupancy::presence::Registration;
use crate::occupancy::presence::Registry;
use beep_proto::SessionId;
use beep_proto::node::LinkRx;
use beep_proto::node::agent::{ExternalSession, Presence, SessionsReply, SessionsRequest};
use serde_json::json;
use std::sync::Arc;
use tokio::time::{Duration, timeout};
use uuid::Uuid;

fn registration(id: &str) -> Registration {
    serde_json::from_value(json!({
        "version": 1, "agent": "flsh", "session_id": id,
        "cwd": std::env::temp_dir().join("presence-project"), "pid": 123
    }))
    .unwrap()
}

#[tokio::test]
async fn native_presence_answers_without_a_transcript_and_overrides_recent_files() {
    let agents = Agents::default();
    let id = Uuid::new_v4();
    agents
        .occupancy
        .presence()
        .register(registration("session_a"), id)
        .unwrap();
    let found = agents
        .listings
        .stores_external("flsh", 10, Some("session_a".into()))
        .await
        .unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].presence, Presence::Running);
    assert!(
        agents
            .listings
            .stores_external("codex", 10, None)
            .await
            .unwrap()
            .is_empty()
    );
    agents
        .occupancy
        .presence()
        .close(&SessionId::from("session_a"), id);
    let found = agents
        .listings
        .stores_external("flsh", 10, None)
        .await
        .unwrap();
    assert_eq!(found[0].presence, Presence::Stopped);
}

#[tokio::test]
async fn a_mesh_follow_reports_open_and_close_without_polling() {
    let agents = Arc::new(Agents::default());
    let serving = agents.clone();
    let link = beep_proto::node::local_link();
    let (mut client_tx, mut client_rx) = link.caller;
    let (server_tx, server_rx) = link.node;
    let task = tokio::spawn(async move {
        crate::agents::serve_sessions(
            server_tx,
            server_rx,
            serving,
            iroh::SecretKey::generate().public(),
            None,
        )
        .await
    });
    client_tx
        .send(&SessionsRequest::FollowPresence {})
        .await
        .unwrap();
    async fn next(rx: &mut LinkRx) -> Option<SessionsReply> {
        timeout(Duration::from_secs(5), rx.recv::<SessionsReply>())
            .await
            .unwrap()
            .unwrap()
    }
    assert_eq!(
        next(&mut client_rx).await,
        Some(SessionsReply::External { sessions: vec![] })
    );
    let id = Uuid::new_v4();
    agents
        .occupancy
        .presence()
        .register(registration("session_a"), id)
        .unwrap();
    let Some(SessionsReply::External { sessions }) = next(&mut client_rx).await else {
        panic!("no presence snapshot");
    };
    assert_eq!(sessions[0].presence, Presence::Running);
    agents
        .occupancy
        .presence()
        .close(&SessionId::from("session_a"), id);
    let Some(SessionsReply::External { sessions }) = next(&mut client_rx).await else {
        panic!("no presence snapshot");
    };
    assert_eq!(sessions[0].presence, Presence::Stopped);
    drop(client_tx);
    timeout(Duration::from_secs(5), task)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
}

async fn ask(agents: &Arc<Agents>, request: SessionsRequest) -> SessionsReply {
    let link = beep_proto::node::local_link();
    let (mut client_tx, mut client_rx) = link.caller;
    let (server_tx, server_rx) = link.node;
    let serving = agents.clone();
    let task = tokio::spawn(async move {
        crate::agents::serve_sessions(
            server_tx,
            server_rx,
            serving,
            iroh::SecretKey::generate().public(),
            None,
        )
        .await
    });
    client_tx.send(&request).await.unwrap();
    let reply = timeout(Duration::from_secs(15), client_rx.recv::<SessionsReply>())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    drop(client_tx);
    task.await.unwrap().unwrap();
    reply
}

#[cfg(unix)]
#[tokio::test]
async fn continuing_here_ends_only_the_followed_session_s_process() {
    use std::os::unix::process::ExitStatusExt;

    let agents = Arc::new(Agents::default());
    let mut child = tokio::process::Command::new("sleep")
        .arg("60")
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut outside = registration("session_a");
    outside.pid = child.id();
    agents
        .occupancy
        .presence()
        .register(outside, Uuid::new_v4())
        .unwrap();
    let mut node = registration("session_b");
    node.pid = Some(std::process::id());
    agents
        .occupancy
        .presence()
        .register(node, Uuid::new_v4())
        .unwrap();

    // The process is the one the node finds running that harness's session.
    for (agent, session) in [
        ("codex", "session_a"),
        ("flsh", "session_c"),
        ("flsh", "session_b"),
    ] {
        let reply = ask(
            &agents,
            SessionsRequest::CloseExternal {
                agent: agent.into(),
                session: session.into(),
            },
        )
        .await;
        assert!(matches!(reply, SessionsReply::Error { .. }), "{reply:?}");
    }
    assert!(child.try_wait().unwrap().is_none());

    let reply = ask(
        &agents,
        SessionsRequest::CloseExternal {
            agent: "flsh".into(),
            session: "session_a".into(),
        },
    )
    .await;
    assert_eq!(reply, SessionsReply::ProcessStopped {});
    let status = timeout(Duration::from_secs(5), child.wait())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(status.signal(), Some(libc::SIGTERM));
}

/// flsh registers with no title, then sends the generated one on the same
/// connection; it replaces the first prompt read from the transcript.
#[test]
fn a_later_presence_title_replaces_the_first_prompt() {
    let registry = Registry::default();
    let connection = Uuid::new_v4();
    let found = || ExternalSession {
        id: SessionId::from("session_a"),
        agent: "flsh".into(),
        title: Some("explain rust lifetimes to me please".into()),
        ..ExternalSession::default()
    };
    registry
        .register(registration("session_a"), connection)
        .unwrap();
    let mut sessions = vec![found()];
    registry.merge(&mut sessions);
    assert_eq!(
        sessions[0].title.as_deref(),
        Some("explain rust lifetimes to me please")
    );

    let mut titled = registration("session_a");
    titled.title = Some("Explain Rust lifetime annotations".into());
    registry.register(titled, connection).unwrap();
    let mut sessions = vec![found()];
    registry.merge(&mut sessions);
    assert_eq!(
        sessions[0].title.as_deref(),
        Some("Explain Rust lifetime annotations")
    );

    // A later registration without a title keeps it.
    registry
        .register(registration("session_a"), connection)
        .unwrap();
    assert_eq!(
        registry.snapshot()[0].title.as_deref(),
        Some("Explain Rust lifetime annotations")
    );
}

#[tokio::test]
async fn starting_and_owned_sessions_mark_their_presence_taken_over() {
    use crate::agents::{AgentKind, session::Begin};
    use std::collections::BTreeMap;

    let kind = AgentKind {
        name: "flsh".into(),
        program: "sh".into(),
        args: vec!["-c".into(), "cat > /dev/null".into()],
        env: BTreeMap::new(),
        compact: None,
        compact_models: Vec::new(),
        default_model: None,
        bridge: None,
    };
    let agents = Arc::new(Agents::new(Parts {
        kinds: vec![kind.clone()],
        ..Parts::default()
    }));
    let link = beep_proto::node::local_link();
    let (mut client_tx, mut client_rx) = link.caller;
    let (server_tx, server_rx) = link.node;
    let serving = agents.clone();
    let task = tokio::spawn(async move {
        super::super::serve_sessions(
            server_tx,
            server_rx,
            serving,
            iroh::SecretKey::generate().public(),
            None,
        )
        .await
    });
    client_tx
        .send(&SessionsRequest::FollowPresence {})
        .await
        .unwrap();
    async fn next(rx: &mut LinkRx) -> Vec<ExternalSession> {
        match timeout(Duration::from_secs(5), rx.recv::<SessionsReply>())
            .await
            .unwrap()
            .unwrap()
        {
            Some(SessionsReply::External { sessions }) => sessions,
            other => panic!("expected presence snapshot, got {other:?}"),
        }
    }
    assert!(next(&mut client_rx).await.is_empty());

    // A process spawned by Beep registers presence before ACP names its session:
    // its root pid already marks it as owned by Beep.
    let starting = agents
        .starter
        .start(&kind, Some("/"), Begin::default())
        .await
        .unwrap();
    let mut spawned = registration("session_starting");
    spawned.pid = Some(starting.processes.root());
    agents
        .occupancy
        .presence()
        .register(spawned, Uuid::new_v4())
        .unwrap();
    let sessions = next(&mut client_rx).await;
    assert_eq!(
        sessions[0].taken_over,
        Some(SessionId::from("session_starting")),
        "a starting Beep process must not show as an outside session"
    );

    // An outside session taken over in Beep pushes its new ownership without a
    // new registration on the presence socket.
    let mut outside = registration("session_outside");
    outside.pid = Some(u32::MAX);
    agents
        .occupancy
        .presence()
        .register(outside, Uuid::new_v4())
        .unwrap();
    let sessions = next(&mut client_rx).await;
    assert_eq!(
        sessions
            .iter()
            .find(|s| s.id == "session_outside")
            .and_then(|s| s.taken_over.as_ref()),
        None
    );

    let _resumed = agents
        .starter
        .start(
            &kind,
            Some("/"),
            Begin {
                resume: Some("session_outside".into()),
                ..Begin::default()
            },
        )
        .await
        .unwrap();
    let sessions = next(&mut client_rx).await;
    assert_eq!(
        sessions
            .iter()
            .find(|s| s.id == "session_outside")
            .and_then(|s| s.taken_over.clone()),
        Some(SessionId::from("session_outside"))
    );

    drop(client_tx);
    timeout(Duration::from_secs(5), task)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
}
