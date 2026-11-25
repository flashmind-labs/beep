use crate::agents::{AgentKind, Agents, session::Begin};
use beep_proto::SessionId;
use super::reserve;
use std::{collections::BTreeMap, path::PathBuf, sync::Arc, time::Duration};
use tokio::{sync::watch, time::timeout};

/// Generous: only a hang can reach it.
const LONG: Duration = Duration::from_secs(60);

/// An adapter that never answers: its sessions stay starting.
fn silent() -> AgentKind {
    AgentKind {
        name: "claude".into(),
        program: "sh".into(),
        args: vec!["-c".into(), "cat > /dev/null".into()],
        env: BTreeMap::new(),
        compact: None,
        compact_models: Vec::new(),
        default_model: None,
        bridge: None,
    }
}

fn resuming(id: &str) -> Begin {
    Begin {
        resume: Some(SessionId::from(id)),
        ..Begin::default()
    }
}

#[tokio::test]
async fn an_inserted_session_is_there_for_the_next_read() {
    let kind = silent();
    let agents = Agents::new(vec![kind.clone()]);

    assert!(agents.session_map().is_empty());

    let session = agents.start(&kind, Some("/"), Begin::default()).await.unwrap();

    assert!(agents.get(&session.id).is_some());
    assert!(agents.session_map().contains_key(&session.id));

    agents.sessions.remove(std::slice::from_ref(&session.id));

    assert!(agents.get(&session.id).is_none());
}

#[tokio::test]
async fn a_start_that_has_not_finished_holds_nothing_other_sessions_need() {
    let kind = silent();
    let agents = Agents::new(vec![kind.clone()]);
    let first = agents.start(&kind, Some("/"), Begin::default()).await.unwrap();
    // A start between taking its id and inserting, which is where the
    // process spawns.
    let _slow = agents
        .sessions
        .reserve(None, PathBuf::from("/srv/slow"))
        .unwrap();

    let others = async {
        assert!(agents.get(&first.id).is_some());
        agents.list();
        agents.listed(true);
        let second = agents.start(&kind, Some("/"), Begin::default()).await.unwrap();
        assert!(agents.get(&second.id).is_some());
    };
    timeout(LONG, others).await.expect("another start was held up");

    assert_eq!(agents.sessions.starting(), [PathBuf::from("/srv/slow")]);
}

#[tokio::test]
async fn one_session_is_not_taken_over_while_another_takeover_starts() {
    let kind = silent();
    let agents = Agents::new(vec![kind.clone()]);
    let id = "0f3c2a9e-1111-4222-8333-944455556666";
    let first = agents
        .sessions
        .reserve(Some(SessionId::from(id)), PathBuf::from("/"))
        .unwrap();

    let refused = agents.start(&kind, Some("/"), resuming(id)).await.err().unwrap();
    assert!(refused.to_string().contains("already runs here"), "{refused}");

    // A start that gave up frees the session and its folder.
    drop(first);
    agents.start(&kind, Some("/"), resuming(id)).await.unwrap();
    assert!(agents.sessions.starting().is_empty());
}

#[tokio::test]
async fn a_new_id_skips_the_ids_of_starts_under_way() {
    let sessions = watch::Sender::new(Arc::default());
    let mut pending = Vec::new();
    let mut ids = ["aaaaaa", "aaaaaa", "bbbbbb"].into_iter().map(SessionId::from);
    let first = reserve(&sessions, &mut pending, None, PathBuf::from("/"), || {
        ids.next().unwrap()
    })
    .unwrap();
    let second = reserve(&sessions, &mut pending, None, PathBuf::from("/"), || {
        ids.next().unwrap()
    })
    .unwrap();

    assert_eq!(first.0, SessionId::from("aaaaaa"));
    assert_eq!(second.0, SessionId::from("bbbbbb"));
}
