//! Where Claude Code keeps its files, and how it names a folder's sessions.

use std::{ffi::OsString, path::{Path, PathBuf}};

/// Claude Code's own folder: `~/.claude`, unless `CLAUDE_CONFIG_DIR` says
/// otherwise. `None` without a home folder.
pub(crate) fn config_dir() -> Option<PathBuf> {
    config_dir_in(std::env::var_os("CLAUDE_CONFIG_DIR"), dirs::home_dir())
}

fn config_dir_in(set: Option<OsString>, home: Option<PathBuf>) -> Option<PathBuf> {
    match set {
        Some(dir) if !dir.is_empty() => Some(PathBuf::from(dir)),
        _ => home.map(|home| home.join(".claude")),
    }
}

/// The folder of Claude Code's transcripts here, when it has one:
/// `<config>/projects/<folder>/<session>.jsonl`, a subagent's in
/// `<session>/subagents/`.
pub(crate) fn projects_dir() -> Option<PathBuf> {
    Some(config_dir()?.join("projects")).filter(|projects| projects.is_dir())
}

/// The folder name Claude Code files a folder's sessions under: every
/// character other than a letter or digit becomes `-`.
pub(crate) fn slug(folder: &Path) -> String {
    folder
        .to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

#[cfg(test)]
#[path = "../tests/agents_claude_dir.rs"]
mod tests;
