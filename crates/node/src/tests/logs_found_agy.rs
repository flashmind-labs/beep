use super::*;
use rusqlite::Connection;

/// A conversation `id` in `root` that ran in `folder` from `created`, with
/// `steps` steps.
fn conversation(root: &Path, id: &str, folder: &str, created: i64, steps: i64) {
    let conn = Connection::open(root.join(format!("{id}.db"))).expect("a database");
    conn.execute_batch(
        "CREATE TABLE steps (idx INTEGER PRIMARY KEY, step_type INTEGER, status INTEGER, step_payload BLOB);
         CREATE TABLE trajectory_metadata_blob (id TEXT DEFAULT 'main' PRIMARY KEY, data BLOB);",
    )
    .expect("the schema");
    let trajectory = Trajectory {
        workspaces: vec![Workspace {
            uri: format!("file://{folder}"),
        }],
        created: Some(Time { seconds: created }),
    };
    conn.execute(
        "INSERT INTO trajectory_metadata_blob (data) VALUES (?1)",
        [trajectory.encode_to_vec()],
    )
    .expect("the trajectory");

    for idx in 0..steps {
        conn.execute("INSERT INTO steps (idx) VALUES (?1)", [idx])
            .expect("a step");
    }
}

#[test]
fn lists_conversations_with_titles_and_skips_subagents() {
    let home = tempfile::tempdir().expect("a temp folder");
    let root = home.path().join("conversations");
    std::fs::create_dir_all(&root).expect("a folder");
    conversation(&root, "titled", "/srv/project", 100, 2);
    conversation(&root, "prompted", "/srv/project/sub", 200, 1);
    conversation(&root, "child", "/srv/project", 300, 1);
    conversation(&root, "empty", "/srv/project", 400, 0);

    let summaries =
        Connection::open(home.path().join("conversation_summaries.db")).expect("a database");
    summaries
        .execute_batch(
            "CREATE TABLE conversation_summaries (conversation_id TEXT PRIMARY KEY,
               title TEXT NOT NULL DEFAULT '', parent_conversation_id TEXT NOT NULL DEFAULT '');
             CREATE TABLE conversation_parents (conversation_id TEXT PRIMARY KEY, parent_id TEXT NOT NULL);
             CREATE TABLE conversations_fts (conversation_id TEXT, chunk_index INTEGER, user_body TEXT);
             INSERT INTO conversation_summaries VALUES ('titled', 'Fix the build', ''),
               ('prompted', '', ''), ('child', '', 'titled');
             INSERT INTO conversations_fts VALUES ('titled', 0, 'fix it'),
               ('prompted', 0, 'Port the tests'), ('prompted', 1, 'later');",
        )
        .expect("summaries");
    drop(summaries);

    let mut found = sessions(&root, &Memos::default());
    found.sort_by(|a, b| a.id.cmp(&b.id));

    let ids: Vec<&str> = found.iter().map(|seen| seen.id.as_ref()).collect();
    assert_eq!(ids, ["prompted", "titled"]);
    assert_eq!(found[0].title.as_deref(), Some("Port the tests"));
    assert_eq!(found[0].cwd, Path::new("/srv/project/sub"));
    assert_eq!(found[0].started_at, 200);
    assert_eq!(found[1].title.as_deref(), Some("Fix the build"));
    assert_eq!(found[1].agent, Agent::Agy);
}

#[test]
fn reads_a_conversations_steps_as_its_transcript() {
    let dir = tempfile::tempdir().expect("a temp folder");
    let path = dir.path().join("talk.db");
    let conn = Connection::open(&path).expect("a database");
    conn.execute_batch(
        "CREATE TABLE steps (idx INTEGER PRIMARY KEY, step_type INTEGER, status INTEGER, step_payload BLOB);",
    )
    .expect("the schema");
    let call = Call {
        id: "c1".into(),
        name: "run_command".into(),
        args: r#"{"CommandLine":"cargo test\n--all","toolSummary":"Run tests"}"#.into(),
    };
    let steps: [(i64, i64, Vec<u8>); 6] = [
        (
            14,
            3,
            UserStep {
                user: Some(UserInput {
                    text: "Fix the build".into(),
                }),
            }
            .encode_to_vec(),
        ),
        (101, 3, Vec::new()),
        (
            15,
            3,
            ReplyStep {
                reply: Some(Reply {
                    text: "Running the tests.".into(),
                    thinking: "Tests first.".into(),
                    calls: vec![call.clone()],
                }),
            }
            .encode_to_vec(),
        ),
        (
            132,
            7,
            CallStep {
                meta: Some(StepMeta { call: Some(call) }),
            }
            .encode_to_vec(),
        ),
        (
            17,
            3,
            ErrorStep {
                error: Some(StepError {
                    detail: Some(ErrorDetail {
                        text: "API error".into(),
                    }),
                }),
            }
            .encode_to_vec(),
        ),
        (23, 3, Vec::new()),
    ];
    for (idx, (kind, status, payload)) in (0_i64..).zip(steps) {
        conn.execute(
            "INSERT INTO steps VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![idx, kind, status, payload],
        )
        .expect("a step");
    }
    drop(conn);

    let read: Vec<(EntryKind, String)> = read_transcript(&path)
        .expect("the conversation reads")
        .entries
        .into_iter()
        .map(|entry| (entry.kind, entry.text))
        .collect();
    assert_eq!(
        read,
        [
            (EntryKind::User, "Fix the build".into()),
            (EntryKind::Thought, "Tests first.".into()),
            (EntryKind::Agent, "Running the tests.".into()),
            (EntryKind::Tool, "run_command: cargo test (failed)".into()),
            (EntryKind::Other, "API error".into()),
            (EntryKind::Compacted, COMPACTED_ENTRY.into()),
        ]
    );
}

