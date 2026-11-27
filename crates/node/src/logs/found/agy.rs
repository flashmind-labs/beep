//! Antigravity's sessions (`conversations_dir`, `sessions`): a SQLite
//! database per conversation, `conversations/<id>.db` under
//! `~/.gemini/antigravity-cli`, whose `trajectory_metadata_blob` names the
//! folder it ran in and when it started. Titles, and which conversations a
//! session started as subagents, are in `conversation_summaries.db` beside
//! the folder.

use super::common::{Summaries, Agent, Memos, Seen, newest_files, open_database, prompt_title};
use beep_proto::SessionId;
use prost::Message;
use rusqlite::OptionalExtension;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

const TRAJECTORY: &str = "SELECT data FROM trajectory_metadata_blob LIMIT 1";
const HAS_STEPS: &str = "SELECT EXISTS (SELECT 1 FROM steps)";
const TITLES: &str = "SELECT conversation_id, title FROM conversation_summaries WHERE title != ''";
/// A conversation's first stretch of what the user said, for one with no title.
const FIRST_PROMPTS: &str =
    "SELECT conversation_id, substr(user_body, 1, 65536) FROM conversations_fts
     WHERE chunk_index = 0 AND user_body != ''";
const CHILDREN: &str = "SELECT conversation_id FROM conversation_summaries
     WHERE parent_conversation_id != ''
     UNION SELECT conversation_id FROM conversation_parents";

/// Where Antigravity keeps its conversations: `~/.gemini/antigravity-cli`,
/// which nothing but the home moves. It may not exist yet.
pub(crate) fn conversations_dir() -> Option<PathBuf> {
    Some(
        dirs::home_dir()?
            .join(".gemini")
            .join("antigravity-cli")
            .join("conversations"),
    )
}

pub(crate) fn is_conversation(path: &Path) -> bool {
    path.extension().is_some_and(|ext| ext == "db")
}

/// The conversations in `root` that the user started, newest first.
pub(crate) fn sessions(root: &Path, memos: &Memos) -> Vec<Seen> {
    let summaries = summaries_at(root)
        .and_then(|path| {
            memos
                .agy
                .read_database(&path, || read_summaries(&path).map(Arc::new))
                .inspect_err(|e| log::debug!("agy summaries not read; error={e}"))
                .ok()
        })
        .unwrap_or_default();

    newest_files(root, 0, &is_conversation)
        .into_iter()
        .filter_map(|(path, at)| {
            let mut seen = memos
                .heads
                .read_database(&path, || head(&path))
                .inspect_err(|e| log::debug!("agy conversation not read; path={} error={e}", path.display()))
                .ok()??;

            if summaries.children.contains(&seen.id) {
                return None;
            }

            // agy opens every conversation when it starts, which touches an
            // empty `-wal`: only one holding writes counts.
            let wal_at = std::fs::metadata(super::common::wal_path(&path))
                .ok()
                .filter(|meta| meta.len() > 0)
                .and_then(|meta| meta.modified().ok())
                .map_or(0, super::common::unix_secs);
            seen.last_activity = at.max(wal_at).max(seen.started_at);
            seen.title = summaries.titles.get(&seen.id).cloned();
            Some(seen)
        })
        .collect()
}

/// The summaries database beside the conversations folder `root`.
fn summaries_at(root: &Path) -> Option<PathBuf> {
    Some(root.parent()?.join("conversation_summaries.db")).filter(|path| path.is_file())
}

fn read_summaries(path: &Path) -> rusqlite::Result<Summaries> {
    let conn = open_database(path)?;
    let mut summaries = Summaries::default();

    // Titles are read last, so they win over first prompts.
    for query in [FIRST_PROMPTS, TITLES] {
        let mut statement = conn.prepare(query)?;
        let rows = statement.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?;

        for row in rows {
            let (id, text) = row?;
            if let Some(title) = prompt_title(&text) {
                summaries.titles.insert(Into::<SessionId>::into(id), title);
            }
        }
    }

    let mut statement = conn.prepare(CHILDREN)?;
    let rows = statement.query_map([], |row| row.get::<_, String>(0))?;

    for id in rows {
        summaries.children.insert(Into::<SessionId>::into(id?));
    }

    Ok(summaries)
}

/// The conversation at `database`: its id, folder and start; `None` for
/// one with no folder or no step yet.
fn head(database: &Path) -> rusqlite::Result<Option<Seen>> {
    let conn = open_database(database)?;
    let has_steps: bool = conn.query_row(HAS_STEPS, [], |row| row.get(0))?;

    if !has_steps {
        return Ok(None);
    }

    let Some(trajectory) = conn
        .query_row(TRAJECTORY, [], |row| row.get::<_, Vec<u8>>(0))
        .optional()?
        .and_then(|data| Trajectory::decode(data.as_slice()).ok())
    else {
        return Ok(None);
    };
    let cwd = trajectory
        .workspaces
        .first()
        .and_then(|workspace| url::Url::parse(&workspace.uri).ok()?.to_file_path().ok());
    let id = database
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .filter(|id| !id.is_empty());
    let (Some(cwd), Some(id)) = (cwd, id) else {
        return Ok(None);
    };
    let started = trajectory
        .created
        .and_then(|time| u64::try_from(time.seconds).ok())
        .unwrap_or(0);

    Ok(Some(Seen {
        agent: Agent::Agy,
        id: Into::<SessionId>::into(id),
        cwd,
        started_at: started,
        last_activity: started,
        title: None,
        file: None,
    }))
}

// Antigravity's records are protobuf without a published schema; only the
// fields read here are named, by their numbers as the CLI writes them.

/// The conversation's `trajectory_metadata_blob`.
#[derive(Clone, PartialEq, Message)]
pub(crate) struct Trajectory {
    #[prost(message, repeated, tag = "1")]
    pub(crate) workspaces: Vec<Workspace>,
    #[prost(message, optional, tag = "2")]
    pub(crate) created: Option<Time>,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct Workspace {
    /// A `file://` URI.
    #[prost(string, tag = "1")]
    pub(crate) uri: String,
}

/// A protobuf `Timestamp`, to the second.
#[derive(Clone, PartialEq, Message)]
pub(crate) struct Time {
    #[prost(int64, tag = "1")]
    pub(crate) seconds: i64,
}

#[cfg(test)]
#[path = "../../tests/logs_found_agy.rs"]
mod tests;
