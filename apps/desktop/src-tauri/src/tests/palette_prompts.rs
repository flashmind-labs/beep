use super::*;
use beep_proto::{AgentStatus, node::agent::Entry};
use std::{collections::VecDeque, time::UNIX_EPOCH};

struct Node {
    pages: Mutex<VecDeque<SessionsReply>>,
    cursors: Mutex<Vec<Option<u64>>>,
}

impl Node {
    fn new(pages: impl IntoIterator<Item = SessionsReply>) -> Self {
        Self {
            pages: Mutex::new(pages.into_iter().collect()),
            cursors: Mutex::default(),
        }
    }

    fn cursors(&self) -> Vec<Option<u64>> {
        std::mem::take(&mut self.cursors.lock().expect("test lock"))
    }
}

impl Sessions for Node {
    async fn sessions(
        &self,
        request: SessionsRequest,
        _: Reading,
        _: Duration,
    ) -> std::result::Result<SessionsReply, beep_mesh::MeshError> {
        let SessionsRequest::Transcript {
            before, thoughts, ..
        } = request
        else {
            panic!("expected a transcript request")
        };
        assert!(!thoughts, "only the user's prompts are kept");
        self.cursors.lock().expect("test lock").push(before);
        Ok(self
            .pages
            .lock()
            .expect("test lock")
            .pop_front()
            .expect("one request per page"))
    }
}

/// A page of `entries` entries after `earlier`, the first a prompt.
fn page(earlier: u32, entries: usize, prompt: &str) -> SessionsReply {
    let mut page = vec![Entry::new(EntryKind::User, prompt.into())];
    page.resize_with(entries, || Entry::new(EntryKind::Agent, "output".into()));
    SessionsReply::Transcript {
        entries: page,
        earlier,
    }
}

fn session() -> MachineSession {
    MachineSession {
        id: "one".into(),
        machine_id: "build-box".into(),
        machine_name: "build-box".into(),
        agent: "codex".into(),
        title: None,
        cwd: "/srv/project".into(),
        status: AgentStatus::Idle {},
        doing: None,
        started_at: UNIX_EPOCH,
        last_activity: UNIX_EPOCH + Duration::from_secs(1),
        external: false,
        question: None,
        subagents: 0,
        model: None,
    }
}

fn found(index: &PromptIndex, query: &str) -> anyhow::Result<usize> {
    Ok(index.store().find(query, None)?.len())
}

#[tokio::test]
async fn reads_earlier_prompts_and_excludes_context_and_agent_output() -> anyhow::Result<()> {
    let node = Node::new([
        SessionsReply::Transcript {
            earlier: 2,
            entries: vec![
                Entry::new(
                    EntryKind::User,
                    "<beep-context>hidden instructions</beep-context>add retries".into(),
                ),
                Entry::new(EntryKind::Agent, "assistant output".into()),
            ],
        },
        SessionsReply::Transcript {
            earlier: 0,
            entries: vec![Entry::new(EntryKind::User, "original request".into())],
        },
    ]);
    let index = Arc::new(PromptIndex::open(Path::new(":memory:"))?);
    index_new(&node, &index, &session(), &AtomicBool::new(false)).await?;
    assert_eq!(node.cursors(), [None, Some(2)]);
    assert_eq!(found(&index, "add retries")?, 1);
    assert_eq!(found(&index, "original")?, 1);
    assert_eq!(found(&index, "hidden")?, 0);
    assert_eq!(found(&index, "assistant")?, 0);
    Ok(())
}

