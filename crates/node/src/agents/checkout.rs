//! The branch or worktree a new session starts on, as
//! `agent::CheckoutPlan` asks: made in the session's folder's git checkout
//! before the agent starts.

use super::folder;
use crate::found;
use anyhow::{Context, Result, bail, ensure};
use beep_proto::node::agent::{CheckoutPlan, prompt_branch, valid_ref, worktree_path};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};
use tokio::task::spawn_blocking;

/// The most branch names tried, `beep/<slug>` then `-2` and on.
const MAX_NAMES: u32 = 100;

/// What a checkout plan made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Made {
    /// The folder the session starts in: the one asked for, or its place
    /// in the new worktree.
    pub(super) folder: PathBuf,
    /// The new branch.
    pub(super) branch: String,
    /// What the branch was made from, as asked (`HEAD` when nothing was).
    pub(super) base: String,
    /// Whether it is in a new worktree rather than the checkout itself.
    pub(super) worktree: bool,
}

impl Made {
    /// The first line of the session's transcript, saying where it runs.
    pub(super) fn note(&self) -> String {
        if self.worktree {
            format!(
                "Started on a new branch {} from {}, in the worktree {}.",
                self.branch,
                self.base,
                self.folder.display()
            )
        } else {
            format!(
                "Started on a new branch {} from {}.",
                self.branch, self.base
            )
        }
    }
}

/// Carries out `plan` in the git checkout of `cwd` (as `folder` reads it),
/// naming the branch from `prompt`.
pub(super) async fn make_checkout(
    cwd: Option<&str>,
    plan: CheckoutPlan,
    prompt: &str,
) -> Result<Made> {
    let dir = folder(cwd)?;
    let prompt = prompt.to_owned();
    spawn_blocking(move || make(&dir, &plan, &prompt))
        .await
        .context("the checkout task stopped")?
}

/// Carries out `plan` in the git checkout `dir` is in.
pub(super) fn make(dir: &Path, plan: &CheckoutPlan, prompt: &str) -> Result<Made> {
    let base = plan.base().unwrap_or("HEAD");
    ensure!(valid_ref(base), "{base:?} is not a branch or commit name");
    let said = git(dir, &["rev-parse", "--show-toplevel", "--show-prefix"])
        .with_context(|| format!("{} is not in a git checkout", dir.display()))?;
    let mut lines = said.lines();
    let top = PathBuf::from(lines.next().unwrap_or_default().trim());
    let prefix = lines.next().unwrap_or_default().trim().to_owned();
    // Worktrees go in the main checkout, also when asked from another
    // worktree of it.
    let root = found::checkout(&top).map_or_else(|| top.clone(), |checkout| checkout.root);
    let start = start_point(&top, base)?;
    match plan {
        CheckoutPlan::NewBranch { .. } => {
            let changed = git(&top, &["status", "--porcelain", "--untracked-files=no"])?;
            ensure!(
                changed.trim().is_empty(),
                "the checkout at {} has uncommitted changes; commit or stash them, or start in a new worktree",
                top.display()
            );
            let branch = free_branch(&top, None, prompt)?;
            git(&top, &["switch", "--quiet", "-c", &branch, &start])?;
            Ok(Made {
                folder: dir.to_owned(),
                branch,
                base: base.to_owned(),
                worktree: false,
            })
        }
        CheckoutPlan::NewWorktree { .. } => {
            let branch = free_branch(&top, Some(&root), prompt)?;
            let tree = root.join(worktree_path(&branch));
            let path = tree
                .to_str()
                .context("the checkout's folder is not UTF-8")?;
            git(
                &top,
                &["worktree", "add", "--quiet", "-b", &branch, path, &start],
            )?;
            exclude_worktrees(&root)?;
            // The same subfolder in the worktree, when it has one.
            let inner = tree.join(&prefix);
            let folder = if prefix.is_empty() || !inner.is_dir() {
                tree
            } else {
                inner
            };
            Ok(Made {
                folder,
                branch,
                base: base.to_owned(),
                worktree: true,
            })
        }
        _ => bail!("this beep-node cannot make that checkout"),
    }
}

/// The commit `base` names in the checkout at `top`, else on `origin`.
fn start_point(top: &Path, base: &str) -> Result<String> {
    let mut candidates = vec![base.to_owned()];
    if base != "HEAD" {
        candidates.push(format!("origin/{base}"));
    }
    for candidate in candidates {
        let commit = format!("{candidate}^{{commit}}");
        if let Ok(sha) = git(top, &["rev-parse", "--verify", "--quiet", &commit]) {
            return Ok(sha.trim().to_owned());
        }
    }
    if base == "HEAD" {
        bail!("the checkout at {} has no commits yet", top.display());
    }
    bail!("there is no {base} in the checkout at {}", top.display())
}

