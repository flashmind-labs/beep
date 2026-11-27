use super::*;
use crate::agents::{AgentKind, Begin};
use std::{
    collections::BTreeMap,
    fs::{self, File},
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, SystemTime},
};

const CLAUDE_ID: &str = "0f3c2a9e-1111-4222-8333-944455556666";
const CLAUDE_OTHER: &str = "7a1b2c3d-4e5f-4a6b-8c7d-8e9f0a1b2c3d";
const CODEX_ID: &str = "01a199e3-03f2-7820-a29e-067ae8c4ec85";
const CODEX_OTHER: &str = "01a199e3-03f2-7820-a29e-067ae8c4ec86";

/// Every harness's store in one temp folder, with two sessions each and
/// a file beside the stores that no delete may touch.
struct Machine {
    dir: tempfile::TempDir,
    agents: Agents,
}

fn old(path: &Path) {
    File::options()
        .write(true)
        .open(path)
        .and_then(|file| file.set_modified(SystemTime::now() - Duration::from_secs(86_400)))
        .expect("an old modification time");
}

fn write(path: &Path, text: &str) -> PathBuf {
    fs::create_dir_all(path.parent().expect("a parent")).expect("folders");
    fs::write(path, text).expect("a file");
    old(path);
    path.to_owned()
}

fn kind(name: &str) -> AgentKind {
    AgentKind {
        name: name.into(),
        program: "sh".into(),
        args: vec!["-c".into(), "cat > /dev/null".into()],
        env: BTreeMap::new(),
        compact: None,
        compact_models: Vec::new(),
        default_model: None,
        bridge: None,
    }
}

impl Machine {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("a temp folder");
        let root = dir.path();
        let user = r#"{"type":"user","timestamp":"2026-10-29T08:00:00.000Z","message":{"role":"user","content":"hi"}}"#;

        for id in [CLAUDE_ID, CLAUDE_OTHER] {
            write(
                &root.join(format!("claude/projects/-srv-project/{id}.jsonl")),
                user,
            );
            write(
                &root.join(format!(
                    "claude/projects/-srv-project/{id}/subagents/a.jsonl"
                )),
                "{}",
            );
        }

        for id in [CODEX_ID, CODEX_OTHER] {
            let head = format!(
                r#"{{"type":"session_meta","payload":{{"id":"{id}","cwd":"/srv/project","timestamp":"2026-08-27T23:01:39.077Z"}}}}"#
            );
            write(
                &root.join(format!("codex/sessions/2026/08/27/rollout-x-{id}.jsonl")),
                &format!("{head}\n"),
            );
        }

        for id in ["p1", "p2"] {
            let head = format!(
                r#"{{"type":"session","version":3,"id":"{id}","timestamp":"2026-08-27T23:01:39.077Z","cwd":"/srv/project"}}"#
            );
            write(
                &root.join(format!("pi/--srv-project--/2026_{id}.jsonl")),
                &format!("{head}\n"),
            );
        }

        write(&root.join("outside.jsonl"), "keep me");
        let agents = Agents {
            catalog: Arc::new(crate::agents::catalog::Catalog::new(vec![kind("claude"), kind("codex"), kind("pi")], None, None)),
            occupancy: crate::occupancy::Occupancy::over(crate::logs::claude::Store::at(&root.join("claude")), crate::logs::found::Stores::at(Some(&root.join("codex")), Some(&root.join("pi")), None)),
            ..Agents::default()
        };
        Self { dir, agents }
    }

    fn has(&self, path: &str) -> bool {
        self.dir.path().join(path).exists()
    }

    fn claude(&self, id: &str) -> bool {
        self.has(&format!("claude/projects/-srv-project/{id}.jsonl"))
    }

    fn side(&self, id: &str) -> bool {
        self.has(&format!("claude/projects/-srv-project/{id}"))
    }

    fn codex(&self, id: &str) -> bool {
        self.has(&format!("codex/sessions/2026/08/27/rollout-x-{id}.jsonl"))
    }

    fn pi(&self, id: &str) -> bool {
        self.has(&format!("pi/--srv-project--/2026_{id}.jsonl"))
    }
}

#[tokio::test]
async fn a_deleted_session_leaves_the_listing_kept_from_before() {
    let m = Machine::new();
    let listed = || async {
        m.agents
            .sightings()
            .await
            .unwrap()
            .into_iter()
            .map(|seen| seen.id.to_string())
            .collect::<Vec<_>>()
    };

    assert!(listed().await.contains(&CODEX_ID.to_owned()));
    m.agents.delete_session("codex", &SessionId::from(CODEX_ID)).await.unwrap();

    assert!(!listed().await.contains(&CODEX_ID.to_owned()));
}

