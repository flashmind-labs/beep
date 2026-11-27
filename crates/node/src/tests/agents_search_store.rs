use super::*;

fn indexed(id: &str, activity: u64, current: bool) -> Indexed {
    Indexed {
        id: id.into(),
        activity,
        stamp: current.then(|| activity.to_string()),
    }
}

fn texts(of: &[&str]) -> Vec<String> {
    of.iter().map(|text| (*text).to_owned()).collect()
}

#[test]
fn every_word_must_be_in_one_session_newest_first() -> Result<()> {
    let mut store = Store::open(Path::new(":memory:"))?;
    store.save(
        &indexed("old", 1, true),
        &texts(&["Improve the worktree search bar"]),
        Progress::NEW,
    )?;
    store.save(
        &indexed("split", 2, true),
        &texts(&["Add worktree support", "The agent polished the bar"]),
        Progress::NEW,
    )?;
    store.save(
        &indexed("partial", 3, true),
        &texts(&["worktree only"]),
        Progress::NEW,
    )?;

    let found = store.find("  BAR Worktree bar  ", &[])?;
    let sessions: Vec<&str> = found.iter().map(|hit| hit.session.as_ref()).collect();
    assert_eq!(sessions, ["split", "old"]);
    assert_eq!(
        found[0].texts,
        ["The agent polished the bar", "Add worktree support"]
    );
    assert!(store.find("worktree z", &[])?.is_empty());
    assert_eq!(store.find("wo bar", &[])?.len(), 2, "short words match too");
    assert!(
        store.find("b", &[])?.is_empty(),
        "one character finds nothing"
    );
    Ok(())
}

#[test]
fn a_text_is_kept_once_and_rows_come_in_order() -> Result<()> {
    let mut store = Store::open(Path::new(":memory:"))?;
    store.save(
        &indexed("one", 1, false),
        &texts(&["first", "second"]),
        Progress::NEW,
    )?;
    store.save(
        &indexed("one", 1, true),
        &texts(&["second", "third"]),
        Progress::NEW,
    )?;

    let rows = store.rows_after(0, 10)?;
    let added: Vec<Option<&str>> = rows.iter().map(|row| row.text.as_deref()).collect();
    assert_eq!(added, [Some("first"), Some("second"), Some("third")]);
    assert_eq!(store.rows_after(rows[0].seq, 1)?, rows[1..2]);
    assert_eq!(store.newest()?, rows[2].seq);
    Ok(())
}

#[test]
fn deleting_a_session_resets_its_text_and_progress() -> Result<()> {
    let mut store = Store::open(Path::new(":memory:"))?;
    store.save(
        &indexed("kept", 1, true),
        &texts(&["kept prompt"]),
        Progress::NEW,
    )?;
    let gone = indexed("gone", 1, true);
    store.save(&gone, &texts(&["gone prompt"]), Progress::NEW)?;
    let before = store.newest()?;
    store.forget(&gone.id)?;

    let reset = store.rows_after(before, 10)?;
    assert_eq!(reset.len(), 1);
    assert_eq!(
        (reset[0].session.as_ref(), reset[0].text.as_deref()),
        ("gone", None)
    );
    assert!(store.find("gone", &[])?.is_empty());
    assert!(
        !store.current(&SessionId::from("gone"), "1")?,
        "read again if it comes back"
    );
    assert_eq!(store.progress(&SessionId::from("gone"))?, Progress::NEW);
    assert!(store.current(&SessionId::from("kept"), "1")?);

    let all: Vec<String> = store
        .rows_after(0, 10)?
        .into_iter()
        .map(|row| String::from(row.session))
        .collect();
    assert_eq!(all, ["kept", "gone"], "the old rows went with the reset");

    store.forget(&SessionId::from("kept"))?;
    assert!(
        store.find("kept", &[])?.is_empty(),
        "a deleted session goes"
    );
    Ok(())
}

