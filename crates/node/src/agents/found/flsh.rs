//! flsh's sessions (`locate`, `sessions`): a folder per folder it ran in,
//! `<data dir>/sessions/<hash>/`. flsh 0.2.9 on keeps each session there as
//! a JSONL file: a `session_metadata` header, then one line per message
//! (`type`, `timestamp`, `message.content`). An older flsh kept a SQLite
//! `sessions.db`, read here (read-only) until flsh has imported it, which
//! it marks with `.sqlite-imported`.

use super::{Agent, Memos, Seen, is_jsonl, newest_files, prompt_title, read_databases, xdg_data_dir};
use crate::agents::{
    found::unix_seconds,
    window::{MAX_HEAD, head_lines},
};
use beep_proto::SessionId;
use serde_json::Value;
use std::{
    collections::HashSet,
    env, fs,
    path::{Path, PathBuf},
};

const QUERY: &str = "SELECT m.chat_key, m.title, m.working_dir, m.created_at,
        (SELECT MAX(s.created_at) FROM sessions s WHERE s.chat_key = m.chat_key),
        (SELECT substr(s.content, 1, 1000) FROM sessions s
          WHERE s.chat_key = m.chat_key AND s.entry_kind = '{\"type\":\"user\"}'
          ORDER BY s.turn_index ASC LIMIT 1)
     FROM session_meta m
     WHERE m.working_dir IS NOT NULL AND m.working_dir != ''
     ORDER BY m.created_at DESC";

/// flsh's sessions folder, when it has one here.
pub(super) fn locate() -> Option<PathBuf> {
    sessions_dir().filter(|dir| dir.is_dir())
}

/// Where flsh keeps its sessions, as flsh finds it: `$FLASHMIND_HOME`, else
/// `~/.flashmind` when it exists, else `flashmind` in the platform's data
/// folder (`%LOCALAPPDATA%` on Windows, else `$XDG_DATA_HOME` or
/// `~/.local/share`, macOS too). It may not exist yet.
pub(crate) fn sessions_dir() -> Option<PathBuf> {
    let home = dirs::home_dir();
    let data = match env::var_os("FLASHMIND_HOME").filter(|value| !value.is_empty()) {
        Some(dir) => Some(PathBuf::from(dir)).filter(|dir| dir.is_absolute())?,
        None => match home.as_ref().map(|home| home.join(".flashmind")) {
            Some(legacy) if legacy.is_dir() => legacy,
            _ if cfg!(windows) => dirs::data_local_dir()?.join("flashmind"),
            _ => xdg_data_dir(home)?.join("flashmind"),
        },
    };
    Some(data.join("sessions"))
}

/// Unimported `sessions.db` databases in subfolders of `root`.
pub(in crate::agents) fn databases(root: &Path) -> impl Iterator<Item = PathBuf> {
    fs::read_dir(root)
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .map(|entry| entry.path())
        .filter(|dir| !dir.join(".sqlite-imported").exists())
        .map(|dir| dir.join("sessions.db"))
        .filter(|path| path.is_file())
}

/// The sessions under the sessions folder `root`: its JSONL files, then
/// those of the databases flsh has not imported.
pub(super) fn sessions(root: &Path, memos: &Memos) -> Vec<Seen> {
    let mut seen: Vec<Seen> = newest_files(root, 1, &is_jsonl)
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
    let mut ids: HashSet<SessionId> = seen.iter().map(|one| one.id.clone()).collect();
    seen.extend(
        read_databases(memos, databases(root), read, "flsh")
            .into_iter()
            .filter(|one| ids.insert(one.id.clone())),
    );
    seen
}

/// A session file's header and first prompt; `None` for one with no
/// folder or no message (a session flsh deleted keeps its header).
fn head(path: &Path) -> Option<Seen> {
    let mut lines = head_lines(path, MAX_HEAD).ok()?;
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
        id: Into::<SessionId>::into(id),
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
    let rows = statement.query_map([], |row| {
        let created: Option<i64> = row.get(3)?;
        let last: Option<i64> = row.get(4)?;
        let title: Option<String> = row.get(1)?;
        let first: Option<String> = row.get(5)?;
        Ok(Seen {
            agent: Agent::Flsh,
            id: Into::<SessionId>::into(row.get::<_, String>(0)?),
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
#[path = "../../tests/agents_found_flsh.rs"]
mod tests;
