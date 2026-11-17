use super::*;
use beep_proto::AgentStatus;
use std::time::{Duration, UNIX_EPOCH};

fn session(id: &str, machine: &str, activity: u64) -> MachineSession {
    MachineSession {
        reply: None,
        changes: None,
        id: id.into(),
        machine_id: machine.into(),
        machine_name: machine.into(),
        agent: "codex".into(),
        title: Some("Connection fixes".into()),
        cwd: "/srv/project".into(),
        status: AgentStatus::Ended {},
        doing: None,
        started_at: UNIX_EPOCH,
        last_activity: UNIX_EPOCH + Duration::from_secs(activity),
        external: false,
        question: None,
        subagents: 0,
        model: None,
        assistant: false,
    }
}

#[test]
fn prompt_substrings_survive_restart_and_stay_per_machine() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("search.sqlite");
    {
        let mut store = Store::open(&path)?;
        store.save(
            &session("a", "build-box", 1),
            &["Please add a retry button".into(), "Retry later".into()],
            Progress::NEW,
            true,
        )?;
        store.save(
            &session("a", "gpu-box", 2),
            &["RETRY the upload".into()],
            Progress::NEW,
            true,
        )?;
    }
    let store = Store::open(&path)?;
    let found = store.find("  ReTrY ", None)?;
    assert_eq!(
        found.len(),
        2,
        "one hit per session, including identical ids on different machines"
    );
    assert_eq!(found[0].session.machine_id, "gpu-box");
    assert_eq!(
        store.find("try but", None)?.len(),
        1,
        "substring across word boundaries"
    );
    assert_eq!(store.find("xy", None)?.len(), 0);
    assert_eq!(store.find("tr", None)?.len(), 2);
    let members = vec!["build-box".to_owned()];
    let found = store.find("retry", Some(&members))?;
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].session.machine_id, "build-box");
    Ok(())
}

#[test]
fn literal_punctuation_unicode_and_replacements() -> Result<()> {
    let mut store = Store::open(Path::new(":memory:"))?;
    let session = session("a", "build-box", 1);
    store.save(
        &session,
        &["Find \"quoted\" text with 100% and __ plus café 東京".into()],
        Progress::NEW,
        true,
    )?;

    for query in ["\"quoted\"", "100%", "__", "café", "東京", "text with"] {
        assert_eq!(store.find(query, None)?.len(), 1, "{query}");
    }

    assert!(store.find("' OR 1=1", None)?.is_empty());
    assert!(store.current(&identity(&session), &stamp(&session)?)?);
    store.save(
        &session,
        &["A different prompt".into()],
        Progress::NEW,
        true,
    )?;
    assert_eq!(store.find("quoted", None)?.len(), 1, "later pages only add");
    assert_eq!(store.find("different", None)?.len(), 1);
    store.remove(&identity(&session))?;
    assert!(store.find("different", None)?.is_empty());
    store.save(
        &session,
        &["A late indexing result".into()],
        Progress::NEW,
        true,
    )?;
    assert!(
        store.find("indexing", None)?.is_empty(),
        "deletion wins over an indexing task already in flight"
    );
    Ok(())
}

#[test]
fn a_compacted_transcript_keeps_previously_indexed_prompts() -> Result<()> {
    let mut store = Store::open(Path::new(":memory:"))?;
    let session = session("a", "build-box", 1);
    store.save(&session, &["Original request".into()], Progress::NEW, true)?;
    store.save(
        &session,
        &["Follow up request".into()],
        Progress::NEW,
        false,
    )?;
    assert_eq!(store.find("Original", None)?.len(), 1);
    assert_eq!(store.find("Follow up", None)?.len(), 1);
    Ok(())
}

fn project(name: &str, machine: &str, folder: &str) -> ImportedProject {
    ImportedProject {
        project: name.into(),
        machine_id: machine.into(),
        machine_name: machine.into(),
        folder: folder.into(),
        url: format!("https://example.com/{name}"),
        cloned: false,
        imported_at: UNIX_EPOCH,
        display_name: None,
    }
}

fn count(store: &Store, table: &str) -> Result<i64> {
    Ok(store
        .0
        .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
            row.get(0)
        })?)
}

