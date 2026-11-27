use super::*;

#[tokio::test]
async fn an_archived_conversation_outside_projects_is_indexed_and_can_be_read() -> anyhow::Result<()>
{
    let root = tempfile::tempdir()?;
    let archive = root.path().join("archived_sessions");
    std::fs::create_dir(&archive)?;
    let header = serde_json::json!({
        "type": "session_meta", "payload": {
            "id": "archived-session", "cwd": "/path/to/unimported", "source": "cli"
        }
    });
    let message = serde_json::json!({
        "type": "event_msg", "payload": {"type": "user_message", "message": "retry the upgrade"}
    });
    std::fs::write(
        archive.join("rollout-test.jsonl"),
        format!("{header}\n{message}\n"),
    )?;
    let agents = Agents {
        occupancy: crate::occupancy::Occupancy::over(crate::logs::claude::Store::at(root.path()), crate::logs::found::Stores::at(Some(root.path()), None, None)),
        ..Agents::default()
    };
    let sessions = conversations(&agents).await;
    let [session] = sessions.as_slice() else {
        anyhow::bail!("expected the archived conversation");
    };
    let index = Arc::new(SearchIndex::open(&root.path().join("search.sqlite"))?);
    assert!(index_new(&agents, &index, session, &mut false).await?);
    let hits = index.find("retry".into(), Vec::new()).await?;
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].session, session.id);
    assert_eq!(hits[0].metadata.as_ref(), Some(&session.metadata));
    assert_eq!(hits[0].texts, ["retry the upgrade"]);
    Ok(())
}

#[test]
fn a_read_walks_back_from_the_newest_page_to_what_was_indexed() {
    let indexed = Progress {
        done: 100,
        walk: None,
    };
    let newest = Page {
        earlier: 400,
        entries: 200,
    };
    let walking = advance(indexed, newest);
    assert_eq!(
        walking,
        Progress {
            done: 100,
            walk: Some(Walk { top: 600, low: 400 }),
        }
    );

    let older = Page {
        earlier: 200,
        entries: 200,
    };
    let walking = advance(walking, older);
    assert_eq!(walking.walk, Some(Walk { top: 600, low: 200 }));

    let reaching = Page {
        earlier: 0,
        entries: 200,
    };
    assert_eq!(
        advance(walking, reaching),
        Progress {
            done: 600,
            walk: None
        }
    );
}

#[test]
fn a_newest_page_that_reaches_what_was_indexed_needs_no_walk() {
    let indexed = Progress {
        done: 500,
        walk: None,
    };
    let newest = Page {
        earlier: 450,
        entries: 200,
    };
    assert_eq!(
        advance(indexed, newest),
        Progress {
            done: 650,
            walk: None
        }
    );
}

#[test]
fn stores_that_ignore_before_and_dropped_entries_end_a_walk() {
    let walking = Progress {
        done: 100,
        walk: Some(Walk { top: 600, low: 400 }),
    };
    let finished = Progress {
        done: 600,
        walk: None,
    };
    // A store that cannot read before an entry answers with its newest page.
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
    assert!(!valid_page(
        400,
        Page {
            earlier: 250,
            entries: 200
        }
    ));
}

#[test]
fn a_transcript_numbered_again_is_read_again_from_the_start() {
    let indexed = Progress {
        done: 5000,
        walk: None,
    };
    // A large file read from a later point now has fewer entries.
    let newest = Page {
        earlier: 1800,
        entries: 200,
    };
    assert_eq!(
        advance(indexed, newest),
        Progress {
            done: 0,
            walk: Some(Walk {
                top: 2000,
                low: 1800
            }),
        }
    );
}

#[test]
fn a_store_write_wakes_a_pass_with_no_timer_behind_it() {
    assert!(reads_stores("agent"));
    assert!(reads_stores("agents"));
    assert!(!reads_stores("terminals"));
}
