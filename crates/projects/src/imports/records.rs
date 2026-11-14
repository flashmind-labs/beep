//! The rows a user's imports and syncs are kept as.

use crate::model::RepoName;
use chrono::{DateTime, Utc};
use iroh::EndpointId;

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

/// The innermost imported project holding `cwd` on `endpoint`, if any.
pub fn imported_project_of(
    endpoint: &EndpointId,
    cwd: &str,
    imports: &[ImportRow],
) -> Option<String> {
    let endpoint = endpoint.to_string();
    let cwd = cwd.trim_end_matches(is_separator);
    let holds = |folder: &str| {
        let folder = folder.trim_end_matches(is_separator);
        !folder.is_empty()
            && cwd
                .strip_prefix(folder)
                .is_some_and(|rest| rest.is_empty() || rest.starts_with(is_separator))
    };
    imports
        .iter()
        .filter(|import| import.endpoint == endpoint && holds(&import.folder))
        .max_by_key(|import| import.folder.len())
        .map(|import| import.project.clone())
}

fn is_separator(c: char) -> bool {
    c == '/' || c == '\\'
}

#[cfg(test)]
#[path = "../tests/imports_records.rs"]
mod tests;
