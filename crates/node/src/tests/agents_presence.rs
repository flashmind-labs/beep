use super::*;
use crate::agents::Agents;
use crate::agents::found::Stores;
use beep_proto::node::agent::{SessionsReply, SessionsRequest};
use beep_proto::node::{LinkRx, agent::Presence};
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

#[test]
fn only_the_last_connection_closes_a_session() {
    let registry = Registry::default();
    let a = Uuid::new_v4();
    let b = Uuid::new_v4();
    registry.register(registration("session_a"), a).unwrap();
    registry.register(registration("session_a"), b).unwrap();
    registry.close("session_a", a);
    assert_eq!(registry.snapshot()[0].presence, Presence::Running);
    registry.close("session_a", b);
    assert_eq!(registry.snapshot()[0].presence, Presence::Stopped);
    assert_eq!(registry.snapshot()[0].pid, None);
}

#[test]
fn registration_is_validated_and_session_metadata_updates() {
    let registry = Registry::default();
    let connection = Uuid::new_v4();
    let mut invalid = registration("session_a");
    invalid.cwd = "relative".into();
    assert!(registry.register(invalid, connection).is_err());
    assert!(registry.snapshot().is_empty());
    let mut changed = registration("session_a");
    let cwd = std::env::temp_dir()
        .join("presence-other")
        .display()
        .to_string();
    changed.cwd = cwd.clone();
    changed.title = Some("Working title".into());
    registry.register(changed, connection).unwrap();
    assert_eq!(registry.snapshot()[0].cwd, cwd);
    assert_eq!(
        registry.snapshot()[0].title.as_deref(),
        Some("Working title")
    );
}

#[tokio::test]
async fn native_presence_answers_without_a_transcript_and_overrides_recent_files() {
    let agents = Agents {
        stores: Stores::default(),
        ..Agents::default()
    };
    let id = Uuid::new_v4();
    agents
        .presence
        .register(registration("session_a"), id)
        .unwrap();
    let found = agents
        .stores_external("flsh", 10, Some("session_a".into()))
        .await
        .unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].presence, Presence::Running);
    assert!(
        agents
            .stores_external("codex", 10, None)
            .await
            .unwrap()
            .is_empty()
    );
    agents.presence.close("session_a", id);
    let found = agents.stores_external("flsh", 10, None).await.unwrap();
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
        .presence
        .register(registration("session_a"), id)
        .unwrap();
    let Some(SessionsReply::External { sessions }) = next(&mut client_rx).await else {
        panic!("no presence snapshot");
    };
    assert_eq!(sessions[0].presence, Presence::Running);
    agents.presence.close("session_a", id);
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
        super::super::serve_sessions(
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

    let agents = Arc::new(Agents {
        stores: Stores::default(),
        ..Agents::default()
    });
    let mut child = tokio::process::Command::new("sleep")
        .arg("60")
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut outside = registration("session_a");
    outside.pid = child.id();
    agents.presence.register(outside, Uuid::new_v4()).unwrap();
    let mut node = registration("session_b");
    node.pid = Some(std::process::id());
    agents.presence.register(node, Uuid::new_v4()).unwrap();

    // The process is the one the node finds running that harness's session.
    for (agent, session) in [("codex", "session_a"), ("flsh", "session_c"), ("flsh", "session_b")] {
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
