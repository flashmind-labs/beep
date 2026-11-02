//! flsh's sessions: one SQLite database per folder it started in,
//! `<data dir>/sessions/<hash>/sessions.db`. Its `session_meta` table has a
//! row per session (`chat_key`, `title`, `working_dir`, `created_at` in Unix
//! seconds) and `sessions` its entries, the user's with `entry_kind`
//! `{"type":"user"}`. The data dir is `$FLASHMIND_HOME`, else `~/.flashmind`
//! when it exists, else the platform's data folder plus `flashmind`.
//! Databases are opened read-only; one that is locked or older than the
//! `working_dir` column is skipped.

use super::{Agent, MAX_FILES, Memos, Seen, prompt_title};
use rusqlite::{Connection, OpenFlags};
use std::{
    env, fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

/// The most databases read in one listing.
const MAX_DATABASES: usize = 300;
/// How long a locked database is waited on.
const BUSY: Duration = Duration::from_millis(200);

const QUERY: &str = "SELECT m.chat_key, m.title, m.working_dir, m.created_at,
        (SELECT MAX(s.created_at) FROM sessions s WHERE s.chat_key = m.chat_key),
        (SELECT substr(s.content, 1, 1000) FROM sessions s
          WHERE s.chat_key = m.chat_key AND s.entry_kind = '{\"type\":\"user\"}'
          ORDER BY s.turn_index ASC LIMIT 1)
     FROM session_meta m
     WHERE m.working_dir IS NOT NULL AND m.working_dir != ''
     ORDER BY m.created_at DESC
     LIMIT ?1";

/// flsh's sessions folder, when it has one here.
pub(super) fn locate() -> Option<PathBuf> {
    let data = match env::var_os("FLASHMIND_HOME") {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => {
            let legacy = dirs::home_dir()?.join(".flashmind");
            if legacy.is_dir() {
                legacy
            } else {
                dirs::data_local_dir()?.join("flashmind")
            }
        }
    };
    let sessions = data.join("sessions");
    sessions.is_dir().then_some(sessions)
}

/// The sessions in every database under the sessions folder `root`.
pub(super) fn sessions(root: &Path, memos: &Memos) -> Vec<Seen> {
    let Ok(entries) = fs::read_dir(root) else {
        return Vec::new();
    };
    let mut seen = Vec::new();
    for entry in entries.flatten().take(MAX_DATABASES) {
        let database = entry.path().join("sessions.db");
        if !database.is_file() {
            continue;
        }
        match memos
            .databases
            .read_database(&database, || read(&database).map(Arc::new))
        {
            Ok(found) => seen.extend(found.iter().cloned()),
            Err(e) => log::debug!(
                "flsh sessions not read; database={} error={e}",
                database.display()
            ),
        }
        if seen.len() >= MAX_FILES {
            break;
        }
    }
    seen
}

fn read(database: &Path) -> rusqlite::Result<Vec<Seen>> {
    let conn = Connection::open_with_flags(
        database,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    conn.busy_timeout(BUSY)?;
    let mut statement = conn.prepare(QUERY)?;
    let limit = i64::try_from(MAX_FILES).unwrap_or(i64::MAX);
    let rows = statement.query_map([limit], |row| {
        let created: Option<i64> = row.get(3)?;
        let last: Option<i64> = row.get(4)?;
        let title: Option<String> = row.get(1)?;
        let first: Option<String> = row.get(5)?;
        Ok(Seen {
            agent: Agent::Flsh,
            id: row.get(0)?,
            cwd: PathBuf::from(row.get::<_, String>(2)?),
            started_at: seconds(created),
            last_activity: seconds(last.or(created)),
            title: title
                .as_deref()
                .and_then(prompt_title)
                .or_else(|| first.as_deref().and_then(prompt_title)),
            file: None,
        })
    })?;
    rows.collect()
}

fn seconds(at: Option<i64>) -> u64 {
    at.and_then(|at| u64::try_from(at).ok()).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
