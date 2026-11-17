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
    store.save(&indexed("old", 1, true), &texts(&["Improve the worktree search bar"]), Progress::NEW)?;
    store.save(
        &indexed("split", 2, true),
        &texts(&["Add worktree support", "The agent polished the bar"]),
        Progress::NEW,
    )?;
    store.save(&indexed("partial", 3, true), &texts(&["worktree only"]), Progress::NEW)?;

    let found = store.find("  BAR Worktree bar  ")?;
    let sessions: Vec<&str> = found.iter().map(|hit| hit.session.as_str()).collect();
    assert_eq!(sessions, ["split", "old"]);
    assert_eq!(found[0].texts, ["The agent polished the bar", "Add worktree support"]);
    assert!(store.find("worktree z")?.is_empty());
    assert_eq!(store.find("wo bar")?.len(), 2, "short words match too");
    assert!(store.find("b")?.is_empty(), "one character finds nothing");
    Ok(())
}

#[test]
fn a_text_is_kept_once_and_rows_come_in_order() -> Result<()> {
    let mut store = Store::open(Path::new(":memory:"))?;
    store.save(&indexed("one", 1, false), &texts(&["first", "second"]), Progress::NEW)?;
    store.save(&indexed("one", 1, true), &texts(&["second", "third"]), Progress::NEW)?;

    let rows = store.rows_after(0, 10)?;
    let added: Vec<Option<&str>> = rows.iter().map(|row| row.text.as_deref()).collect();
    assert_eq!(added, [Some("first"), Some("second"), Some("third")]);
    assert_eq!(store.rows_after(rows[0].seq, 1)?, rows[1..2]);
    assert_eq!(store.newest()?, rows[2].seq);
    Ok(())
}

#[test]
fn a_session_that_left_is_reset_for_copies_and_read_again() -> Result<()> {
    let mut store = Store::open(Path::new(":memory:"))?;
    store.save(&indexed("kept", 1, true), &texts(&["kept prompt"]), Progress::NEW)?;
    store.save(&indexed("gone", 1, true), &texts(&["gone prompt"]), Progress::NEW)?;
    let before = store.newest()?;

    assert_eq!(store.keep_only(&BTreeSet::from(["kept".to_owned()]))?, 1);
    assert_eq!(store.keep_only(&BTreeSet::from(["kept".to_owned()]))?, 0, "once");

    let reset = store.rows_after(before, 10)?;
    assert_eq!(reset.len(), 1);
    assert_eq!((reset[0].session.as_str(), reset[0].text.as_deref()), ("gone", None));
    assert!(store.find("gone")?.is_empty());
    assert!(!store.current("gone", "1")?, "read again if it comes back");
    assert_eq!(store.progress("gone")?, Progress::NEW);
    assert!(store.current("kept", "1")?);

    let all: Vec<String> = store.rows_after(0, 10)?.into_iter().map(|row| row.session).collect();
    assert_eq!(all, ["kept", "gone"], "the old rows went with the reset");
    Ok(())
}

#[test]
fn a_walk_in_progress_is_kept_and_is_not_current() -> Result<()> {
    let mut store = Store::open(Path::new(":memory:"))?;
    let walking = Progress {
        done: 0,
        walk: Some(Walk { top: 600, low: 400 }),
    };
    store.save(&indexed("one", 5, false), &texts(&["Newest request"]), walking)?;
    assert_eq!(store.progress("one")?, walking);
    assert!(!store.current("one", "5")?);
    assert_eq!(store.find("newest")?.len(), 1, "saved page by page");

    let done = Progress {
        done: 600,
        walk: None,
    };
    store.save(&indexed("one", 5, true), &[], done)?;
    assert_eq!(store.progress("one")?, done);
    assert!(store.current("one", "5")?);
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
    node.save(&indexed("kept", 1, true), &texts(&["retry the upload"]), Progress::NEW)?;
    node.save(&indexed("gone", 1, true), &texts(&["retry the build"]), Progress::NEW)?;
    let epoch = node.epoch()?;

    let mut copy = IndexCopy::open(Path::new(":memory:"))?;
    let (_, after) = copy.position()?;
    copy.apply(&epoch, &node.rows_after(after, 1)?)?;
    let (_, after) = copy.position()?;
    copy.apply(&epoch, &node.rows_after(after, 10)?)?;
    assert_eq!(copy.find("retry")?.len(), 2);
    assert_eq!(copy.position()?, (epoch.clone(), node.newest()?));

    node.keep_only(&BTreeSet::from(["kept".to_owned()]))?;
    let (_, after) = copy.position()?;
    copy.apply(&epoch, &node.rows_after(after, 10)?)?;
    let found: Vec<String> = copy.find("retry")?.into_iter().map(|hit| hit.session).collect();
    assert_eq!(found, ["kept"]);

    copy.apply("another", &[])?;
    assert!(copy.find("retry")?.is_empty(), "another index starts over");
    assert_eq!(copy.position()?, ("another".to_owned(), 0));
    Ok(())
}
