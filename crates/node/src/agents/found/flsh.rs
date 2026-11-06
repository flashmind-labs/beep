//! flsh's sessions (`locate`, `sessions`): a folder per folder it ran in,
//! `<data dir>/sessions/<hash>/`. flsh 0.2.9 on keeps each session there as
//! a JSONL file: a `session_metadata` header, then one line per message
//! (`type`, `timestamp`, `message.content`). An older flsh kept a SQLite
//! `sessions.db`, read here (read-only) until flsh has imported it, which
//! it marks with `.sqlite-imported`.

use super::{Agent, MAX_FILES, Memos, Seen, head_lines, newest_files, prompt_title};
use crate::agents::claude::unix_seconds;
use serde_json::Value;
use std::{
    collections::HashSet,
    env, fs,
    path::{Path, PathBuf},
    sync::Arc,
};

/// The most databases read in one listing.
const MAX_DATABASES: usize = 300;

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
    sessions_dir().filter(|dir| dir.is_dir())
}

/// Where flsh keeps its sessions, as flsh finds it: `$FLASHMIND_HOME`, else
/// `~/.flashmind` when it exists, else `flashmind` in the platform's data
/// folder (`%LOCALAPPDATA%` on Windows, else `$XDG_DATA_HOME` or
/// `~/.local/share`, macOS too). It may not exist yet.
pub(crate) fn sessions_dir() -> Option<PathBuf> {
    let var = |name: &str| env::var_os(name).filter(|value| !value.is_empty());
    let home = dirs::home_dir();
    let data = match var("FLASHMIND_HOME") {
        Some(dir) => Some(PathBuf::from(dir)).filter(|dir| dir.is_absolute())?,
        None => match home.as_ref().map(|home| home.join(".flashmind")) {
            Some(legacy) if legacy.is_dir() => legacy,
            _ if cfg!(windows) => dirs::data_local_dir()?.join("flashmind"),
            _ => var("XDG_DATA_HOME")
                .map(PathBuf::from)
                .filter(|dir| dir.is_absolute())
                .or_else(|| home.map(|home| home.join(".local/share")))?
                .join("flashmind"),
        },
    };
    Some(data.join("sessions"))
}

/// The sessions under the sessions folder `root`: its JSONL files, then
/// those of the databases flsh has not imported.
pub(super) fn sessions(root: &Path, memos: &Memos) -> Vec<Seen> {
    let jsonl = |path: &Path| path.extension().is_some_and(|ext| ext == "jsonl");
    let mut seen: Vec<Seen> = newest_files(root, 1, &jsonl)
        .into_iter()
        .filter_map(|(path, at)| {
            let read = || Ok::<_, std::convert::Infallible>(head(&path));
            let Ok(found) = memos.heads.read(&path, read);
            let mut seen = found?;
            seen.last_activity = seen.last_activity.max(at);
            seen.file = Some(path);
            Some(seen)
        })
        .collect();
    let Ok(entries) = fs::read_dir(root) else {
        return seen;
    };
    let mut ids: HashSet<String> = seen.iter().map(|one| one.id.clone()).collect();
    for entry in entries.flatten().take(MAX_DATABASES) {
        let database = entry.path().join("sessions.db");
        if !database.is_file() || entry.path().join(".sqlite-imported").exists() {
            continue;
        }
        match memos
            .databases
            .read_database(&database, || read(&database).map(Arc::new))
        {
            Ok(found) => seen.extend(
                found
                    .iter()
                    .filter(|one| ids.insert(one.id.clone()))
                    .cloned(),
            ),
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

/// A session file's header and first prompt; `None` for one with no
/// folder or no message (a session flsh deleted keeps its header).
fn head(path: &Path) -> Option<Seen> {
    let mut lines = head_lines(path).ok()?;
    let header: Value = serde_json::from_str(&lines.next()?).ok()?;
    if header["subtype"] != "session_metadata" {
        return None;
    }
    let id = header["sessionId"].as_str().filter(|id| !id.is_empty())?;
    let metadata = &header["metadata"];
    let cwd = metadata["working_dir"]
        .as_str()
        .filter(|cwd| !cwd.is_empty())?;
    let mut started = None;
    let mut first = None;
    for line in lines {
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        started = started.or_else(|| value["timestamp"].as_str().and_then(unix_seconds));
        if value["type"] == "user" {
            first = value["message"]["content"].as_str().and_then(prompt_title);
            break;
        }
    }
    let started = started?;
    Some(Seen {
        agent: Agent::Flsh,
        id: id.to_owned(),
        cwd: PathBuf::from(cwd),
        started_at: started,
        last_activity: started,
        title: metadata["title"].as_str().and_then(prompt_title).or(first),
        file: None,
    })
}

/// A locked database, or one older than the `working_dir` column, fails
/// here and is skipped.
fn read(database: &Path) -> rusqlite::Result<Vec<Seen>> {
    let conn = super::open_database(database)?;
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

    const FIXTURE: &str = include_str!("fixtures/flsh.jsonl");
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
        let ids: Vec<&str> = found.iter().map(|one| one.id.as_str()).collect();
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
}
