//! OpenCode's sessions (`locate`, `sessions`): SQLite databases in its data
//! folder, opened read-only; one that is locked or of another shape is
//! skipped.

use super::{Agent, MAX_FILES, Memos, Seen, prompt_title};
use std::{
    env,
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

/// The most databases read in one listing.
const MAX_DATABASES: usize = 8;
/// The title OpenCode gives a session before it names it; the first prompt
/// is shown instead.
const UNNAMED: &str = "New session - ";

/// Top-level, unarchived sessions with the text of their first user
/// message; times are Unix milliseconds.
const QUERY: &str = "SELECT s.id, s.title, s.directory, s.time_created, s.time_updated,
        (SELECT substr(group_concat(json_extract(p.data, '$.text'), char(10)), 1, 1000)
           FROM part p
          WHERE p.message_id = (SELECT m.id FROM message m
                                 WHERE m.session_id = s.id
                                   AND json_extract(m.data, '$.role') = 'user'
                                 ORDER BY m.time_created ASC, m.id ASC LIMIT 1)
            AND json_extract(p.data, '$.type') = 'text'
            AND coalesce(json_extract(p.data, '$.synthetic'), 0) = 0)
     FROM session s
     WHERE s.parent_id IS NULL AND s.time_archived IS NULL AND s.directory != ''
     ORDER BY s.time_updated DESC
     LIMIT ?1";

/// Where OpenCode keeps its sessions here: its databases, the one
/// `OPENCODE_DB` names or every `opencode*.db` in its data folder.
pub(super) fn locate() -> Option<PathBuf> {
    let data = data_dir()?;
    match env::var_os("OPENCODE_DB") {
        Some(named) if !named.is_empty() => {
            let database = data.join(named);
            database.is_file().then_some(database)
        }
        _ => data.is_dir().then_some(data),
    }
}

/// OpenCode's data folder, which follows XDG on every platform.
fn data_dir() -> Option<PathBuf> {
    let base = match env::var_os("XDG_DATA_HOME") {
        Some(dir) if Path::new(&dir).is_absolute() => PathBuf::from(dir),
        _ => dirs::home_dir()?.join(".local/share"),
    };
    Some(base.join("opencode"))
}

/// The databases at `root`: the database itself, or those in the folder.
pub(crate) fn databases_at(root: &Path) -> Vec<PathBuf> {
    if root.is_file() {
        vec![root.to_owned()]
    } else {
        databases(root)
    }
}

/// The sessions in `root`: a database, or a data folder of them.
pub(super) fn sessions(root: &Path, memos: &Memos) -> Vec<Seen> {
    let databases = databases_at(root);
    let mut seen = Vec::new();
    for database in databases {
        match memos
            .databases
            .read_database(&database, || read(&database).map(Arc::new))
        {
            Ok(found) => seen.extend(found.iter().cloned()),
            Err(e) => log::debug!(
                "OpenCode sessions not read; database={} error={e}",
                database.display()
            ),
        }
        if seen.len() >= MAX_FILES {
            break;
        }
    }
    seen
}

/// The `opencode*.db` files in the data folder `dir`.
fn databases(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let wanted = |name: &OsStr| {
        let name = name.to_string_lossy();
        name.starts_with("opencode") && name.ends_with(".db")
    };
    let mut found: Vec<PathBuf> = entries
        .flatten()
        .filter(|entry| wanted(&entry.file_name()))
        .map(|entry| entry.path())
        .filter(|path| path.is_file())
        .collect();
    found.sort();
    found.truncate(MAX_DATABASES);
    found
}

fn read(database: &Path) -> rusqlite::Result<Vec<Seen>> {
    let conn = super::open_database(database)?;
    let mut statement = conn.prepare(QUERY)?;
    let limit = i64::try_from(MAX_FILES).unwrap_or(i64::MAX);
    let rows = statement.query_map([limit], |row| {
        let title: Option<String> = row.get(1)?;
        let created: Option<i64> = row.get(3)?;
        let updated: Option<i64> = row.get(4)?;
        let first: Option<String> = row.get(5)?;
        Ok(Seen {
            agent: Agent::OpenCode,
            id: row.get(0)?,
            cwd: PathBuf::from(row.get::<_, String>(2)?),
            started_at: seconds(created),
            last_activity: seconds(updated.or(created)),
            title: title
                .as_deref()
                .filter(|title| !title.starts_with(UNNAMED))
                .and_then(prompt_title)
                .or_else(|| first.as_deref().and_then(prompt_title)),
            file: None,
        })
    })?;
    rows.collect()
}

/// Unix milliseconds as seconds.
fn seconds(at: Option<i64>) -> u64 {
    at.and_then(|at| u64::try_from(at / 1000).ok()).unwrap_or(0)
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use rusqlite::Connection;

    const SCHEMA: &str = "CREATE TABLE project (
            id TEXT PRIMARY KEY, worktree TEXT NOT NULL, time_created INTEGER NOT NULL,
            time_updated INTEGER NOT NULL, sandboxes TEXT NOT NULL);
        CREATE TABLE session (
            id TEXT PRIMARY KEY, project_id TEXT NOT NULL, workspace_id TEXT,
            parent_id TEXT, slug TEXT NOT NULL, directory TEXT NOT NULL, path TEXT,
            title TEXT NOT NULL, version TEXT NOT NULL, time_created INTEGER NOT NULL,
            time_updated INTEGER NOT NULL, time_compacting INTEGER, time_archived INTEGER);
        CREATE TABLE message (
            id TEXT PRIMARY KEY, session_id TEXT NOT NULL, time_created INTEGER NOT NULL,
            time_updated INTEGER NOT NULL, data TEXT NOT NULL);
        CREATE TABLE part (
            id TEXT PRIMARY KEY, message_id TEXT NOT NULL, session_id TEXT NOT NULL,
            time_created INTEGER NOT NULL, time_updated INTEGER NOT NULL, data TEXT NOT NULL);";

    const ROWS: &str = "INSERT INTO project VALUES ('prj_1', '/home/user/proj', 1, 1, '[]');
        INSERT INTO session VALUES
          ('ses_a', 'prj_1', NULL, NULL, 'calm-fox', '/home/user/proj', NULL,
           'New session - 2026-08-27T23:01:39.077Z', '1.0.0', 1788000000000, 1788000090500,
           NULL, NULL),
          ('ses_b', 'prj_1', NULL, NULL, 'bold-owl', '/home/user/proj/sub', NULL,
           'Fix the flaky test', '1.0.0', 1788000100000, 1788000200000, NULL, NULL),
          ('ses_c', 'prj_1', NULL, 'ses_b', 'sub-task', '/home/user/proj', NULL,
           'Explore the tests', '1.0.0', 1788000110000, 1788000120000, NULL, NULL),
          ('ses_d', 'prj_1', NULL, NULL, 'old-elk', '/home/user/proj', NULL,
           'Archived', '1.0.0', 1788000000000, 1788000000000, NULL, 1788000300000);
        INSERT INTO message VALUES
          ('msg_1', 'ses_a', 1788000001000, 1788000001000, '{\"role\":\"user\",\"agent\":\"build\"}'),
          ('msg_2', 'ses_a', 1788000002000, 1788000002000, '{\"role\":\"assistant\"}'),
          ('msg_3', 'ses_a', 1788000003000, 1788000003000, '{\"role\":\"user\"}');
        INSERT INTO part VALUES
          ('prt_1', 'msg_1', 'ses_a', 1, 1, '{\"type\":\"text\",\"text\":\"Port the tests\"}'),
          ('prt_2', 'msg_1', 'ses_a', 2, 2,
           '{\"type\":\"text\",\"text\":\"<context>\",\"synthetic\":true}'),
          ('prt_3', 'msg_2', 'ses_a', 3, 3, '{\"type\":\"text\",\"text\":\"Done\"}'),
          ('prt_4', 'msg_3', 'ses_a', 4, 4, '{\"type\":\"text\",\"text\":\"Thanks\"}');";

    pub(in crate::agents::found) fn database(path: &Path) {
        let conn = Connection::open(path).expect("a database");
        conn.execute_batch(SCHEMA).expect("the schema");
        conn.execute_batch(ROWS).expect("rows");
    }

    #[test]
    fn reads_the_top_sessions_and_their_first_prompt() {
        let root = tempfile::tempdir().expect("a temp folder");
        database(&root.path().join("opencode.db"));
        fs::write(root.path().join("auth.json"), "{}").expect("a file");
        let found = sessions(root.path(), &Memos::default());
        let ids: Vec<&str> = found.iter().map(|seen| seen.id.as_str()).collect();
        assert_eq!(ids, ["ses_b", "ses_a"], "{found:?}");
        let (named, unnamed) = (&found[0], &found[1]);
        assert_eq!(named.agent, Agent::OpenCode);
        assert_eq!(named.cwd, Path::new("/home/user/proj/sub"));
        assert_eq!(named.title.as_deref(), Some("Fix the flaky test"));
        assert_eq!(unnamed.title.as_deref(), Some("Port the tests"));
        assert_eq!(
            (unnamed.started_at, unnamed.last_activity),
            (1_788_000_000, 1_788_000_090)
        );
    }
}
