use super::*;
use rusqlite::Connection;

const SCHEMA: &str = "CREATE TABLE sessions (
        id INTEGER PRIMARY KEY AUTOINCREMENT, chat_key TEXT NOT NULL,
        entry_kind TEXT NOT NULL, content TEXT NOT NULL DEFAULT '',
        tool_calls TEXT, tool_call_id TEXT, tool_name TEXT, metadata TEXT,
        turn_index INTEGER NOT NULL, created_at INTEGER NOT NULL);
    CREATE TABLE session_meta (
        chat_key TEXT PRIMARY KEY, title TEXT, model TEXT,
        working_dir TEXT, created_at INTEGER NOT NULL);";

#[test]
fn reads_each_database() {
    let root = tempfile::tempdir().expect("a temp folder");
    let dir = root.path().join("0123456789abcdef");
    fs::create_dir_all(&dir).expect("folders");
    let conn = Connection::open(dir.join("sessions.db")).expect("a database");
    conn.execute_batch(SCHEMA).expect("the schema");
    conn.execute_batch(
        "INSERT INTO session_meta VALUES ('session_a', NULL, 'm', '/home/user/proj', 100);
         INSERT INTO session_meta VALUES ('session_b', 'Named', 'm', '/home/user/proj', 200);
         INSERT INTO session_meta VALUES ('session_c', 'No folder', 'm', NULL, 300);
         INSERT INTO sessions (chat_key, entry_kind, content, turn_index, created_at)
           VALUES ('session_a', '{\"type\":\"system_prompt\"}', 'You are flsh', 0, 100),
                  ('session_a', '{\"type\":\"user\"}', 'Port the tests', 1, 110),
                  ('session_a', '{\"type\":\"assistant\"}', 'Done', 2, 150);",
    )
    .expect("rows");
    drop(conn);
    fs::create_dir_all(root.path().join("display")).expect("a folder");
    let mut found = sessions(root.path(), &Memos::default());
    found.sort_by(|a, b| a.id.cmp(&b.id));
    assert_eq!(found.len(), 2, "{found:?}");
    assert_eq!(found[0].id, "session_a");
    assert_eq!(found[0].cwd, Path::new("/home/user/proj"));
    assert_eq!(found[0].title.as_deref(), Some("Port the tests"));
    assert_eq!((found[0].started_at, found[0].last_activity), (100, 150));
    assert_eq!(found[1].title.as_deref(), Some("Named"));
    assert_eq!(found[1].last_activity, 200);
}

const FIXTURE: &str = include_str!("../logs/found/fixtures/flsh.jsonl");
const JSONL_ID: &str = "session_0b1c2d3e00004000800001234567890a";

/// A database in `dir` holding `id`.
fn database(dir: &Path, id: &str) {
    fs::create_dir_all(dir).expect("folders");
    let conn = Connection::open(dir.join("sessions.db")).expect("a database");
    conn.execute_batch(SCHEMA).expect("the schema");
    conn.execute(
        "INSERT INTO session_meta VALUES (?1, 'Old', 'm', '/srv/laptop/old', 100)",
        [id],
    )
    .expect("a row");
}

#[test]
fn reads_session_files_and_databases_not_imported() {
    let root = tempfile::tempdir().expect("a temp folder");
    let dir = root.path().join("0123456789abcdef");
    fs::create_dir_all(&dir).expect("folders");
    fs::write(dir.join("a.jsonl"), FIXTURE).expect("a session file");
    let deleted = FIXTURE
        .lines()
        .next()
        .unwrap()
        .replace(JSONL_ID, "session_gone");
    fs::write(dir.join("b.jsonl"), deleted + "\n").expect("a deleted session");
    // The same session, from before flsh imported it.
    database(&dir, JSONL_ID);
    database(&root.path().join("1111111111111111"), "session_old");
    let imported = root.path().join("2222222222222222");
    database(&imported, "session_imported");
    fs::write(imported.join(".sqlite-imported"), "").expect("the marker");

    let mut found = sessions(root.path(), &Memos::default());
    found.sort_by(|a, b| a.id.cmp(&b.id));
    let ids: Vec<&str> = found.iter().map(|one| one.id.as_ref()).collect();
    assert_eq!(ids, [JSONL_ID, "session_old"]);
    let file = &found[0];
    assert_eq!(file.cwd, Path::new("/srv/laptop/app"));
    assert_eq!(
        file.title.as_deref(),
        Some("Run the tests in /srv/laptop/app")
    );
    assert_eq!(file.started_at, 1_793_784_600);
    assert!(file.last_activity >= file.started_at);
    assert_eq!(file.file.as_deref(), Some(dir.join("a.jsonl").as_path()));
}

#[test]
fn a_database_without_the_folder_column_is_skipped() {
    let root = tempfile::tempdir().expect("a temp folder");
    let dir = root.path().join("old");
    fs::create_dir_all(&dir).expect("folders");
    let conn = Connection::open(dir.join("sessions.db")).expect("a database");
    conn.execute_batch(
        "CREATE TABLE session_meta (chat_key TEXT, title TEXT, created_at INTEGER);",
    )
    .expect("the schema");
    drop(conn);
    assert!(sessions(root.path(), &Memos::default()).is_empty());
}

/// flsh saves the title it generates in the header, replacing the first
/// prompt as the session's name.
#[test]
fn a_generated_title_in_the_header_names_the_session() {
    let root = tempfile::tempdir().expect("a temp folder");
    let dir = root.path().join("0123456789abcdef");
    fs::create_dir_all(&dir).expect("folders");
    let titled = FIXTURE.replacen(
        r#""title":null"#,
        r#""title":"Run the app's tests""#,
        1,
    );
    fs::write(dir.join("a.jsonl"), titled).expect("a session file");

    let found = sessions(root.path(), &Memos::default());
    assert_eq!(found[0].title.as_deref(), Some("Run the app's tests"));
}