/// The first of `beep/<slug>`, `beep/<slug>-2` and on that is no branch
/// in the checkout at `top` and, for a worktree, no folder in `root`.
fn free_branch(top: &Path, root: Option<&Path>, prompt: &str) -> Result<String> {
    let first = prompt_branch(prompt);
    for n in 1..=MAX_NAMES {
        let name = if n == 1 {
            first.clone()
        } else {
            format!("{first}-{n}")
        };
        let head = format!("refs/heads/{name}");
        let branch = git(top, &["show-ref", "--verify", "--quiet", &head]).is_ok();
        let folder = root.is_some_and(|root| root.join(worktree_path(&name)).exists());
        if !branch && !folder {
            return Ok(name);
        }
    }
    bail!(
        "{first} and the next {} names after it are taken",
        MAX_NAMES - 1
    )
}

/// Keeps `.worktrees/` out of `git status` in the checkout at `root`: adds
/// it to `info/exclude` unless git ignores it already. The repo's own
/// `.gitignore` is left alone.
fn exclude_worktrees(root: &Path) -> Result<()> {
    // `.worktrees` exists now, so a pattern for folders only matches it.
    if git(root, &["check-ignore", "--quiet", ".worktrees"]).is_ok() {
        return Ok(());
    }
    let path = git(root, &["rev-parse", "--git-path", "info/exclude"])?;
    let path = root.join(path.trim());
    if let Some(info) = path.parent() {
        fs::create_dir_all(info).with_context(|| format!("making {}", info.display()))?;
    }
    let had = fs::read_to_string(&path).unwrap_or_default();
    let line = if had.is_empty() || had.ends_with('\n') {
        ".worktrees/\n"
    } else {
        "\n.worktrees/\n"
    };
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .and_then(|mut file| file.write_all(line.as_bytes()))
        .with_context(|| format!("adding .worktrees/ to {}", path.display()))
}

