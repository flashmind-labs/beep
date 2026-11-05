//! Deleting an ended session: the agent's own files and the node's record.

use super::{Agents, found::Agent};
use anyhow::{Context, Result, bail, ensure};
use beep_proto::node::agent::Status;
use tokio::task::spawn_blocking;

/// The longest session id accepted.
const MAX_ID: usize = 128;

/// An id that cannot name anything outside a store: letters, digits, `-`
/// and `_`, so no separator, no dot and no `..`.
fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_ID
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

impl Agents {
    /// Deletes session `id` of the harness `agent`, the agent's own id or
    /// this node's. It refuses one that runs or that a process has open,
    /// and the harnesses that keep sessions in a database. The agent's
    /// files go first, then the node's record of it, so a failure leaves
    /// the session listed. What it spent stays in the spend totals.
    pub(super) async fn delete_session(&self, agent: &str, id: &str) -> Result<()> {
        let which =
            Agent::from_name(agent).with_context(|| format!("this node has no agent {agent}"))?;
        if matches!(which, Agent::Flsh | Agent::OpenCode) {
            bail!("{agent} keeps its sessions in a database, which Beep does not edit");
        }
        ensure!(valid_id(id), "{id:?} is not a session id");
        let agent_id = match self.get(id) {
            Some(ours) => {
                ensure!(ours.agent == agent, "session {id} runs {}", ours.agent);
                ours.live.borrow().agent_session.clone()
            }
            None => Some(id.to_owned()),
        };
        if let Some(agent_id) = &agent_id {
            ensure!(valid_id(agent_id), "{agent_id:?} is not a session id");
        }
        let records: Vec<(String, bool)> = self
            .lock()
            .iter()
            .filter(|(key, session)| {
                session.agent == agent
                    && (key.as_str() == id
                        || agent_id.is_some() && session.live.borrow().agent_session == agent_id)
            })
            .map(|(key, session)| {
                let ended = session.live.borrow().status() == Status::Ended;
                (key.clone(), ended)
            })
            .collect();
        ensure!(
            records.iter().all(|(_, ended)| *ended),
            "session {id} still runs here; close it first"
        );
        let removed = match &agent_id {
            Some(agent_id) => {
                self.delete_files(which, agent_id, !records.is_empty())
                    .await?
            }
            None => false,
        };
        ensure!(
            removed || !records.is_empty(),
            "there is no {agent} session {id} on this machine"
        );
        let mut sessions = self.lock();
        for (key, _) in records {
            sessions.remove(&key);
        }
        Ok(())
    }