#[tokio::test]
async fn a_session_is_read_again_only_from_its_new_entries() -> anyhow::Result<()> {
    let index = Arc::new(PromptIndex::open(Path::new(":memory:"))?);
    let mut session = session();
    let node = Node::new([
        page(400, 200, "third"),
        page(200, 200, "second"),
        page(0, 200, "first"),
    ]);
    index_new(&node, &index, &session, &AtomicBool::new(false)).await?;
    assert_eq!(node.cursors(), [None, Some(400), Some(200)]);

    // 50 new entries: the newest page reaches what was read.
    session.last_activity += Duration::from_secs(60);
    let node = Node::new([page(450, 200, "fourth")]);
    index_new(&node, &index, &session, &AtomicBool::new(false)).await?;
    assert_eq!(node.cursors(), [None]);

    // 300 more: one page back, not to the start.
    session.last_activity += Duration::from_secs(60);
    let node = Node::new([page(750, 200, "sixth"), page(550, 200, "fifth")]);
    index_new(&node, &index, &session, &AtomicBool::new(false)).await?;
    assert_eq!(node.cursors(), [None, Some(750)]);

    for prompt in ["first", "second", "third", "fourth", "fifth", "sixth"] {
        assert_eq!(found(&index, prompt)?, 1, "{prompt}");
    }

    let id = identity(&session);
    assert!(index.store().current(&id, &stamp(&session)?)?);
    Ok(())
}

#[tokio::test]
async fn an_interrupted_read_keeps_its_pages_and_resumes() -> anyhow::Result<()> {
    let index = Arc::new(PromptIndex::open(Path::new(":memory:"))?);
    let session = session();
    let saved = AtomicBool::new(false);
    let node = Node::new([
        page(400, 200, "third"),
        SessionsReply::Error {
            message: "the link went away".into(),
        },
    ]);
    assert!(index_new(&node, &index, &session, &saved).await.is_err());
    assert!(saved.load(Ordering::Relaxed), "a page was saved");
    assert_eq!(found(&index, "third")?, 1);
    assert!(
        !index
            .store()
            .current(&identity(&session), &stamp(&session)?)?
    );

    // The walk goes on where it stopped, then reads the newest page.
    let node = Node::new([
        page(200, 200, "second"),
        page(0, 200, "first"),
        page(450, 200, "fourth"),
    ]);
    index_new(&node, &index, &session, &AtomicBool::new(false)).await?;
    assert_eq!(node.cursors(), [Some(400), Some(200), None]);
    assert_eq!(found(&index, "first")?, 1);
    assert_eq!(found(&index, "fourth")?, 1);
    assert!(
        index
            .store()
            .current(&identity(&session), &stamp(&session)?)?
    );
    Ok(())
}

#[test]
fn a_session_from_the_full_reader_reads_only_its_newest_page() {
    let legacy = Progress {
        done: None,
        walk: None,
    };
    let page = Page {
        earlier: 4000,
        entries: 200,
    };
    assert_eq!(
        advance(legacy, page),
        Progress {
            done: Some(4200),
            walk: None
        }
    );
}

#[test]
fn old_nodes_and_dropped_entries_end_a_walk() {
    let walking = Progress {
        done: Some(100),
        walk: Some(Walk { top: 600, low: 400 }),
    };
    let finished = Progress {
        done: Some(600),
        walk: None,
    };
    // A node that ignores `before` answers with its newest page.
    let newest = Page {
        earlier: 450,
        entries: 200,
    };
    assert_eq!(advance(walking, newest), finished);
    // Past dropped entries the answer is empty.
    let dropped = Page {
        earlier: 400,
        entries: 0,
    };
    assert_eq!(advance(walking, dropped), finished);
    // Thoughts left out make a page short, which is fine.
    let short = Page {
        earlier: 200,
        entries: 150,
    };
    assert_eq!(
        advance(walking, short),
        Progress {
            done: Some(100),
            walk: Some(Walk { top: 600, low: 200 }),
        }
    );
    assert!(!valid_page(
        400,
        Page {
            earlier: 250,
            entries: 200
        }
    ));
}

#[test]
fn a_broken_index_does_not_stop_the_app() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("search.sqlite");
    std::fs::write(&path, b"broken cache")?;
    let index = PromptIndex::open(&path)?;
    assert!(index.reader().find("retry", None)?.is_empty());
    Ok(())
}

#[test]
fn excerpt_includes_a_late_match() {
    let text = format!(
        "{} Add a retry button. {}",
        "earlier ".repeat(200),
        "later ".repeat(200)
    );
    let found = excerpt(&text, "RETRY");
    assert!(found.contains("retry button"));
    assert!(found.starts_with('…') && found.ends_with('…'));
    assert!(found.len() < 180);
    assert_eq!(excerpt("修正 接続を再試行", "再試行"), "修正 接続を再試行");
}
