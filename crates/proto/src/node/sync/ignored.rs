//! Opt-in file sync for explicitly selected, wholly Git-ignored folders.

use serde::{Deserialize, Serialize};

pub const IGNORED_PREFIX: &str = "@ignored/";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IgnoredSync {
    pub enabled: bool,
    pub folders: Vec<String>,
}

impl IgnoredSync {
    /// Project-relative directory names, without overlapping roots or path traversal.
    pub fn normalized(mut self) -> Result<Self, &'static str> {
        if self.folders.len() > 16 {
            return Err("Choose at most 16 ignored folders.");
        }
        for folder in &mut self.folders {
            *folder = folder.trim().trim_end_matches('/').to_owned();
            if folder.is_empty()
                || folder.len() > 1024
                || folder.contains(['\\', ':', '\n', '\r', '\0'])
            {
                return Err("Use project-relative folder paths, such as assets/models.");
            }
            if folder.split('/').any(|part| {
                part.is_empty()
                    || matches!(
                        part.to_ascii_lowercase().as_str(),
                        "." | ".." | ".git" | ".worktrees" | "target" | "node_modules"
                    )
                    || part.ends_with([' ', '.'])
            }) {
                return Err(
                    "Choose plain relative folders outside Git metadata, worktrees and build output.",
                );
            }
        }
        self.folders.sort();
        self.folders.dedup();
        for (i, a) in self.folders.iter().enumerate() {
            let a = a.to_lowercase();
            for b in &self.folders[i + 1..] {
                let b = b.to_lowercase();
                if a == b || a.starts_with(&format!("{b}/")) || b.starts_with(&format!("{a}/")) {
                    return Err(
                        "Choose folders that do not overlap, including differences only in letter case.",
                    );
                }
            }
        }
        if self.enabled && self.folders.is_empty() {
            return Err("Choose at least one ignored folder before enabling sync.");
        }
        Ok(self)
    }
}

impl IgnoredSync {
    pub fn status(
        &self,
        main: &super::ProjectStatus,
        all: &[super::ProjectStatus],
    ) -> super::ProjectStatus {
        use super::State;
        let project = &main.name;
        let config = self;
        let mut status = main.clone();
        if !config.enabled {
            return status;
        }
        for folder in &config.folders {
            let name = ignored_project(project, folder);
            let Some(child) = all.iter().find(|s| s.name == name) else {
                status.state = State::Failed;
                let reason = format!(
                    "Ignored folder {folder}: not running yet; check that this machine supports ignored-folder sync."
                );
                status.error = Some(status.error.map_or_else(
                    || reason.clone(),
                    |previous| format!("{previous}\n{reason}"),
                ));
                continue;
            };
            status.files = status.files.saturating_add(child.files);
            status.bytes = status.bytes.saturating_add(child.bytes);
            status.pending = status.pending.saturating_add(child.pending);
            status
                .conflicts
                .extend(child.conflicts.iter().map(|p| format!("{folder}/{p}")));
            status
                .waiting
                .extend(child.waiting.iter().map(|p| format!("{folder}/{p}")));
            if status.progress.is_none() {
                status.progress = child.progress;
            }
            if status.hint.is_none() {
                status.hint = child.hint.clone();
            }
            if let Some(error) = &child.error {
                let reason = format!("Ignored folder {folder}: {error}");
                status.error = Some(status.error.map_or_else(
                    || reason.clone(),
                    |previous| format!("{previous}\n{reason}"),
                ));
            }
            status.state = if status.error.is_some()
                || status.state == State::Failed
                || child.state == State::Failed
            {
                State::Failed
            } else if status.state == State::Syncing || child.state == State::Syncing {
                State::Syncing
            } else if status.state == State::Scanning || child.state == State::Scanning {
                State::Scanning
            } else {
                status.state
            };
        }
        status
    }
}

/// A separate dataset identity keeps ignored content independent of Git commit sync.
pub fn ignored_project(project: &str, folder: &str) -> String {
    let project = project.replace('%', "%25").replace('/', "%2F");
    format!("{IGNORED_PREFIX}{project}/{folder}")
}

pub fn ignored_folder(name: &str) -> Option<(String, &str)> {
    let (project, folder) = name.strip_prefix(IGNORED_PREFIX)?.split_once('/')?;
    let project = project.replace("%2F", "/").replace("%25", "%");
    let valid = IgnoredSync {
        enabled: true,
        folders: vec![folder.to_owned()],
    }
    .normalized()
    .ok()?;
    (valid.folders.first().is_some_and(|f| f == folder)
        && ignored_project(&project, folder) == name)
        .then_some((project, folder))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_is_explicit_relative_and_does_not_overlap() {
        for paths in [
            vec![],
            vec!["../secret"],
            vec!["/assets"],
            vec![".git/objects"],
            vec!["node_modules"],
            vec!["assets", "assets/models"],
            vec!["Models", "models"],
            vec!["x\\y"],
        ] {
            assert!(
                IgnoredSync {
                    enabled: true,
                    folders: paths.into_iter().map(str::to_owned).collect()
                }
                .normalized()
                .is_err()
            );
        }
        let config = IgnoredSync {
            enabled: true,
            folders: vec![" models/ ".into(), "models".into(), "data/captures".into()],
        }
        .normalized()
        .unwrap();
        assert_eq!(config.folders, ["data/captures", "models"]);
        assert_eq!(
            ignored_folder(&ignored_project("acme/widget", "data/captures")),
            Some(("acme/widget".into(), "data/captures"))
        );
        assert!(ignored_folder("@ignored/acme%2Fwidget/../outside").is_none());
    }
}
