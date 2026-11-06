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

/// One machine that syncs a project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncRow {
    pub project: String,
    /// The node's key, as the store keeps it.
    pub endpoint: String,
    pub folder: String,
}

/// The innermost imported project holding `cwd` on `endpoint`, else the folder's own name.
pub fn project_of(endpoint: &EndpointId, cwd: &str, imports: &[ImportRow]) -> String {
    imported_project_of(endpoint, cwd, imports).unwrap_or_else(|| {
        let cwd = cwd.trim_end_matches(is_separator);
        match cwd.rsplit(is_separator).next() {
            Some(name) if !name.is_empty() => name.to_owned(),
            _ => cwd.to_owned(),
        }
    })
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
mod tests {
    use super::*;
    use iroh::SecretKey;

    fn machine(seed: u8) -> EndpointId {
        SecretKey::from_bytes(&[seed; 32]).public()
    }

    fn import(folder: &str, project: &str) -> ImportRow {
        ImportRow {
            project: project.into(),
            endpoint: machine(1).to_string(),
            folder: folder.into(),
            url: String::new(),
            imported_at: DateTime::<Utc>::UNIX_EPOCH,
            fork: None,
        }
    }

    #[test]
    fn sessions_go_to_the_imported_project_that_holds_them() {
        let imports = [
            import("/srv/work/widget", "acme/widget"),
            import("/srv/work/widget/vendor/relay", "acme/relay"),
        ];
        assert_eq!(
            project_of(&machine(1), "/srv/work/widget", &imports),
            "acme/widget"
        );
        assert_eq!(
            project_of(&machine(1), "/srv/work/widget/.worktrees/fix", &imports),
            "acme/widget"
        );
        assert_eq!(
            project_of(&machine(1), "/srv/work/widget/vendor/relay/src", &imports),
            "acme/relay"
        );
        assert_eq!(
            project_of(&machine(1), "/srv/work/widgets", &imports),
            "widgets"
        );
        assert_eq!(
            project_of(&machine(2), "/srv/work/widget", &imports),
            "widget"
        );
    }
}
