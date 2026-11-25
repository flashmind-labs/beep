use super::*;
use crate::agents::claude::transcript::tests::FIXTURE;
use crate::files::paths;

const ID: &str = "0f3c2a9e-1111-4222-8333-944455556666";
const OTHER: &str = "7a1b2c3d-4e5f-4a6b-8c7d-8e9f0a1b2c3d";
static SEEN_ID: std::sync::LazyLock<SessionId> = std::sync::LazyLock::new(|| SessionId::from(ID));

fn seen(modified: SystemTime, newest: bool) -> Seen<'static> {
    Seen {
        id: &SEEN_ID,
        folder: "-srv-project",
        modified,
        newest_in_folder: newest,
    }
}

fn process(pid: u32, cwd: &str) -> Process {
    Process {
        pid,
        cwd: Some(PathBuf::from(cwd)),
        session: None,
    }
}

/// A session the daemon runs in the background: its process names the
/// session in its arguments and has no pid file.
#[test]
fn a_process_naming_the_session_runs_it_wherever_it_is() {
    let now = SystemTime::now();
    let old = now - Duration::from_secs(3600);
    let named = |pid: u32, cwd: &str, session: &str| Process {
        session: Some(SessionId::from(session)),
        ..process(pid, cwd)
    };
    let snapshot = Snapshot {
        processes: vec![named(8271, "/elsewhere", ID)],
        ..Snapshot::default()
    };
    assert_eq!(
        presence(&seen(old, true), &snapshot, now),
        Found {
            presence: Presence::Running,
            pid: Some(8271)
        }
    );
    // The arguments name the session that was resumed; after `/clear`
    // the process runs a newer one, so only the folder's newest counts.
    assert_eq!(
        presence(&seen(old, false), &snapshot, now).presence,
        Presence::Stopped
    );
    // One in the folder naming another, older session runs the newest.
    let other = Snapshot {
        processes: vec![named(8271, "/srv/project", OTHER)],
        ..Snapshot::default()
    };
    assert_eq!(
        presence(&seen(old, true), &other, now),
        Found {
            presence: Presence::Running,
            pid: Some(8271)
        }
    );
    // Its pid file, when it has one, says what it runs.
    let claimed = Snapshot {
        processes: vec![named(8271, "/srv/project", ID)],
        pid_files: vec![PidFile {
            pid: 8271,
            session_id: OTHER.into(),
            status: Some("busy".into()),
        }],
        ..Snapshot::default()
    };
    assert_eq!(
        presence(&seen(old, true), &claimed, now).presence,
        Presence::Stopped
    );
}

#[test]
fn a_pid_file_names_the_session_and_its_state() {
    let now = SystemTime::now();
    let snapshot = Snapshot {
        processes: vec![process(41, "/elsewhere")],
        pid_files: vec![PidFile {
            pid: 41,
            session_id: ID.into(),
            status: Some("busy".into()),
        }],
        ..Snapshot::default()
    };
    let old = now - Duration::from_secs(3600);
    assert_eq!(
        presence(&seen(old, false), &snapshot, now),
        Found {
            presence: Presence::Working,
            pid: Some(41)
        }
    );
}

#[test]
fn a_stale_pid_file_does_not_count() {
    let now = SystemTime::now();
    let snapshot = Snapshot {
        pid_files: vec![PidFile {
            pid: 41,
            session_id: ID.into(),
            status: Some("idle".into()),
        }],
        ..Snapshot::default()
    };
    let old = now - Duration::from_secs(3600);
    assert_eq!(
        presence(&seen(old, true), &snapshot, now).presence,
        Presence::Stopped
    );
    let fresh = now - Duration::from_secs(30);
    assert_eq!(
        presence(&seen(fresh, true), &snapshot, now).presence,
        Presence::Recent
    );
}

#[test]
fn a_process_in_the_folder_runs_its_newest_session_only() {
    let now = SystemTime::now();
    let old = now - Duration::from_secs(3600);
    let snapshot = Snapshot {
        processes: vec![process(7, "/srv/project")],
        ..Snapshot::default()
    };
    assert_eq!(
        presence(&seen(old, true), &snapshot, now),
        Found {
            presence: Presence::Running,
            pid: Some(7)
        }
    );
    assert_eq!(
        presence(&seen(old, false), &snapshot, now).presence,
        Presence::Stopped
    );
    // A process whose pid file names another session is not this one.
    let claimed = Snapshot {
        pid_files: vec![PidFile {
            pid: 7,
            session_id: OTHER.into(),
            status: None,
        }],
        ..snapshot
    };
    assert_eq!(
        presence(&seen(old, true), &claimed, now).presence,
        Presence::Stopped
    );
}

