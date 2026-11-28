use super::{Sessions, reserve};
use crate::agents::Parts;
use crate::agents::{AgentKind, Agents, session::Begin};
use beep_proto::{SessionId, watched};
use std::{collections::BTreeMap, path::Path, sync::Arc, time::Duration};
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
    let agents = Agents::new(Parts {
        kinds: vec![kind.clone()],
        ..Parts::default()
    });

    assert!(agents.sessions.read().is_empty());

    let session = agents
        .starter
        .start(&kind, Some("/"), Begin::default())
        .await
        .unwrap();

    assert!(agents.sessions.get(&session.id).is_some());
    assert!(agents.sessions.read().contains_key(&session.id));

    agents.sessions.remove(std::slice::from_ref(&session.id));

    assert!(agents.sessions.get(&session.id).is_none());
}

#[tokio::test]
async fn a_start_that_has_not_finished_holds_nothing_other_sessions_need() {
    let kind = silent();
    let agents = Agents::new(Parts {
        kinds: vec![kind.clone()],
        ..Parts::default()
    });
    let first = agents
        .starter
        .start(&kind, Some("/"), Begin::default())
        .await
        .unwrap();
    // A start between taking its id and inserting, which is where the
    // process spawns.
    let _slow = agents.sessions.reserve(None).unwrap();

    let others = async {
        assert!(agents.sessions.get(&first.id).is_some());
        agents.sessions.list();
        agents.sessions.listed(true);
        let second = agents
            .starter
            .start(&kind, Some("/"), Begin::default())
            .await
            .unwrap();
        assert!(agents.sessions.get(&second.id).is_some());
    };
    timeout(LONG, others)
        .await
        .expect("another start was held up");
}

#[tokio::test]
async fn one_session_is_not_taken_over_while_another_takeover_starts() {
    let kind = silent();
    let agents = Agents::new(Parts {
        kinds: vec![kind.clone()],
        ..Parts::default()
    });
    let id = "0f3c2a9e-1111-4222-8333-944455556666";
    let first = agents.sessions.reserve(Some(SessionId::from(id))).unwrap();

    let refused = agents
        .starter
        .start(&kind, Some("/"), resuming(id))
        .await
        .err()
        .unwrap();
    assert!(
        refused.to_string().contains("already runs here"),
        "{refused}"
    );

    // A start that gave up frees the session and its folder.
    drop(first);
    agents
        .starter
        .start(&kind, Some("/"), resuming(id))
        .await
        .unwrap();
}

#[tokio::test]
async fn a_new_id_skips_the_ids_of_starts_under_way() {
    let sessions = watch::Sender::new(Arc::default());
    let mut pending = Vec::new();
    let mut ids = ["aaaaaa", "aaaaaa", "bbbbbb"]
        .into_iter()
        .map(SessionId::from);
    let first = reserve(&sessions, &mut pending, None, || ids.next().unwrap()).unwrap();
    let second = reserve(&sessions, &mut pending, None, || ids.next().unwrap()).unwrap();

    assert_eq!(first.0, SessionId::from("aaaaaa"));
    assert_eq!(second.0, SessionId::from("bbbbbb"));
}

#[tokio::test]
async fn recovery_blocks_new_sessions_and_open_sessions_block_recovery() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let kind = silent();
    let agents = Agents::new(Parts {
        kinds: vec![kind.clone()],
        ..Parts::default()
    });
    let holds = agents.occupancy.holds();
    let hold = holds.hold(root).await.unwrap();
    let blocked = agents
        .starter
        .start(&kind, root.to_str(), Begin::default())
        .await;
    assert!(
        blocked
            .err()
            .unwrap()
            .to_string()
            .contains("preserving local edits")
    );

    let alias = dir.path().join("alias");
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(root, &alias).unwrap();
        let blocked = agents
            .starter
            .start(&kind, alias.to_str(), Begin::default())
            .await;
        assert!(
            blocked
                .err()
                .unwrap()
                .to_string()
                .contains("preserving local edits")
        );
    }
    #[cfg(not(unix))]
    let alias = root.to_owned();

    drop(hold);
    let session = agents
        .starter
        .start(&kind, alias.to_str(), Begin::default())
        .await
        .unwrap();
    let blocked = agents.occupancy.reserve(root).await;
    assert!(
        blocked
            .err()
            .unwrap()
            .to_string()
            .contains("close running sessions")
    );

    session.close().await;
    // No yield between the end and the check: the folder is free at once.
    assert!(!holds.running_under(root));
    agents.occupancy.reserve(root).await.unwrap();
}

/// Agents whose Claude Code sessions are read from `claude` and that find
/// no others.
fn reading(claude: &Path) -> (Agents, tempfile::TempDir) {
    let nothing = tempfile::tempdir().unwrap();
    let agents = Agents::new(Parts {
        occupancy: crate::occupancy::Occupancy::over(
            crate::logs::claude::Store::locate_in(claude),
            crate::logs::found::Stores::at(Some(nothing.path()), Some(nothing.path()), None),
        ),
        ..Parts::default()
    });

    (agents, nothing)
}

