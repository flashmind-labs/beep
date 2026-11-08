//! A project's own reviewer: the custom agent named `reviewer` its
//! checkout defines for any harness, whose instructions go into the review
//! prompt. Beep writes no file. A definition is used only when it is the
//! same at the base as in the checkout, so the changes under review cannot
//! rewrite their own reviewer.

use super::super::{custom_agents, work_folder::git};
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
    found.sort_by_key(|agent| agent.harness.as_str() != harness);
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
mod tests {
    use super::*;
    use std::{fs, process::Command};

    fn run(dir: &Path, args: &[&str]) {
        let ok = Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@example.com")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@example.com")
            .status()
            .expect("git runs")
            .success();
        assert!(ok, "git {args:?}");
    }

    const REVIEWER: &str =
        "---\nname: reviewer\ndescription: Reviews\n---\nCheck every SQL query for injection.\n";

    #[test]
    fn a_reviewer_counts_only_as_the_base_has_it() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path();
        run(root, &["init", "-q", "-b", "main"]);
        fs::create_dir_all(root.join(".claude/agents")).expect("folder");
        fs::write(root.join("README"), "hi").expect("file");
        run(root, &["add", "."]);
        run(root, &["commit", "-qm", "start"]);
        fs::write(root.join(".claude/agents/reviewer.md"), REVIEWER).expect("file");
        assert_eq!(
            find(root, "HEAD", "claude"),
            None,
            "a reviewer the changes add is not used"
        );
        run(root, &["add", "."]);
        run(root, &["commit", "-qm", "reviewer"]);
        let found = find(root, "HEAD", "codex").expect("the committed reviewer");
        assert_eq!(found.path, ".claude/agents/reviewer.md");
        assert_eq!(found.prompt, "Check every SQL query for injection.");
        fs::write(
            root.join(".claude/agents/reviewer.md"),
            REVIEWER.replace("Check", "Approve"),
        )
        .expect("file");
        assert_eq!(
            find(root, "HEAD", "claude"),
            None,
            "a reviewer the changes edit is not used"
        );
        assert_eq!(find(root, "--output=x", "claude"), None);
    }
}