#[test]
fn a_session_that_moved_into_a_linked_worktree_says_so() {
    use crate::agents::checkout::tests::{has_git, run};

    if !has_git() {
        eprintln!("skipped: no git");
        return;
    }

    let root = tempfile::tempdir().expect("a temp folder");
    let repo = root.path().join("proj");
    fs::create_dir_all(&repo).expect("a folder");
    let repo = paths::canonicalize(repo).expect("a real path");
    run(&repo, &["init", "-q", "-b", "main"]);
    run(&repo, &["commit", "-q", "--allow-empty", "-m", "start"]);
    run(
        &repo,
        &["worktree", "add", "-q", "-b", "fix", ".worktrees/fix"],
    );
    let tree = repo.join(".worktrees/fix");
    let line = |cwd: &Path, branch: &str| {
        format!(
            r#"{{"type":"user","cwd":"{}","gitBranch":"{branch}","timestamp":"2026-11-02T10:00:00.000Z","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"t1","content":"ok"}}]}}}}"#,
            cwd.display()
        )
    };
    let store_root = root.path().join("claude");
    let project = store_root.join("projects/-srv-project");
    fs::create_dir_all(&project).expect("folders");
    let lines = [line(&repo, "main"), line(&tree, "fix")].join("\n");
    fs::write(project.join(format!("{ID}.jsonl")), lines + "\n").expect("a transcript");
    let store = Store::at(&store_root);
    let sessions = list(&store, &Snapshot::default(), 10, None).expect("a list");
    // Its first line said the main checkout; the last says the worktree.
    let place = sessions[0].place.as_ref().expect("a place");
    assert!(place.linked);
    assert_eq!(place.branch.as_deref(), Some("fix"));
    assert_eq!(place.main, repo.to_string_lossy());
    // It resumes where it was started, which outlives the worktree.
    assert_eq!(sessions[0].cwd, repo.to_string_lossy());
}

