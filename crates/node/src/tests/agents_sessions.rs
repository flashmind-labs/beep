use crate::agents::{AgentKind, Agents, session::Begin};
use beep_proto::SessionId;
use super::reserve;
use std::{collections::BTreeMap, sync::Arc, time::Duration};
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
        .reserve(None)
        .unwrap();

    let others = async {
        assert!(agents.get(&first.id).is_some());
        agents.list();
        agents.listed(true);
        let second = agents.start(&kind, Some("/"), Begin::default()).await.unwrap();
        assert!(agents.get(&second.id).is_some());
    };
    timeout(LONG, others).await.expect("another start was held up");
}

#[tokio::test]
async fn one_session_is_not_taken_over_while_another_takeover_starts() {
    let kind = silent();
    let agents = Agents::new(vec![kind.clone()]);
    let id = "0f3c2a9e-1111-4222-8333-944455556666";
    let first = agents
        .sessions
        .reserve(Some(SessionId::from(id)))
        .unwrap();

    let refused = agents.start(&kind, Some("/"), resuming(id)).await.err().unwrap();
    assert!(refused.to_string().contains("already runs here"), "{refused}");

    // A start that gave up frees the session and its folder.
    drop(first);
    agents.start(&kind, Some("/"), resuming(id)).await.unwrap();
}

#[tokio::test]
async fn a_new_id_skips_the_ids_of_starts_under_way() {
    let sessions = watch::Sender::new(Arc::default());
    let mut pending = Vec::new();
    let mut ids = ["aaaaaa", "aaaaaa", "bbbbbb"].into_iter().map(SessionId::from);
    let first = reserve(&sessions, &mut pending, None, || {
        ids.next().unwrap()
    })
    .unwrap();
    let second = reserve(&sessions, &mut pending, None, || {
        ids.next().unwrap()
    })
    .unwrap();

    assert_eq!(first.0, SessionId::from("aaaaaa"));
    assert_eq!(second.0, SessionId::from("bbbbbb"));
}

#[tokio::test]
async fn recovery_blocks_new_sessions_and_open_sessions_block_recovery() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let kind = silent();
    let agents = Agents::new(vec![kind.clone()]);
    let holds = agents.holds();
    let hold = holds.reserve(root).await.unwrap();
    let blocked = agents.start(&kind, root.to_str(), Begin::default()).await;
    assert!(blocked.err().unwrap().to_string().contains("preserving local edits"));

    let alias = dir.path().join("alias");
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(root, &alias).unwrap();
        let blocked = agents.start(&kind, alias.to_str(), Begin::default()).await;
        assert!(blocked.err().unwrap().to_string().contains("preserving local edits"));
    }
    #[cfg(not(unix))]
    let alias = root.to_owned();

    drop(hold);
    let session = agents.start(&kind, alias.to_str(), Begin::default()).await.unwrap();
    let blocked = holds.reserve(root).await;
    assert!(blocked.err().unwrap().to_string().contains("close running sessions"));

    session.close().await;
    timeout(LONG, async {
        while holds.reserve(root).await.is_err() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("an ended session still holds its folder");
}