#[test]
fn native_invocations_keep_their_card_state_and_open_the_child_transcript() {
    let home = tempfile::tempdir().expect("a temp folder");
    let root = home.path().join("conversations");
    std::fs::create_dir_all(&root).expect("a folder");
    conversation(&root, "parent", "/srv/project", 100, 0);
    conversation(&root, "child", "/srv/project", 101, 0);
    let parent = Connection::open(root.join("parent.db")).expect("a parent database");
    let child = Connection::open(root.join("child.db")).expect("a child database");
    let summary = Connection::open(home.path().join("conversation_summaries.db"))
        .expect("a summary database");
    summary
        .execute_batch(
            "PRAGMA journal_mode=WAL;
             CREATE TABLE conversation_summaries (conversation_id TEXT PRIMARY KEY,
               title TEXT, parent_conversation_id TEXT, status TEXT, killed INTEGER);
             CREATE TABLE conversation_parents (conversation_id TEXT, parent_id TEXT);
             CREATE TABLE conversations_fts (conversation_id TEXT, chunk_index INTEGER, user_body TEXT);
             INSERT INTO conversation_summaries
             VALUES ('child', 'Review fixtures', 'parent', 'CASCADE_RUN_STATUS_RUNNING', 0);",
        )
        .expect("a running child summary");

    let spawn = Call {
        id: "call-native".into(),
        name: SPAWN_TOOL.into(),
        args: r#"{"Prompt":"Review the test fixtures","Role":"Review fixtures","TypeName":"self"}"#
            .into(),
    };
    let cancelled = Call {
        id: "call-cancelled".into(),
        args: r#"{"Role":"Cancelled task"}"#.into(),
        ..spawn.clone()
    };
    let ordinary = Call {
        id: "call-file".into(),
        name: "read_file".into(),
        args: r#"{"Path":"tests/fixtures.rs"}"#.into(),
    };
    let record = |conn: &Connection, index, kind, status, bytes: Vec<u8>| {
        conn.execute(
            "INSERT INTO steps VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![index, kind, status, bytes],
        )
        .expect("a saved step");
    };
    record(
        &parent,
        0,
        REPLY,
        SUCCEEDED,
        ReplyStep {
            reply: Some(Reply {
                calls: vec![spawn.clone(), cancelled.clone(), ordinary.clone()],
                ..Reply::default()
            }),
        }
        .encode_to_vec(),
    );
    let result = |call| {
        CallStep {
            meta: Some(StepMeta { call: Some(call) }),
        }
        .encode_to_vec()
    };
    let mut launched = result(spawn);
    launched.extend(
        InvocationStep {
            output: Some(InvocationResult {
                response: Some(InvocationResponse {
                    result: Some(InvocationPayload {
                        invocation: Some(Invocation {
                            reference: Some(ConversationReference { id: "child".into() }),
                        }),
                    }),
                }),
            }),
        }
        .encode_to_vec(),
    );
    record(&parent, 1, CALL, SUCCEEDED, launched);
    record(&parent, 2, CALL, CANCELLED, result(cancelled));
    record(
        &child,
        0,
        REPLY,
        SUCCEEDED,
        ReplyStep {
            reply: Some(Reply {
                text: "Fixtures checked.".into(),
                calls: vec![ordinary],
                ..Reply::default()
            }),
        }
        .encode_to_vec(),
    );

    let stores = super::super::Stores {
        agy: Some(root),
        ..Default::default()
    };
    let parent_id = SessionId::from("parent");
    let native_id = SessionId::from("subagent:agy:child");
    let parent_page = |last| {
        stores
            .transcript(&parent_id, last, None)
            .expect("history can be read")
            .expect("the parent is found")
    };
    let page = parent_page(20);
    assert!(
        page.entries[0]
            .text
            .starts_with("Review fixtures [in_progress]")
    );
    assert!(
        page.entries[0]
            .text
            .contains("[session] subagent:agy:child")
    );
    assert!(page.entries[0].text.contains("[steps] 1"));
    assert!(page.entries[1].text.starts_with("Cancelled task [failed]"));
    assert!(!page.entries[1].text.contains("[session]"));
    assert_eq!(page.entries[2].text, "read_file: tests/fixtures.rs");

    let live = stores
        .agy_child_cards(
            &parent_id,
            &[(7, "call-native".into()), (4, "unrelated".into())],
            9,
        )
        .expect("live call IDs resolve");
    assert_eq!(live, [(7, page.entries[0].text.clone())]);
    assert_eq!(parent_page(2).earlier, 1);
    assert!(parent_page(2).entries[0].text.starts_with("Cancelled task"));

    let native = stores
        .find(Agent::Agy, &native_id)
        .expect("the linked child opens");
    assert_eq!(native.presence, beep_proto::node::agent::Presence::Working);
    assert_eq!(native.cwd, "/srv/project");
    assert_eq!(
        stores
            .transcript(&native_id, 20, None)
            .expect("native transcript")
            .expect("child page")
            .entries[0]
            .text,
        "Fixtures checked."
    );
    assert_eq!(stores.sessions(Some(Agent::Agy)).len(), 1);

    summary
        .execute(
            "UPDATE conversation_summaries SET status = 'CASCADE_RUN_STATUS_IDLE'",
            [],
        )
        .expect("the child finishes");
    assert!(
        parent_page(20).entries[0]
            .text
            .starts_with("Review fixtures [completed]")
    );
    summary
        .execute("UPDATE conversation_summaries SET killed = 1", [])
        .expect("the child stops");
    assert!(
        parent_page(20).entries[0]
            .text
            .starts_with("Review fixtures [failed]")
    );
}