#[tokio::test]
async fn each_harness_deletes_only_its_own_session() {
    let m = Machine::new();
    m.agents.delete_session("claude", &SessionId::from(CLAUDE_ID)).await.unwrap();
    assert!(!m.claude(CLAUDE_ID) && !m.side(CLAUDE_ID));
    assert!(m.claude(CLAUDE_OTHER) && m.side(CLAUDE_OTHER));
    assert!(m.codex(CODEX_ID) && m.pi("p1"));

    m.agents.delete_session("codex", &SessionId::from(CODEX_ID)).await.unwrap();
    assert!(!m.codex(CODEX_ID) && m.codex(CODEX_OTHER));

    m.agents.delete_session("pi", &SessionId::from("p1")).await.unwrap();
    assert!(!m.pi("p1") && m.pi("p2"));
    assert!(m.claude(CLAUDE_OTHER) && m.codex(CODEX_OTHER));
    assert!(m.has("outside.jsonl"));

    let err = m.agents.delete_session("pi", &SessionId::from("p1")).await.unwrap_err();
    assert!(err.to_string().contains("no pi session"), "{err}");
}

#[tokio::test]
async fn a_deleted_session_is_no_longer_listed() {
    let m = Machine::new();
    let listed = |m: &Machine| {
        let stores = m.agents.stores().clone();
        stores.find(Agent::Codex, &SessionId::from(CODEX_ID)).is_some()
    };
    assert!(listed(&m));
    m.agents.delete_session("codex", &SessionId::from(CODEX_ID)).await.unwrap();
    assert!(!listed(&m));
    m.agents.delete_session("claude", &SessionId::from(CLAUDE_ID)).await.unwrap();
    let left = m
        .agents
        .external()
        .list(10, None, Default::default())
        .await
        .unwrap();
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].id, CLAUDE_OTHER);
}

#[tokio::test]
async fn a_running_session_is_refused_and_an_ended_one_is_forgotten() {
    let m = Machine::new();
    let begin = Begin {
        resume: Some(SessionId::from(CLAUDE_ID)),
        ..Begin::default()
    };
    let session = m
        .agents
        .start(&kind("claude"), Some("/"), begin)
        .await.expect("a session");
    for id in [session.id.clone(), SessionId::from(CLAUDE_ID)] {
        let err = m.agents.delete_session("claude", &id).await.unwrap_err();
        assert!(err.to_string().contains("still runs"), "{err}");
    }

    assert!(m.claude(CLAUDE_ID), "the transcript stays");
    assert_eq!(m.agents.sessions.list().len(), 1);
    assert_eq!(m.agents.reopen.open_sessions().len(), 1, "it would be reopened");

    session.close().await;
    m.agents
        .delete_session("claude", &session.id)
        .await
        .unwrap();
    assert!(!m.claude(CLAUDE_ID) && !m.side(CLAUDE_ID));
    assert!(m.agents.sessions.list().is_empty(), "the node's record is gone");
    assert!(m.agents.reopen.open_sessions().is_empty(), "and not reopened");
}

#[tokio::test]
async fn a_session_another_process_has_recently_written_is_refused() {
    let m = Machine::new();
    let file = m
        .dir
        .path()
        .join(format!("claude/projects/-srv-project/{CLAUDE_ID}.jsonl"));
    File::options()
        .write(true)
        .open(&file)
        .and_then(|file| file.set_modified(SystemTime::now()))
        .expect("a new modification time");
    let err = m
        .agents
        .delete_session("claude", &SessionId::from(CLAUDE_ID))
        .await
        .unwrap_err();
    assert!(err.to_string().contains("terminal or IDE"), "{err}");
    assert!(m.claude(CLAUDE_ID));
}

#[tokio::test]
async fn an_id_that_walks_out_of_the_store_is_refused() {
    let m = Machine::new();

    for agent in ["claude", "codex", "pi"] {
        for id in [
            "..",
            "../outside",
            "../../outside.jsonl",
            "a/b",
            "/etc/passwd",
            "",
            "x\0y",
            "p1/../p2",
        ] {
            let err = m.agents.delete_session(agent, &SessionId::from(id)).await.unwrap_err();
            assert!(
                err.to_string().contains("not a session id"),
                "{id:?}: {err}"
            );
        }
    }

    assert!(m.has("outside.jsonl"));
    assert!(m.claude(CLAUDE_ID) && m.codex(CODEX_ID) && m.pi("p1"));
}

#[test]
fn a_file_outside_the_store_or_a_link_to_one_is_not_removed() {
    let dir = tempfile::tempdir().expect("a temp folder");
    let store = dir.path().join("store");
    fs::create_dir_all(&store).expect("a store");
    let outside = write(&dir.path().join("outside.jsonl"), "keep me");
    assert!(crate::logs::found::remove_file_in(&store, &outside).is_err());
    let link = store.join("link.jsonl");
    std::os::unix::fs::symlink(&outside, &link).expect("a link");
    assert!(crate::logs::found::remove_file_in(&store, &link).is_err());
    let folder = store.join("folder-link");
    std::os::unix::fs::symlink(dir.path(), &folder).expect("a folder link");
    crate::logs::found::remove_dir_in(&store, &folder).expect("a link is skipped");
    assert!(outside.exists() && link.exists());
}