#[test]
fn existing_text_gets_openable_metadata_without_a_transcript_reread() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("search.sqlite");
    let mut store = Store::open(&path)?;
    let session = indexed("archived", 1000, true);
    store.save(&session, &texts(&["retry in the archive"]), Progress::NEW)?;
    let old_epoch = store.epoch()?;
    // The previous schema had no metadata but did hold the text and its cursor.
    store
        .0
        .execute_batch("ALTER TABLE sessions DROP COLUMN metadata; PRAGMA user_version=1;")?;
    drop(store);

    let mut node = Store::open(&path)?;
    assert_ne!(node.epoch()?, old_epoch);
    assert!(node.current(&session.id, "1000")?);
    let mut copy = IndexCopy::open(Path::new(":memory:"))?;
    let epoch = node.epoch()?;
    copy.apply(&epoch, &node.rows_after(0, 10)?)?;
    assert!(copy.find("retry", &[])?[0].metadata.is_none());

    let mut metadata = SearchSession {
        agent: "codex".into(),
        cwd: "/path/to/unimported".into(),
        title: Some("Retry".into()),
        started_at: 1,
        managed: None,
    };
    let (_, after) = copy.position()?;
    assert!(node.set_metadata(&session.id, session.activity, &metadata)?);
    assert!(!node.set_metadata(&session.id, session.activity, &metadata)?);
    let rows = node.rows_after(after, 10)?;
    assert_eq!(rows.len(), 1);
    copy.apply(&epoch, &rows)?;
    assert_eq!(
        node.find("retry", &[])?[0].metadata.as_ref(),
        Some(&metadata)
    );
    assert_eq!(
        copy.find("retry", &[])?[0].metadata.as_ref(),
        Some(&metadata)
    );

    let (_, after) = copy.position()?;
    metadata.title = Some("Revised title".into());
    assert!(node.set_metadata(&session.id, session.activity, &metadata)?);
    copy.apply(&epoch, &node.rows_after(after, 10)?)?;
    copy.apply(&epoch, &rows)?;
    assert_eq!(
        copy.find("retry", &[])?[0].metadata.as_ref(),
        Some(&metadata)
    );
    assert_eq!(copy.find("title archive", &[])?.len(), 1);

    let (_, after) = copy.position()?;
    node.forget(&session.id)?;
    copy.apply(&epoch, &node.rows_after(after, 10)?)?;
    assert!(copy.find("retry", &[])?.is_empty());
    Ok(())
}

#[test]
fn words_are_literal_and_fold_every_capital() -> Result<()> {
    let mut store = Store::open(Path::new(":memory:"))?;
    store.save(
        &indexed("one", 1, true),
        &texts(&["Ärger im Build", "Привет мир", "call my_function"]),
        Progress::NEW,
    )?;
    store.save(
        &indexed("two", 2, true),
        &texts(&["call my function"]),
        Progress::NEW,
    )?;

    assert_eq!(store.find("ärger", &[])?.len(), 1);
    assert_eq!(store.find("ÄRGER build", &[])?.len(), 1);
    assert_eq!(store.find("привет", &[])?[0].texts, ["Привет мир"]);
    let underscored: Vec<String> = store
        .find("my_function", &[])?
        .into_iter()
        .map(|hit| String::from(hit.session))
        .collect();
    assert_eq!(underscored, ["one"], "_ is not a wildcard");
    assert!(store.find("__", &[])?.is_empty());
    assert!(store.find("%%", &[])?.is_empty());
    assert!(
        store.find("\"call", &[])?.is_empty(),
        "quotes are taken as written"
    );
    Ok(())
}

#[test]
fn a_search_can_keep_to_named_sessions() -> Result<()> {
    let mut store = Store::open(Path::new(":memory:"))?;
    store.save(
        &indexed("listed", 1, true),
        &texts(&["retry"]),
        Progress::NEW,
    )?;
    store.save(&indexed("past", 2, true), &texts(&["retry"]), Progress::NEW)?;

    let hits: Vec<String> = store
        .find("retry", &[SessionId::from("listed")])?
        .into_iter()
        .map(|hit| String::from(hit.session))
        .collect();
    assert_eq!(hits, ["listed"]);
    assert_eq!(store.find("retry", &[])?.len(), 2, "no names, any session");
    Ok(())
}

#[test]
fn a_walk_in_progress_is_kept_and_is_not_current() -> Result<()> {
    let mut store = Store::open(Path::new(":memory:"))?;
    let walking = Progress {
        done: 0,
        walk: Some(Walk { top: 600, low: 400 }),
    };
    store.save(
        &indexed("one", 5, false),
        &texts(&["Newest request"]),
        walking,
    )?;
    assert_eq!(store.progress(&SessionId::from("one"))?, walking);
    assert!(!store.current(&SessionId::from("one"), "5")?);
    assert_eq!(store.find("newest", &[])?.len(), 1, "saved page by page");

    let done = Progress {
        done: 600,
        walk: None,
    };
    store.save(&indexed("one", 5, true), &[], done)?;
    assert_eq!(store.progress(&SessionId::from("one"))?, done);
    assert!(store.current(&SessionId::from("one"), "5")?);
    Ok(())
}