#[test]
fn accepted_prompts_are_immediate_durable_and_survive_an_older_scan() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("search.sqlite");
    let session = session("a", "build-box", 1);
    {
        let mut writer = Store::open(&path)?;
        let reader = Store::open(&path)?;
        writer.append(&session, "Add a retry button")?;
        assert_eq!(reader.find("retry", None)?.len(), 1);
        writer.save(&session, &["Earlier request".into()], Progress::NEW, true)?;
        assert_eq!(
            reader.find("retry", None)?.len(),
            1,
            "a scan taken before the send cannot erase it"
        );
        writer.append(&session, "Add a retry button")?;
        writer.save(
            &session,
            &["Earlier request".into(), "Add a retry button".into()],
            Progress::NEW,
            true,
        )?;
        assert_eq!(
            count(&writer, "prompt_texts")?,
            2,
            "the reconciled prompt is not duplicated"
        );
    }
    assert_eq!(Store::open(&path)?.find("retry", None)?.len(), 1);
    Ok(())
}

#[test]
fn project_deletion_cascades_and_stale_scans_cannot_restore_it() -> Result<()> {
    let mut store = Store::open(Path::new(":memory:"))?;
    let projects = vec![
        project("acme/widget", "build-box", "/srv/project"),
        project("acme/widget", "gpu-box", "C:\\work\\widget"),
    ];
    store.sync_projects(&projects)?;
    let build = session("a", "build-box", 1);
    let mut gpu = session("a", "gpu-box", 1);
    gpu.cwd = "C:\\work\\widget\\.worktrees\\fix".into();
    store.append(&build, "retry button")?;
    store.append(&gpu, "retry upload")?;
    store.remove_project("acme/widget", &projects)?;

    for table in ["projects", "places", "sessions", "prompt_texts"] {
        assert_eq!(count(&store, table)?, 0, "{table}");
    }

    assert!(
        store.find("retry", None)?.is_empty(),
        "FTS entries are deleted too"
    );
    store.save(
        &build,
        &["a stale retry result".into()],
        Progress::NEW,
        true,
    )?;
    assert!(!store.append(&gpu, "retry again")?);
    let new = session("never-indexed", "build-box", 2);
    store.save(&new, &["new stale retry".into()], Progress::NEW, true)?;
    assert!(store.find("retry", None)?.is_empty());
    let violations: i64 =
        store
            .0
            .query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |row| {
                row.get(0)
            })?;
    assert_eq!(violations, 0);
    assert!(
        store
            .0
            .execute(
                "INSERT INTO prompt_texts(session,text,words) VALUES('missing','orphan','orphan')",
                []
            )
            .is_err()
    );
    assert!(
        store
            .0
            .execute(
                "INSERT INTO sessions(id,stamp,data,activity,place) VALUES('orphan','','{}',0,999)",
                []
            )
            .is_err()
    );
    store.0.execute(
        "INSERT INTO prompt_search(prompt_search) VALUES('integrity-check')",
        [],
    )?;
    store.sync_projects(&projects)?;
    assert!(
        store.append(&build, "retry after reimport")?,
        "an explicit reimport makes the location searchable again"
    );
    Ok(())
}

#[test]
fn removing_one_location_preserves_the_other_and_unrelated_folders() -> Result<()> {
    let mut store = Store::open(Path::new(":memory:"))?;
    let projects = vec![
        project("acme/widget", "build-box", "/srv/project"),
        project("acme/widget", "gpu-box", "/srv/project"),
    ];
    let build = session("a", "build-box", 1);
    let gpu = session("a", "gpu-box", 1);
    let mut other = session("other", "build-box", 1);
    other.cwd = "/srv/project-other".into();
    store.append(&build, "retry build")?;
    store.append(&gpu, "retry upload")?;
    store.append(&other, "retry elsewhere")?;
    store.sync_projects(&projects)?;
    store.sync_projects(&projects[1..])?;
    let found = store.find("retry", None)?;
    assert_eq!(found.len(), 2);
    assert!(
        found
            .iter()
            .all(|f| f.session.machine_id == "gpu-box" || f.session.id == "other")
    );
    assert!(!store.append(&build, "late retry")?);
    Ok(())
}

#[test]
fn a_removed_nested_project_does_not_fall_back_to_its_parent() -> Result<()> {
    let mut store = Store::open(Path::new(":memory:"))?;
    let projects = vec![
        project("acme/parent", "build-box", "/srv"),
        project("acme/widget", "build-box", "/srv/project"),
    ];
    store.sync_projects(&projects)?;
    let child = session("a", "build-box", 1);
    store.append(&child, "nested request")?;
    store.sync_projects(&projects[..1])?;
    assert!(!store.append(&child, "must stay removed")?);
    assert_eq!(count(&store, "projects")?, 1);
    assert_eq!(count(&store, "sessions")?, 0);
    Ok(())
}