/// What `git <args>` printed in `dir`; its error output when it failed.
fn git(dir: &Path, args: &[&str]) -> Result<String> {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .output()
        .context("running git")?;
    if !out.status.success() {
        let said = String::from_utf8_lossy(&out.stderr);
        bail!(
            "git {} failed: {}",
            args.first().copied().unwrap_or_default(),
            said.trim()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A temporary repository with one commit on `main`.
    fn repo(name: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::Builder::new()
            .prefix(&format!("beep-checkout-{name}"))
            .tempdir()
            .expect("a temporary folder");
        let work = dir.path().join("work");
        fs::create_dir_all(work.join("src")).expect("the work folder");
        run(&work, &["init", "--quiet", "--initial-branch=main"]);
        fs::write(work.join("README.md"), "hi\n").expect("a file");
        fs::write(work.join("src/lib.rs"), "\n").expect("a file");
        run(&work, &["add", "."]);
        run(&work, &["commit", "--quiet", "-m", "first"]);
        (dir, work)
    }

    /// Runs git with a fixed identity, as a test sets a repository up.
    fn run(dir: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@example.com")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@example.com")
            .output()
            .expect("git runs");
        assert!(out.status.success(), "git {args:?}: {out:?}");
        String::from_utf8_lossy(&out.stdout).trim().to_owned()
    }

    fn current(dir: &Path) -> String {
        run(dir, &["branch", "--show-current"])
    }

    #[test]
    fn a_new_branch_switches_the_checkout() {
        let (_dir, work) = repo("branch");
        let plan = CheckoutPlan::NewBranch { base: None };
        let made = make(&work, &plan, "Fix the login bug!").expect("the branch");
        assert_eq!(made.branch, "beep/fix-the-login-bug");
        assert_eq!(made.folder, work);
        assert!(!made.worktree);
        assert_eq!(current(&work), "beep/fix-the-login-bug");
        assert_eq!(
            made.note(),
            "Started on a new branch beep/fix-the-login-bug from HEAD."
        );
    }

    #[test]
    fn a_new_branch_is_refused_on_uncommitted_changes() {
        let (_dir, work) = repo("dirty");
        fs::write(work.join("README.md"), "changed\n").expect("a change");
        let plan = CheckoutPlan::NewBranch { base: None };
        let error = make(&work, &plan, "go").expect_err("a dirty checkout");
        assert!(
            format!("{error:#}").contains("uncommitted changes"),
            "{error:#}"
        );
        assert_eq!(current(&work), "main", "the checkout stays put");
        // A file git does not track yet is no change to carry.
        run(&work, &["checkout", "--", "README.md"]);
        fs::write(work.join("notes.txt"), "mine\n").expect("a new file");
        make(&work, &plan, "go").expect("only an untracked file");
    }

    #[test]
    fn a_new_worktree_goes_under_dot_worktrees_and_is_excluded() {
        let (_dir, work) = repo("worktree");
        run(&work, &["branch", "base-here"]);
        run(
            &work,
            &["commit", "--quiet", "--allow-empty", "-m", "second"],
        );
        let plan = CheckoutPlan::NewWorktree {
            base: Some("base-here".into()),
        };
        let made = make(&work.join("src"), &plan, "Add dark mode").expect("the worktree");
        let tree = work.join(".worktrees/beep-add-dark-mode");
        assert_eq!(made.branch, "beep/add-dark-mode");
        assert_eq!(made.folder, tree.join("src"), "the same subfolder");
        assert!(made.worktree);
        assert_eq!(current(&tree), "beep/add-dark-mode");
        assert_eq!(current(&work), "main", "the checkout keeps its branch");
        assert_eq!(
            run(&tree, &["rev-parse", "HEAD"]),
            run(&work, &["rev-parse", "base-here"]),
            "made from the base"
        );
        let exclude = fs::read_to_string(work.join(".git/info/exclude")).expect("the exclude");
        assert!(
            exclude.lines().any(|line| line == ".worktrees/"),
            "{exclude}"
        );
        assert_eq!(run(&work, &["status", "--porcelain"]), "");
        // Once excluded, it is not added again.
        make(&work, &plan, "Add dark mode").expect("a second worktree");
        let again = fs::read_to_string(work.join(".git/info/exclude")).expect("the exclude");
        assert_eq!(again, exclude);
    }

    #[test]
    fn a_worktree_already_ignored_leaves_the_exclude_alone() {
        let (_dir, work) = repo("ignored");
        fs::write(work.join(".gitignore"), ".worktrees/\n").expect("a gitignore");
        run(&work, &["add", ".gitignore"]);
        run(&work, &["commit", "--quiet", "-m", "ignore"]);
        let before = fs::read_to_string(work.join(".git/info/exclude")).unwrap_or_default();
        let plan = CheckoutPlan::NewWorktree { base: None };
        make(&work, &plan, "go").expect("the worktree");
        let after = fs::read_to_string(work.join(".git/info/exclude")).unwrap_or_default();
        assert_eq!(after, before);
        assert_eq!(
            fs::read_to_string(work.join(".gitignore")).expect("the gitignore"),
            ".worktrees/\n"
        );
    }

    #[test]
    fn a_taken_name_gets_the_next_number() {
        let (_dir, work) = repo("taken");
        run(&work, &["branch", "beep/fix-it"]);
        fs::create_dir_all(work.join(".worktrees/beep-fix-it-2")).expect("a folder");
        let plan = CheckoutPlan::NewWorktree { base: None };
        let made = make(&work, &plan, "fix it").expect("the worktree");
        assert_eq!(made.branch, "beep/fix-it-3");
        assert_eq!(made.folder, work.join(".worktrees/beep-fix-it-3"));
        let branch =
            make(&work, &CheckoutPlan::NewBranch { base: None }, "fix it").expect("the branch");
        assert_eq!(
            branch.branch, "beep/fix-it-2",
            "only a branch takes a branch name"
        );
    }

    #[test]
    fn a_base_only_on_origin_or_nowhere() {
        let (dir, work) = repo("origin");
        let clone = dir.path().join("clone");
        let from = work.to_str().expect("a UTF-8 path");
        let to = clone.to_str().expect("a UTF-8 path");
        run(dir.path(), &["clone", "--quiet", from, to]);
        run(&work, &["branch", "release"]);
        run(&clone, &["fetch", "--quiet", "origin"]);
        let plan = CheckoutPlan::NewBranch {
            base: Some("release".into()),
        };
        let made = make(&clone, &plan, "ship").expect("from origin/release");
        assert_eq!(made.base, "release");
        let missing = CheckoutPlan::NewBranch {
            base: Some("nope".into()),
        };
        let error = make(&clone, &missing, "ship").expect_err("no such base");
        assert!(
            format!("{error:#}").contains("there is no nope"),
            "{error:#}"
        );
        let bad = CheckoutPlan::NewBranch {
            base: Some("--orphan".into()),
        };
        assert!(make(&clone, &bad, "ship").is_err());
    }

    #[test]
    fn a_folder_outside_git_is_refused() {
        let dir = tempfile::tempdir().expect("a temporary folder");
        let plan = CheckoutPlan::NewWorktree { base: None };
        let error = make(dir.path(), &plan, "go").expect_err("no checkout");
        assert!(
            format!("{error:#}").contains("not in a git checkout"),
            "{error:#}"
        );
    }
}