#[test]
fn a_file_keeps_its_epoch_and_a_new_file_gets_another() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("search.sqlite");
    let epoch = Store::open(&path)?.epoch()?;
    assert_eq!(Store::open(&path)?.epoch()?, epoch);

    let other = Store::open(&dir.path().join("other.sqlite"))?.epoch()?;
    assert_ne!(other, epoch);
    let vacuum: i64 = Store::open(&path)?
        .0
        .pragma_query_value(None, "auto_vacuum", |row| row.get(0))?;
    assert_eq!(vacuum, 2, "incremental");
    Ok(())
}

#[test]
fn a_copy_follows_rows_resets_and_a_new_epoch() -> Result<()> {
    let mut node = Store::open(Path::new(":memory:"))?;
    node.save(
        &indexed("gone", 1, true),
        &texts(&["retry the build"]),
        Progress::NEW,
    )?;
    node.save(
        &indexed("kept", 2, true),
        &texts(&["retry the upload"]),
        Progress::NEW,
    )?;
    let epoch = node.epoch()?;

    let mut copy = IndexCopy::open(Path::new(":memory:"))?;
    let (_, after) = copy.position()?;
    copy.apply(&epoch, &node.rows_after(after, 1)?)?;
    let (_, after) = copy.position()?;
    copy.apply(&epoch, &node.rows_after(after, 10)?)?;
    let order: Vec<String> = copy
        .find("retry", &[])?
        .into_iter()
        .map(|hit| String::from(hit.session))
        .collect();
    assert_eq!(order, ["kept", "gone"], "newest activity first");
    assert_eq!(copy.position()?, (epoch.clone(), node.newest()?));

    node.forget(&SessionId::from("gone"))?;
    let (_, after) = copy.position()?;
    copy.apply(&epoch, &node.rows_after(after, 10)?)?;
    let found: Vec<String> = copy
        .find("retry", &[])?
        .into_iter()
        .map(|hit| String::from(hit.session))
        .collect();
    assert_eq!(found, ["kept"]);

    copy.apply("another", &[])?;
    assert!(
        copy.find("retry", &[])?.is_empty(),
        "another index starts over"
    );
    assert_eq!(copy.position()?, ("another".to_owned(), 0));
    Ok(())
}

#[test]
fn a_hit_carries_only_the_text_around_its_word() -> Result<()> {
    let mut store = Store::open(Path::new(":memory:"))?;
    let long = format!(
        "{}İstanbul retry button{}",
        "a ".repeat(2000),
        " b".repeat(2000)
    );
    store.save(&indexed("long", 1, true), &texts(&[&long]), Progress::NEW)?;

    let texts = &store.find("retry", &[])?[0].texts;
    assert!(texts[0].contains("İstanbul retry button"));
    assert!(texts[0].chars().count() <= 2 * AROUND + "retry".len());
    assert_eq!(folded_at("İstanbul retry", "retry"), Some(9));
    Ok(())
}

#[test]
fn a_batch_of_rows_ends_once_it_carries_enough_text() -> Result<()> {
    let mut store = Store::open(Path::new(":memory:"))?;
    let big: Vec<String> = (0..3)
        .map(|n| format!("{n}{}", "x".repeat(BATCH_BYTES / 2)))
        .collect();
    store.save(&indexed("big", 1, true), &big, Progress::NEW)?;

    assert_eq!(store.rows_after(0, 500)?.len(), 2);
    Ok(())
}

#[test]
fn session_lookup_finds_by_agent_id_and_managed_id() -> Result<()> {
    let mut node = Store::open(Path::new(":memory:"))?;
    let meta = SearchSession {
        agent: "flsh".into(),
        cwd: "/path/to/work".into(),
        title: Some("Migration work".into()),
        started_at: 100,
        managed: Some("m12345".into()),
    };
    node.set_metadata(&SessionId::from("session_deadbeef"), 2000, &meta)?;
    let epoch = node.epoch()?;

    let mut copy = IndexCopy::open(Path::new(":memory:"))?;
    copy.apply(&epoch, &node.rows_after(0, 10)?)?;

    assert_eq!(
        copy.session(&SessionId::from("session_deadbeef"))?,
        Some((SessionId::from("session_deadbeef"), 2000, meta.clone()))
    );
    assert_eq!(
        copy.session(&SessionId::from("m12345"))?,
        Some((SessionId::from("session_deadbeef"), 2000, meta))
    );
    assert_eq!(copy.session(&SessionId::from("missing"))?, None);
    Ok(())
}