/// A Claude Code session of `cwd` whose last message came `ago` ago, as it
/// is when written again for a resume.
fn transcript(claude: &Path, id: &str, cwd: &Path, ago: Duration) -> std::path::PathBuf {
    let project = claude.join("projects/-checkout");
    std::fs::create_dir_all(&project).unwrap();
    let file = project.join(format!("{id}.jsonl"));
    let line = format!(
        r#"{{"type":"user","cwd":{},"timestamp":"{}","message":{{"role":"user","content":"hi"}}}}"#,
        serde_json::to_string(&cwd.to_string_lossy()).unwrap(),
        (chrono::Utc::now() - ago).to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
    );
    std::fs::write(&file, format!("{line}\n")).unwrap();
    file
}

const ID: &str = "0f3c2a9e-1111-4222-8333-944455556666";
const LONG_AGO: Duration = Duration::from_secs(24 * 3600);

#[tokio::test]
async fn recovery_reads_the_sessions_when_it_reserves() {
    let (claude, checkout) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (agents, _nothing) = reading(claude.path());

    // Nothing runs: the checkout is idle.
    drop(agents.occupancy.reserve(checkout.path()).await.unwrap());

    // A session started since, with no listing in between.
    transcript(claude.path(), ID, checkout.path(), Duration::ZERO);
    let blocked = agents.occupancy.reserve(checkout.path()).await;
    assert!(
        blocked
            .err()
            .unwrap()
            .to_string()
            .contains("external session is running")
    );
}

#[tokio::test]
async fn a_resumed_session_blocks_recovery() {
    let (claude, checkout) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (agents, _nothing) = reading(claude.path());
    transcript(claude.path(), ID, checkout.path(), LONG_AGO);
    drop(agents.occupancy.reserve(checkout.path()).await.unwrap());

    transcript(claude.path(), ID, checkout.path(), Duration::ZERO);
    let blocked = agents.occupancy.reserve(checkout.path()).await;
    assert!(
        blocked
            .err()
            .unwrap()
            .to_string()
            .contains("external session is running")
    );
}

#[tokio::test]
async fn one_lock_serves_every_holder_of_a_stored_session() {
    let agents = Agents::new(Parts {
        kinds: vec![silent()],
        ..Parts::default()
    });
    let id = SessionId::from("stored-1");

    let first = agents.sessions.store_of(&id);
    let again = agents.sessions.store_of(&id);
    let other = agents.sessions.store_of(&SessionId::from("stored-2"));

    assert!(Arc::ptr_eq(&first, &again));
    assert!(!Arc::ptr_eq(&first, &other));

    drop((first, again));

    assert_eq!(crate::lock(&agents.sessions.store).len(), 2);

    // A lock nobody holds is forgotten by the next look.
    agents.sessions.store_of(&SessionId::from("stored-3"));
    assert_eq!(crate::lock(&agents.sessions.store).len(), 2);
}

#[tokio::test]
async fn a_subscriber_sees_an_insert_and_a_removal() {
    let kind = silent();
    let agents = Agents::new(Parts {
        kinds: vec![kind.clone()],
        ..Parts::default()
    });
    let mut map = agents.sessions.subscribe();

    assert!(watched::read_and_update(&mut map, |map| map.is_empty()));

    let session = agents
        .starter
        .start(&kind, Some("/"), Begin::default())
        .await
        .unwrap();

    timeout(LONG, map.changed()).await.unwrap().unwrap();
    assert!(watched::read_and_update(&mut map, |map| map.contains_key(&session.id)));

    agents.sessions.remove(std::slice::from_ref(&session.id));

    timeout(LONG, map.changed()).await.unwrap().unwrap();
    assert!(watched::read_and_update(&mut map, |map| map.is_empty()));
}

#[tokio::test]
async fn a_node_with_no_children_counts_none() {
    let kind = silent();
    let agents = Agents::new(Parts {
        kinds: vec![kind.clone()],
        ..Parts::default()
    });
    let session = agents
        .starter
        .start(&kind, Some("/"), Begin::default())
        .await
        .unwrap();
    let who = agents.family.who_of(&session);

    assert_eq!(agents.sessions.local_children(&who), 0);
    assert!(agents.sessions.peers(None).is_empty());
    assert!(agents.sessions.owned().is_empty());
}

#[tokio::test]
async fn sessions_answer_the_reads_without_an_agents() {
    let kind = silent();
    let agents = Agents::new(Parts {
        kinds: vec![kind.clone()],
        ..Parts::default()
    });
    let session = agents
        .starter
        .start(&kind, Some("/"), Begin::default())
        .await
        .unwrap();

    let sessions = Sessions::default();
    let reservation = sessions.reserve(None).unwrap();
    let id = reservation.id.clone();
    sessions.insert(reservation, None, session.clone());

    assert!(Arc::ptr_eq(&sessions.get(&id).unwrap(), &session));
    assert!(sessions.get(&SessionId::from("nobody")).is_none());
    assert!(sessions.runs(&id));
    assert!(!sessions.runs(&SessionId::from("nobody")));
    assert_eq!(session.link_name(), session.id);
}
