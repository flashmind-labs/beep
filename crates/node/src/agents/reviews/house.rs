//! A project's own reviewer: the custom agent named `reviewer` its
//! checkout defines for any harness, whose instructions go into the review
//! prompt. Beep writes no file. A definition is used only when it is the
//! same at the base as in the checkout, so the changes under review cannot
//! rewrite their own reviewer.

use super::super::work_folder::git;
use crate::custom_agents;
use beep_proto::{CustomAgent, CustomAgentScope};
use std::path::Path;

/// The names a project's reviewer goes by.
const NAMES: &[&str] = &["reviewer", "code-reviewer"];

/// A project's reviewer instructions and the file they came from.
#[derive(Debug, PartialEq)]
pub(crate) struct HouseRules {
    pub path: String,
    pub prompt: String,
}

/// The reviewer the checkout at `cwd` defines, the one for `harness` first,
/// when its file is unchanged against `base` (see `ReviewOrder::base`).
/// Blocks on the file system and git.
pub(super) fn find(cwd: &Path, base: &str, harness: &str) -> Option<HouseRules> {
    let mut found: Vec<CustomAgent> = custom_agents::list(cwd, None)
        .ok()?
        .into_iter()
        .filter(|agent| {
            agent.scope == CustomAgentScope::Project {}
                && agent.error.is_none()
                && !agent.prompt.trim().is_empty()
                && NAMES.contains(&agent.name.to_lowercase().as_str())
        })
        .collect();
    found.sort_by_key(|agent| agent.harness.as_ref() != harness);
    let at = base_commit(cwd, base)?;
    found
        .into_iter()
        .find(|agent| unchanged(cwd, &at, &agent.path))
        .map(|agent| HouseRules {
            path: agent.path,
            prompt: agent.prompt.trim().to_owned(),
        })
}

/// The commit the changes are measured from.
fn base_commit(cwd: &Path, base: &str) -> Option<String> {
    let base = base.trim();

    if base.starts_with('-') {
        return None;
    }

    let merge_base = |with: &str| git(cwd, &["merge-base", "HEAD", with]);
    let at = match base {
        "HEAD" => git(cwd, &["rev-parse", "HEAD"]),
        "" => ["origin/HEAD", "main", "master"]
            .into_iter()
            .find_map(merge_base),
        base => merge_base(base),
    }?;
    Some(at.trim().to_owned())
}

/// Whether `path`, relative to `cwd`, is at `at` and the same there as in
/// the checkout, its uncommitted edits included.
fn unchanged(cwd: &Path, at: &str, path: &str) -> bool {
    let blob = format!("{at}:./{path}");
    git(cwd, &["cat-file", "-e", &blob]).is_some()
        && git(cwd, &["diff", "--quiet", at, "--", path]).is_some()
}

#[cfg(test)]
#[path = "../../tests/agents_reviews_house.rs"]
mod tests;