#[test]
fn a_store_lists_sessions_newest_first_with_what_they_say() {
    let root = std::env::temp_dir().join(format!("beep-claude-store-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let project = root.join("projects").join("-srv-project");
    fs::create_dir_all(&project).unwrap();
    let older = project.join(format!("{OTHER}.jsonl"));
    fs::write(&older, "").unwrap();
    fs::File::options()
        .write(true)
        .open(&older)
        .unwrap()
        .set_modified(SystemTime::now() - Duration::from_secs(600))
        .unwrap();
    fs::write(project.join(format!("{ID}.jsonl")), FIXTURE).unwrap();
    fs::write(project.join("notes.jsonl"), "").unwrap();
    fs::create_dir_all(root.join("sessions")).unwrap();
    fs::write(
        root.join("sessions").join("41.json"),
        // Left over from a longer earlier version of the file.
        format!(r#"{{"pid":41,"sessionId":"{ID}","status":"idle","cwd":"/srv/project"}}t":1}}"#),
    )
    .unwrap();
    let store = Store::at(&root);
    let snapshot = Snapshot {
        processes: vec![process(41, "/srv/project")],
        pid_files: store.pid_files(),
        owned: HashMap::from([(SessionId::from(OTHER), SessionId::from("a3f9c1"))]),
    };
    let sessions = list(&store, &snapshot, 10, None).unwrap();
    assert_eq!(sessions.len(), 2);
    let first = &sessions[0];
    assert_eq!(first.id, ID);
    assert_eq!(first.cwd, "/srv/project");
    assert_eq!(first.title.as_deref(), Some("Fix the flaky test"));
    assert_eq!(first.presence, Presence::Waiting);
    assert_eq!(first.pid, Some(41));
    assert_eq!(first.last_prompt, "now run the whole suite");
    assert_eq!(first.last_message, "All 42 tests pass now.");
    assert_eq!(sessions[1].taken_over, Some(SessionId::from("a3f9c1")));
    assert_eq!(list(&store, &snapshot, 10, Some(&SessionId::from(OTHER))).unwrap().len(), 1);
    assert!(store.find(&SessionId::from(ID)).is_some());
    assert!(store.find(&SessionId::from("../x")).is_none());
    // A transcript filed under another project is found where it went.
    let moved = root.join("projects").join("-srv-other");
    fs::create_dir_all(&moved).unwrap();
    let file = moved.join(format!("{ID}.jsonl"));
    fs::rename(project.join(format!("{ID}.jsonl")), &file).unwrap();
    assert_eq!(store.find(&SessionId::from(ID)), Some(file));
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn a_transcript_is_read_again_only_once_it_changed() {
    let root = tempfile::tempdir().expect("a temp folder");
    let project = root.path().join("projects").join("-srv-project");
    fs::create_dir_all(&project).expect("folders");
    let path = project.join(format!("{ID}.jsonl"));
    fs::write(&path, FIXTURE).expect("a transcript");
    let store = Store::at(root.path());
    let snapshot = Snapshot::default();
    let ids = HashSet::from([SessionId::from(ID)]);
    let prompt = |store: &Store| {
        let listed = list_ids(store, &snapshot, &ids).expect("the listing");
        listed[0].last_prompt.clone()
    };
    assert_eq!(prompt(&store), "now run the whole suite");

    // The same length and modification time: taken as unchanged.
    let modified = fs::metadata(&path)
        .and_then(|meta| meta.modified())
        .expect("a modification time");
    fs::write(&path, FIXTURE.replace("whole suite", "whole thing")).expect("a rewrite");
    fs::File::options()
        .write(true)
        .open(&path)
        .and_then(|file| file.set_modified(modified))
        .expect("the old time back");
    assert_eq!(prompt(&store), "now run the whole suite");
    assert_eq!(
        prompt(&Store::at(root.path())),
        "now run the whole thing",
        "a new store reads the file"
    );

    fs::write(&path, FIXTURE.replace("whole suite", "net tests")).expect("a change");
    assert_eq!(prompt(&store), "now run the net tests");
}

/// Lines Claude Code writes between turns, such as its artifact
/// ledger, touch the file but are not activity: the session's time is
/// its last entry's, and nothing running it shows it stopped.
#[test]
fn lines_between_turns_do_not_make_a_session_recent() {
    let root = tempfile::tempdir().expect("a temp folder");
    let project = root.path().join("projects").join("-srv-project");
    fs::create_dir_all(&project).expect("folders");
    let ledger = r#"{"type":"artifact-autoreact-ledger","v":1,"artifacts":{}}"#;
    let text = format!("{FIXTURE}{ledger}\n{ledger}\n");
    fs::write(project.join(format!("{ID}.jsonl")), text).expect("a transcript");
    let store = Store::at(root.path());
    let listed = list(&store, &Snapshot::default(), 10, None).expect("the listing");
    // 2026-10-29T08:03:45Z, the fixture's last entry.
    assert_eq!(listed[0].last_activity, 1_793_261_025);
    assert_eq!(listed[0].presence, Presence::Stopped);
}

/// An append is read alone, carrying on from the summary before it, and
/// says what reading the file again from scratch says.
#[test]
fn an_appended_transcript_summarizes_as_one_read_whole() {
    let root = tempfile::tempdir().expect("a temp folder");
    let project = root.path().join("projects").join("-srv-project");
    fs::create_dir_all(&project).expect("folders");
    let path = project.join(format!("{ID}.jsonl"));
    fs::write(&path, FIXTURE).expect("a transcript");
    let store = Store::at(root.path());
    let ids = HashSet::from([SessionId::from(ID)]);
    let shown = |store: &Store| {
        let listed = list_ids(store, &Snapshot::default(), &ids).expect("the listing");
        let session = listed.into_iter().next().expect("the session");
        (
            session.title,
            session.last_prompt,
            session.last_message,
            session.cwd,
            session.last_activity,
        )
    };
    let append = |text: &str| {
        let mut file = fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .expect("the transcript");
        std::io::Write::write_all(&mut file, text.as_bytes()).expect("an append");
    };
    let line = |second: u32, kind: &str, rest: &str| {
        format!(
            r#"{{"type":"{kind}","cwd":"/srv/project","timestamp":"2026-10-29T09:00:{second:02}.000Z"{rest}}}"#
        )
    };
    shown(&store);

    let steps = [
        line(1, "user", r#","message":{"role":"user","content":"rename the module"}"#) + "\n",
        line(2, "assistant", r#","message":{"content":[{"type":"text","text":"Renamed it."}]}"#) + "\n",
        r#"{"type":"ai-title","aiTitle":"Rename the module"}"#.to_owned() + "\n",
        r#"{"type":"summary","summary":"Not the title"}"#.to_owned() + "\n",
        line(3, "user", r#","isSidechain":true,"message":{"role":"user","content":"a subagent's prompt"}"#) + "\n",
        // A line not yet ended is read once it is.
        line(4, "user", r#","message":{"role":"user","content":"now the docs"}"#),
        "\n".to_owned(),
        r#"{"type":"custom-title","customTitle":"Module rename"}"#.to_owned() + "\n",
        r#"{"type":"last-prompt","lastPrompt":"recorded"}"#.to_owned() + "\n",
        line(5, "assistant", r#","cwd":"/srv/other","message":{"content":[{"type":"text","text":"Docs done."}]}"#) + "\n",
    ];

    for step in &steps {
        append(step);
        assert_eq!(shown(&store), shown(&Store::at(root.path())), "after {step}");
    }

    assert_eq!(
        shown(&store).0.as_deref(),
        Some("Module rename"),
        "the summary followed the appends"
    );

    // Past a tail read of appends, and after a rewrite, it reads the tail again.
    let filler = line(6, "assistant", r#","message":{"content":[{"type":"text","text":"more"}]}"#);
    append(&format!("{filler}\n").repeat(1024 * 1024 / filler.len() + 1));
    assert_eq!(shown(&store), shown(&Store::at(root.path())));
    fs::write(&path, FIXTURE).expect("a rewrite");
    assert_eq!(shown(&store), shown(&Store::at(root.path())));
}

#[test]
fn a_followed_file_lists_its_folder_again_only_once_a_file_comes_or_goes() {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("projects").join("-srv-project");
    fs::create_dir_all(&project).unwrap();
    let aged = |path: &Path, secs: u64| {
        fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(SystemTime::now() - Duration::from_secs(secs))
            .unwrap();
    };
    let followed = project.join(format!("{ID}.jsonl"));
    let other = project.join(format!("{OTHER}.jsonl"));
    fs::write(&followed, FIXTURE).unwrap();
    aged(&followed, 600);
    fs::write(&other, "").unwrap();
    aged(&other, 60);
    let store = Store::at(root.path());
    let snapshot = Snapshot {
        processes: vec![process(41, "/srv/project")],
        ..Snapshot::default()
    };
    let presence = || {
        describe_file(&store, &followed, &snapshot)
            .unwrap()
            .expect("the session")
            .presence
    };

    // The other session is the folder's newest, so the folder's claude runs it.
    assert_ne!(presence(), Presence::Running);

    // Its time alone changing does not list the folder again.
    aged(&other, 1200);
    assert_ne!(presence(), Presence::Running);

    // A file coming does.
    let third = project.join("3c2b1a0f-2222-4333-8444-955566667777.jsonl");
    fs::write(&third, "").unwrap();
    aged(&third, 1200);
    assert_eq!(presence(), Presence::Running);
}

/// One session asked for by id is found by its file; the store is not
/// listed, so no other transcript is looked at.
#[test]
fn one_session_is_looked_up_by_its_file_without_listing_the_store() {
    let root = tempfile::tempdir().expect("a folder");
    let project = root.path().join("projects").join("-srv-project");
    let elsewhere = root.path().join("projects").join("-srv-other");
    fs::create_dir_all(&project).expect("folders");
    fs::create_dir_all(&elsewhere).expect("folders");
    fs::write(project.join(format!("{ID}.jsonl")), FIXTURE).expect("a transcript");
    fs::write(elsewhere.join(format!("{OTHER}.jsonl")), FIXTURE).expect("a transcript");
    let store = Store::at(root.path());

    let found = list(&store, &Snapshot::default(), 1, Some(&SessionId::from(ID))).expect("a lookup");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].id, ID);
    assert_eq!(found[0].title.as_deref(), Some("Fix the flaky test"));
    // Only its own folder was read for its newest transcript, and only it was summarized.
    assert_eq!(store.memos.newest.len(), 1);
    assert_eq!(store.memos.summaries.len(), 1);
    assert!(
        list(&store, &Snapshot::default(), 1, Some(&SessionId::from("0f3c2a9e-0000-4222-8333-944455556666")))
            .expect("a lookup")
            .is_empty()
    );
}
