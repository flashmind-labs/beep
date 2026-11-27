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