#[tokio::test]
async fn sessions_kept_in_a_database_are_refused() {
    let m = Machine::new();

    for agent in ["flsh", "opencode"] {
        let err = m.agents.delete_session(agent, &SessionId::from("ses_1")).await.unwrap_err();
        assert!(err.to_string().contains("database"), "{err}");
    }

    let err = m
        .agents
        .delete_session("example", &SessionId::from("ses_1"))
        .await
        .unwrap_err();
    assert!(err.to_string().contains("no agent"), "{err}");
}

#[tokio::test]
async fn a_closed_codex_session_moves_without_changing_the_conversation() {
    let m = Machine::new();
    let destination = m.dir.path().join("destination");
    fs::create_dir(&destination).expect("destination");
    m.agents
        .relocate_session("codex", &SessionId::from(CODEX_ID), destination.to_str().expect("UTF-8 path"))
        .await
        .expect("move");
    let moved = m
        .agents
        .stores()
        .find(Agent::Codex, &SessionId::from(CODEX_ID))
        .expect("moved session");
    assert_eq!(Path::new(&moved.cwd), destination);
    assert!(m.codex(CODEX_OTHER));
    assert!(
        m.agents
            .relocate_session("codex", &SessionId::from("../outside"), "/srv/project")
            .await
            .is_err()
    );
    assert!(
        m.agents
            .relocate_session("codex", &SessionId::from(CODEX_OTHER), "/missing/destination")
            .await
            .is_err()
    );
}

#[tokio::test]
async fn an_ended_owned_session_keeps_its_identity_after_moving() {
    let m = Machine::new();
    let destination = m.dir.path().join("destination");
    fs::create_dir(&destination).expect("destination");
    let session = m
        .agents
        .start(
            &kind("codex"),
            Some("/"),
            Begin {
                resume: Some(SessionId::from(CODEX_ID)),
                ..Begin::default()
            },
        )
        .await.expect("session");
    assert!(
        m.agents
            .relocate_session("codex", &session.id, destination.to_str().unwrap())
            .await
            .is_err()
    );
    session.close().await;
    m.agents
        .relocate_session("codex", &session.id, destination.to_str().unwrap())
        .await
        .expect("move");
    let listed = m.agents.sessions.list();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, session.info().id);
    assert_eq!(Path::new(&listed[0].cwd), destination);
}

#[tokio::test]
async fn a_pi_session_moves_to_its_new_store_folder() {
    let m = Machine::new();
    let destination = m.dir.path().join("destination");
    fs::create_dir(&destination).expect("destination");
    m.agents
        .relocate_session("pi", &SessionId::from("p1"), destination.to_str().unwrap())
        .await
        .expect("move");
    let found = m
        .agents
        .stores()
        .find(Agent::Pi, &SessionId::from("p1"))
        .expect("moved session");
    assert_eq!(Path::new(&found.cwd), destination);
    assert!(!m.pi("p1"));
    assert!(m.pi("p2"));
}

#[tokio::test]
async fn a_claude_session_moves_with_its_subagents() {
    let m = Machine::new();
    let destination = m.dir.path().join("destination");
    fs::create_dir(&destination).expect("destination");
    let transcript = m
        .dir
        .path()
        .join(format!("claude/projects/-srv-project/{CLAUDE_ID}.jsonl"));
    write(
        &transcript,
        &format!(
            concat!(
                r#"{{"type":"user","sessionId":"{CLAUDE_ID}","cwd":"/srv/project","timestamp":"2026-10-29T08:00:00.000Z","message":{{"role":"user","content":"hi"}}}}"#,
                "\n"
            ),
            CLAUDE_ID = CLAUDE_ID
        ),
    );
    m.agents
        .relocate_session("claude", &SessionId::from(CLAUDE_ID), destination.to_str().unwrap())
        .await
        .expect("move");
    let found = m
        .agents
        .external()
        .list(1, Some(SessionId::from(CLAUDE_ID)), Default::default())
        .await
        .unwrap()
        .pop()
        .expect("session");
    assert_eq!(Path::new(&found.cwd), destination);
    let moved = m
        .agents
        .external()
        .file(&SessionId::from(CLAUDE_ID))
        .await
        .unwrap()
        .expect("transcript");
    assert!(moved.with_extension("").join("subagents/a.jsonl").exists());
    assert!(!transcript.exists());
    assert!(m.claude(CLAUDE_OTHER));
}

#[tokio::test]
async fn a_process_that_ends_on_the_request_is_not_waited_out() {
    let mut child = tokio::process::Command::new("sleep")
        .arg("60")
        .spawn()
        .unwrap();
    let pid = child.id().unwrap();

    // Not reaped yet, so it stays listed as a zombie: its exit, not its
    // disappearance, ends the wait.
    tokio::time::timeout(Duration::from_secs(2), end_process(pid))
        .await
        .expect("the wait ends when the process exits")
        .unwrap();

    assert!(!child.wait().await.unwrap().success());
}
