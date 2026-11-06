//! Terminals, of a project or none: which background command or pty on which node.

use super::Store;
use anyhow::Result;
use chrono::{DateTime, Utc};
use sqlx::{Row, postgres::PgRow};
use uuid::Uuid;

/// One terminal as the store keeps it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TerminalRow {
    pub(crate) id: Uuid,
    /// `org/repo`, or empty for a terminal of no project.
    pub(crate) project: String,
    /// The node's key.
    pub(crate) endpoint: String,
    /// The node's number for the task, or for the terminal when `pty`.
    pub(crate) task: u32,
    /// An interactive terminal (`terminal/1`) rather than a background task (`process/1`).
    pub(crate) pty: bool,
    pub(crate) name: String,
    pub(crate) command: String,
    pub(crate) cwd: String,
    pub(crate) started_at: DateTime<Utc>,
}

pub(super) const COLUMNS: &str =
    "id, project, endpoint_id, task_id, name, command, cwd, started_at, pty";

pub(super) fn row_from(row: &PgRow) -> Result<TerminalRow> {
    let task: i64 = row.try_get(3)?;
    Ok(TerminalRow {
        id: row.try_get(0)?,
        project: row.try_get(1)?,
        endpoint: row.try_get(2)?,
        task: u32::try_from(task)?,
        name: row.try_get(4)?,
        command: row.try_get(5)?,
        cwd: row.try_get(6)?,
        started_at: row.try_get(7)?,
        pty: row.try_get(8)?,
    })
}

impl Store {}