#[test]
fn the_previous_disk_index_is_migrated_and_acquires_project_ownership() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("search.sqlite");
    let session = session("a", "build-box", 1);
    {
        let db = Connection::open(&path)?;
        db.execute_batch("CREATE TABLE sessions(id TEXT PRIMARY KEY,stamp TEXT,data TEXT,activity INTEGER); CREATE TABLE removed(id TEXT PRIMARY KEY); CREATE VIRTUAL TABLE prompts USING fts5(session UNINDEXED,text UNINDEXED,words,tokenize='trigram');")?;
        db.execute(
            "INSERT INTO sessions VALUES(?1,?2,?3,0)",
            params![
                identity(&session),
                stamp(&session)?,
                serde_json::to_string(&session)?
            ],
        )?;
        db.execute(
            "INSERT INTO prompts VALUES(?1,'Original prompt','original prompt')",
            [identity(&session)],
        )?;
    }
    let mut store = Store::open(&path)?;
    assert_eq!(store.find("original", None)?.len(), 1);
    let projects = vec![project("acme/widget", "build-box", "/srv/project")];
    store.sync_projects(&projects)?;
    store.remove_project("acme/widget", &projects)?;
    assert!(store.find("original", None)?.is_empty());
    Ok(())
}

#[test]
fn unchanged_empty_sessions_are_also_indexed() -> Result<()> {
    let mut store = Store::open(Path::new(":memory:"))?;
    let session = session("a", "build-box", 1);
    store.save(&session, &[], Progress::NEW, true)?;
    assert!(store.current(&identity(&session), &stamp(&session)?)?);
    Ok(())
}

#[test]
fn terms_match_across_prompts_only_within_one_session() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let mut store = Store::open(&dir.path().join("search.sqlite"))?;
    store.save(
        &session("both", "build-box", 1),
        &["Improve the worktree search bar".into()],
        Progress::NEW,
        true,
    )?;
    store.save(
        &session("split", "build-box", 2),
        &["Add worktree support".into(), "Polish the bar".into()],
        Progress::NEW,
        true,
    )?;
    store.save(
        &session("partial", "build-box", 3),
        &["worktree only".into()],
        Progress::NEW,
        true,
    )?;
    let found = store.find("  BAR worktree bar  ", None)?;
    assert_eq!(found.len(), 2);
    assert_eq!(found[0].session.id, "split");
    assert!(found[0].texts.iter().any(|text| text.contains("worktree")));
    assert!(found[0].texts.iter().any(|text| text.contains("bar")));
    assert_eq!(store.find("worktree z", None)?.len(), 0);
    assert_eq!(store.find("wo bar", None)?.len(), 2);
    Ok(())
}

#[test]
fn a_walk_in_progress_is_kept_and_is_not_current() -> Result<()> {
    let mut store = Store::open(Path::new(":memory:"))?;
    let session = session("a", "build-box", 1);
    let id = identity(&session);
    assert_eq!(store.progress(&id)?, Progress::NEW);

    let walking = Progress {
        done: Some(0),
        walk: Some(Walk { top: 600, low: 400 }),
    };
    store.save(&session, &["Newest request".into()], walking, false)?;
    assert_eq!(store.progress(&id)?, walking);
    assert!(!store.current(&id, &stamp(&session)?)?);
    assert_eq!(store.find("newest", None)?.len(), 1, "saved page by page");

    let done = Progress {
        done: Some(600),
        walk: None,
    };
    store.save(&session, &[], done, true)?;
    assert_eq!(store.progress(&id)?, done);
    assert!(store.current(&id, &stamp(&session)?)?);
    Ok(())
}

#[test]
fn sessions_indexed_by_schema_one_keep_prompts_and_read_their_newest_page() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("search.sqlite");
    let indexed = session("a", "build-box", 1);
    let sent = session("b", "build-box", 1);
    {
        let mut store = Store::open(&path)?;
        store.save(&indexed, &["Original prompt".into()], Progress::NEW, true)?;
        store.append(&sent, "Sent from here")?;
        store.0.execute_batch(
            "ALTER TABLE sessions DROP COLUMN walk_low;
             ALTER TABLE sessions DROP COLUMN walk_top;
             ALTER TABLE sessions DROP COLUMN done;
             UPDATE sessions SET stamp='[\"old\",\"title\"]' WHERE stamp<>'';
             PRAGMA user_version=1;",
        )?;
    }
    let store = Store::open(&path)?;
    assert_eq!(store.find("original", None)?.len(), 1);
    assert_eq!(
        store.progress(&identity(&indexed))?,
        Progress {
            done: None,
            walk: None
        }
    );
    assert_eq!(store.progress(&identity(&sent))?, Progress::NEW);
    assert!(!store.current(&identity(&indexed), &stamp(&indexed)?)?);
    drop(store);
    assert_eq!(
        Store::open(&path)?.progress(&identity(&sent))?,
        Progress::NEW,
        "opening again changes nothing"
    );
    Ok(())
}
