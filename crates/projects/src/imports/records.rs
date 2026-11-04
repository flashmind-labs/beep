//! The rows a user's imports and syncs are kept as.

use crate::model::RepoName;
use chrono::{DateTime, Utc};

/// One project imported onto one machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportRow {
    /// `org/repo`.
    pub project: String,
    /// The node's key, as the store keeps it.
    pub endpoint: String,
    pub folder: String,
    /// The repository's web page.
    pub url: String,
    pub imported_at: DateTime<Utc>,
    /// The user's fork cloned as `origin`, `None` for the repository itself.
    pub fork: Option<RepoName>,
}

pub struct NewImport<'a> {
    pub project: &'a str,
    pub endpoint: &'a str,
    pub folder: &'a str,
    pub url: &'a str,
}

/// One machine that syncs a project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncRow {
    pub project: String,
    /// The node's key, as the store keeps it.
    pub endpoint: String,
    pub folder: String,
}
