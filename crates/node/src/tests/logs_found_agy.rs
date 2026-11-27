use super::*;
use rusqlite::Connection;

/// A conversation `id` in `root` that ran in `folder` from `created`, with
/// `steps` steps.
fn conversation(root: &Path, id: &str, folder: &str, created: i64, steps: i64) {
    let conn = Connection::open(root.join(format!("{id}.db"))).expect("a database");
    conn.execute_batch(
        "CREATE TABLE steps (idx INTEGER PRIMARY KEY, metadata BLOB);
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
    let steps = [
        (
            14,
            3,
            Step {
                user: Some(UserInput {
                    text: "Fix the build".into(),
                }),
                ..Default::default()
            },
        ),
        (101, 3, Step::default()),
        (
            15,
            3,
            Step {
                reply: Some(Reply {
                    text: "Running the tests.".into(),
                    thinking: "Tests first.".into(),
                    calls: vec![call.clone()],
                }),
                ..Default::default()
            },
        ),
        (
            132,
            7,
            Step {
                meta: Some(StepMeta { call: Some(call) }),
                ..Default::default()
            },
        ),
        (
            17,
            3,
            Step {
                error: Some(StepError {
                    detail: Some(ErrorDetail {
                        text: "API error".into(),
                    }),
                }),
                ..Default::default()
            },
        ),
        (23, 3, Step::default()),
    ];
    for (idx, (kind, status, step)) in (0_i64..).zip(steps) {
        conn.execute(
            "INSERT INTO steps VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![idx, kind, status, step.encode_to_vec()],
        )
        .expect("a step");
    }
    drop(conn);

    let read: Vec<(EntryKind, String)> = transcript(&path)
        .expect("the conversation reads")
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