    /// Deletes `agent`'s files of its session `id` unless a process has
    /// them open; `false` when it has none. `ours` says this node ran the
    /// session and it ended, so a recent write is not a sign of life.
    async fn delete_files(&self, agent: Agent, id: &str, ours: bool) -> Result<bool> {
        let mut found = match agent {
            Agent::Claude => {
                let mut owned = self.owned();
                owned.remove(id);
                self.external
                    .list(1, Some(id.to_owned()), owned)
                    .await?
                    .pop()
            }
            other => {
                let stores = self.stores.clone();
                let find = id.to_owned();
                spawn_blocking(move || stores.find(other, &find)).await?
            }
        };
        if let Some(found) = &mut found {
            // Deleting cannot be undone: for Claude Code a recent write still
            // counts, in case a process Beep cannot see is writing it.
            if agent != Agent::Claude {
                found.presence = self.presence(agent, found).await;
            }
            let live = found.pid.is_some() || (found.presence.is_live() && !ours);
            if live {
                bail!(
                    "session {id} is open in a terminal or IDE (it is {}). \
                     Exit it there, then delete it.",
                    found.presence
                );
            }
        }
        match agent {
            Agent::Claude => self.external.delete(id).await,
            other => {
                let stores = self.stores.clone();
                let id = id.to_owned();
                spawn_blocking(move || stores.delete(other, &id)).await?
            }
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::agents::{AgentKind, Begin, claude::External, found::Stores};
    use std::{
        collections::BTreeMap,
        fs::{self, File},
        path::{Path, PathBuf},
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
                kinds: vec![kind("claude"), kind("codex"), kind("pi")],
                external: External::at(&root.join("claude")),
                stores: Stores::at(Some(&root.join("codex")), Some(&root.join("pi")), None),
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
    async fn each_harness_deletes_only_its_own_session() {
        let m = Machine::new();
        m.agents.delete_session("claude", CLAUDE_ID).await.unwrap();
        assert!(!m.claude(CLAUDE_ID) && !m.side(CLAUDE_ID));
        assert!(m.claude(CLAUDE_OTHER) && m.side(CLAUDE_OTHER));
        assert!(m.codex(CODEX_ID) && m.pi("p1"));

        m.agents.delete_session("codex", CODEX_ID).await.unwrap();
        assert!(!m.codex(CODEX_ID) && m.codex(CODEX_OTHER));

        m.agents.delete_session("pi", "p1").await.unwrap();
        assert!(!m.pi("p1") && m.pi("p2"));
        assert!(m.claude(CLAUDE_OTHER) && m.codex(CODEX_OTHER));
        assert!(m.has("outside.jsonl"));

        let err = m.agents.delete_session("pi", "p1").await.unwrap_err();
        assert!(err.to_string().contains("no pi session"), "{err}");
    }

    #[tokio::test]
    async fn a_deleted_session_is_no_longer_listed() {
        let m = Machine::new();
        let listed = |m: &Machine| {
            let stores = m.agents.stores.clone();
            stores.find(Agent::Codex, CODEX_ID).is_some()
        };
        assert!(listed(&m));
        m.agents.delete_session("codex", CODEX_ID).await.unwrap();
        assert!(!listed(&m));
        m.agents.delete_session("claude", CLAUDE_ID).await.unwrap();
        let left = m
            .agents
            .external
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
            resume: Some(CLAUDE_ID.to_owned()),
            ..Begin::default()
        };
        let session = m
            .agents
            .start(&kind("claude"), Some("/"), begin)
            .expect("a session");
        for id in [session.id.as_str(), CLAUDE_ID] {
            let err = m.agents.delete_session("claude", id).await.unwrap_err();
            assert!(err.to_string().contains("still runs"), "{err}");
        }
        assert!(m.claude(CLAUDE_ID), "the transcript stays");
        assert_eq!(m.agents.list().len(), 1);

        session.close().await;
        m.agents
            .delete_session("claude", &session.id)
            .await
            .unwrap();
        assert!(!m.claude(CLAUDE_ID) && !m.side(CLAUDE_ID));
        assert!(m.agents.list().is_empty(), "the node's record is gone");
        assert!(m.agents.open_sessions().is_empty(), "and not reopened");
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
            .delete_session("claude", CLAUDE_ID)
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
                let err = m.agents.delete_session(agent, id).await.unwrap_err();
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
        assert!(crate::agents::found::remove_file_in(&store, &outside).is_err());
        let link = store.join("link.jsonl");
        std::os::unix::fs::symlink(&outside, &link).expect("a link");
        assert!(crate::agents::found::remove_file_in(&store, &link).is_err());
        let folder = store.join("folder-link");
        std::os::unix::fs::symlink(dir.path(), &folder).expect("a folder link");
        crate::agents::found::remove_dir_in(&store, &folder).expect("a link is skipped");
        assert!(outside.exists() && link.exists());
    }

    #[tokio::test]
    async fn sessions_kept_in_a_database_are_refused() {
        let m = Machine::new();
        for agent in ["flsh", "opencode"] {
            let err = m.agents.delete_session(agent, "ses_1").await.unwrap_err();
            assert!(err.to_string().contains("database"), "{err}");
        }
        let err = m
            .agents
            .delete_session("gemini", "ses_1")
            .await
            .unwrap_err();
        assert!(err.to_string().contains("no agent"), "{err}");
    }
}
