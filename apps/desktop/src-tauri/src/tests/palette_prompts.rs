use super::*;

struct Node {
    pages: Mutex<std::collections::VecDeque<SessionsReply>>,
    cursors: Mutex<Vec<Option<u64>>>,
}

impl Sessions for Node {
    async fn sessions(
        &self,
        request: SessionsRequest,
        _: Duration,
    ) -> std::result::Result<SessionsReply, beep_mesh::MeshError> {
        let SessionsRequest::Transcript {
            before, thoughts, ..
        } = request
        else {
            panic!("expected a transcript request")
        };
        assert!(thoughts, "pagination counts every entry");
        self.cursors.lock().expect("test lock").push(before);
        Ok(self
            .pages
            .lock()
            .expect("test lock")
            .pop_front()
            .expect("one request per page"))
    }
}

#[tokio::test]
async fn reads_earlier_prompts_and_excludes_context_and_agent_output() -> anyhow::Result<()> {
    use beep_proto::node::agent::Entry;
    let node = Node {
        pages: Mutex::new(
            [
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
                    entries: vec![
                        Entry::new(EntryKind::User, "original request".into()),
                        Entry::new(EntryKind::Thought, "reasoning".into()),
                    ],
                },
            ]
            .into(),
        ),
        cursors: Mutex::default(),
    };
    let read = read_prompts(&node, "a").await?;
    assert_eq!(read.texts, ["add retries", "original request"]);
    assert!(read.complete);
    assert_eq!(*node.cursors.lock().expect("test lock"), [None, Some(2)]);
    Ok(())
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
fn old_nodes_and_dropped_transcripts_cannot_repeat_a_page() {
    assert!(valid_page(None, 400, 200));
    assert!(valid_page(Some(400), 200, 200));
    assert!(!valid_page(Some(400), 400, 200));
    assert!(!valid_page(Some(400), 250, 200));
    assert!(!valid_page(Some(400), 400, 0));
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

#[tokio::test]
async fn a_session_found_current_is_skipped_until_it_changes_or_goes() -> anyhow::Result<()> {
    let index = Arc::new(PromptIndex::open(Path::new(":memory:"))?);
    index.known().insert("build-box one".into(), "stamp".into());
    assert!(index.is_known("build-box one", "stamp"));
    assert!(!index.is_known("build-box one", "newer"));

    index.remove("build-box one".into()).await;
    assert!(!index.is_known("build-box one", "stamp"));
    Ok(())
}
