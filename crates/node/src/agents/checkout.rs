//! The branch or worktree a new session starts on, as
//! `agent::CheckoutPlan` asks: made in the session's folder's git checkout
//! before the agent starts.

use super::folder;
use crate::found;
use anyhow::{Context, Result, bail, ensure};
use beep_proto::node::agent::{
    CheckoutPlan, prompt_branch, valid_branch, valid_ref, worktree_path,
};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};
use tokio::task::spawn_blocking;

/// What a session Beep made no branch or worktree for is told ahead of its
/// first prompt, so the agent makes its own when it changes files.
const WORKTREE_BRIEF: &str = "Beep: if you will change files and this folder is a git repository's main checkout, first create a new branch and a git worktree for it under the repository's .worktrees/ folder, both named for the work, and work there. Questions that change nothing need neither.";

/// The most branch names tried, `beep/<slug>` then `-2` and on.
const MAX_NAMES: u32 = 100;

/// Where a checkout plan put the session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Start {
    /// A new branch in the checkout itself.
    NewBranch,
    /// A new branch in a new worktree.
    NewWorktree,
    /// An existing branch, in the worktree that already had it.
    OwnWorktree,
    /// An existing branch, in a worktree made for it now.
    AddedWorktree,
}

/// What a checkout plan made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Made {
    /// The folder the session starts in: the one asked for, or its place
    /// in the worktree.
    pub(super) folder: PathBuf,
    /// The branch it is on.
    pub(super) branch: String,
    /// What a new branch was made from, as asked (`HEAD` when nothing
    /// was); the branch itself for an existing one.
    pub(super) base: String,
    pub(super) start: Start,
}

impl Made {
    /// The first line of the session's transcript, saying where it runs.
    pub(super) fn note(&self) -> String {
        let (branch, base, folder) = (&self.branch, &self.base, self.folder.display());
        match self.start {
            Start::NewBranch => format!("Started on a new branch {branch} from {base}."),
            Start::NewWorktree => {
                format!("Started on a new branch {branch} from {base}, in the worktree {folder}.")
            }
            Start::OwnWorktree => {
                format!("Started on the branch {branch}, in its worktree {folder}.")
            }
            Start::AddedWorktree => {
                format!("Started on the branch {branch}, in a new worktree {folder}.")
            }
        }
    }
}

/// Where a new session starts, and what it opens with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Place {
    /// The folder it starts in; the node's own default when `None`.
    pub(super) folder: Option<String>,
    /// The first line of its transcript, when a plan made a checkout.
    pub(super) note: Option<String>,
    /// Beep's word to the agent ahead of its first prompt.
    pub(super) brief: Option<&'static str>,
}

/// Where a session asked to start in `cwd` runs: what `plan` makes, or,
/// with no plan, `cwd` as it is, with the agent told to make its own
/// branch and worktree before it changes files.
pub(super) async fn place(
    cwd: Option<String>,
    plan: Option<CheckoutPlan>,
    prompt: &str,
) -> Result<Place> {
    let Some(plan) = plan else {
        return Ok(Place {
            folder: cwd,
            note: None,
            brief: Some(WORKTREE_BRIEF),
        });
    };
    let made = make_checkout(cwd.as_deref(), plan, prompt).await?;
    let folder = made
        .folder
        .to_str()
        .context("the new folder is not UTF-8")?
        .to_owned();
    Ok(Place {
        folder: Some(folder),
        note: Some(made.note()),
        brief: None,
    })
}

/// Carries out `plan` in the git checkout of `cwd` (as `folder` reads it),
/// naming a new branch from `prompt` when the plan names none.
async fn make_checkout(cwd: Option<&str>, plan: CheckoutPlan, prompt: &str) -> Result<Made> {
    let dir = folder(cwd)?;
    let prompt = prompt.to_owned();
    spawn_blocking(move || make(&dir, &plan, &prompt))
        .await
        .context("the checkout task stopped")?
}

/// The checkout a plan is carried out in.
struct Checkout {
    /// The top of the worktree `dir` is in.
    top: PathBuf,
    /// `dir` below `top`, empty at the top.
    prefix: String,
    /// The main checkout, where worktrees go.
    root: PathBuf,
}

impl Checkout {
    fn of(dir: &Path) -> Result<Self> {
        let said = git(dir, &["rev-parse", "--show-toplevel", "--show-prefix"])
            .with_context(|| format!("{} is not in a git checkout", dir.display()))?;
        let mut lines = said.lines();
        let top = PathBuf::from(lines.next().unwrap_or_default().trim());
        let prefix = lines
            .next()
            .unwrap_or_default()
            .trim()
            .trim_end_matches('/')
            .to_owned();
        // Worktrees go in the main checkout, also when asked from another
        // worktree of it.
        let root = found::checkout(&top).map_or_else(|| top.clone(), |checkout| checkout.root);
        Ok(Self { top, prefix, root })
    }

    /// The same subfolder in the worktree `tree`, when it has one.
    fn inside(&self, tree: PathBuf) -> PathBuf {
        let inner = tree.join(&self.prefix);
        if self.prefix.is_empty() || !inner.is_dir() {
            tree
        } else {
            inner
        }
    }
}

/// Carries out `plan` in the git checkout `dir` is in.
pub(super) fn make(dir: &Path, plan: &CheckoutPlan, prompt: &str) -> Result<Made> {
    let base = plan.base().unwrap_or("HEAD");
    ensure!(valid_ref(base), "{base:?} is not a branch or commit name");
    if let Some(branch) = plan.branch() {
        ensure!(valid_branch(branch), "{branch:?} is not a branch name");
    }
    let at = Checkout::of(dir)?;
    match plan {
        CheckoutPlan::NewBranch { .. } => {
            let start = start_point(&at.top, base)?;
            let changed = git(&at.top, &["status", "--porcelain", "--untracked-files=no"])?;
            ensure!(
                changed.trim().is_empty(),
                "the checkout at {} has uncommitted changes; commit or stash them, or start in a new worktree",
                at.top.display()
            );
            let branch = free_branch(&at.top, None, &prompt_branch(prompt))?;
            git(&at.top, &["switch", "--quiet", "-c", &branch, &start])?;
            Ok(Made {
                folder: dir.to_owned(),
                branch,
                base: base.to_owned(),
                start: Start::NewBranch,
            })
        }
        CheckoutPlan::NewWorktree { .. } => new_worktree(&at, base, &prompt_branch(prompt)),
        CheckoutPlan::NamedWorktree { branch, .. } => new_worktree(&at, base, branch),
        CheckoutPlan::BranchWorktree { branch } => branch_worktree(&at, branch),
        _ => bail!("this beep-node cannot make that checkout"),
    }
}

/// A new branch named `first` (or the first free name after it) from
/// `base`, in a new worktree.
fn new_worktree(at: &Checkout, base: &str, first: &str) -> Result<Made> {
    let start = start_point(&at.top, base)?;
    let branch = free_branch(&at.top, Some(&at.root), first)?;
    let tree = at.root.join(worktree_path(&branch));
    let path = tree
        .to_str()
        .context("the checkout's folder is not UTF-8")?;
    git(
        &at.top,
        &["worktree", "add", "--quiet", "-b", &branch, path, &start],
    )?;
    exclude_worktrees(&at.root)?;
    Ok(Made {
        folder: at.inside(tree),
        branch,
        base: base.to_owned(),
        start: Start::NewWorktree,
    })
}

/// The existing `branch`: in the worktree that has it checked out, else in
/// a new worktree on it, fetched from `origin` first.
fn branch_worktree(at: &Checkout, branch: &str) -> Result<Made> {
    let made = |folder, start| Made {
        folder,
        branch: branch.to_owned(),
        base: branch.to_owned(),
        start,
    };
    if let Some(tree) = worktree_of(&at.top, branch)? {
        return Ok(made(at.inside(tree), Start::OwnWorktree));
    }
    // A branch only in this checkout is fine without a remote.
    let fetched = git(&at.top, &["fetch", "--quiet", "origin", branch]);
    let tree = at.root.join(worktree_path(branch));
    ensure!(
        !tree.exists(),
        "{} is there already, but not as a worktree of {branch}",
        tree.display()
    );
    let path = tree
        .to_str()
        .context("the checkout's folder is not UTF-8")?;
    let local = format!("refs/heads/{branch}");
    let remote = format!("refs/remotes/origin/{branch}");
    if git(&at.top, &["show-ref", "--verify", "--quiet", &local]).is_ok() {
        git(&at.top, &["worktree", "add", "--quiet", path, branch])?;
    } else if git(&at.top, &["show-ref", "--verify", "--quiet", &remote]).is_ok() {
        let tracked = format!("origin/{branch}");
        git(
            &at.top,
            &[
                "worktree", "add", "--quiet", "--track", "-b", branch, path, &tracked,
            ],
        )?;
    } else {
        let why = fetched
            .err()
            .map(|e| format!(" ({e:#})"))
            .unwrap_or_default();
        bail!(
            "there is no branch {branch} in the checkout at {} or on origin{why}",
            at.top.display()
        );
    }
    exclude_worktrees(&at.root)?;
    Ok(made(at.inside(tree), Start::AddedWorktree))
}

/// The folder of the worktree of the checkout at `top` that has `branch`
/// checked out, if one does.
fn worktree_of(top: &Path, branch: &str) -> Result<Option<PathBuf>> {
    let listed = git(top, &["worktree", "list", "--porcelain"])?;
    let want = format!("branch refs/heads/{branch}");
    for block in listed.split(
        "

",
    ) {
        let mut lines = block.lines();
        let Some(path) = lines.next().and_then(|line| line.strip_prefix("worktree ")) else {
            continue;
        };
        if lines.any(|line| line == want) {
            return Ok(Some(PathBuf::from(path)));
        }
    }
    Ok(None)
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

/// The first of `first`, `first-2` and on that is no branch in the
/// checkout at `top` and, for a worktree, no folder in `root`.
fn free_branch(top: &Path, root: Option<&Path>, first: &str) -> Result<String> {
    for n in 1..=MAX_NAMES {
        let name = if n == 1 {
            first.to_owned()
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

    #[tokio::test]
    async fn a_plain_start_makes_no_branch() {
        let (_dir, work) = repo("plain");
        let cwd = work.to_str().expect("a UTF-8 folder").to_owned();
        let prompt = "do we need to update any methods? check the api";
        let place = place(Some(cwd.clone()), None, prompt)
            .await
            .expect("a place");
        assert_eq!(place.folder.as_deref(), Some(cwd.as_str()));
        assert_eq!(place.note, None, "no branch line");
        assert_eq!(place.brief, Some(WORKTREE_BRIEF));
        assert_eq!(current(&work), "main");
        assert_eq!(run(&work, &["branch", "--format=%(refname:short)"]), "main");
        assert!(!work.join(".worktrees").exists(), "no worktree");
    }

    #[tokio::test]
    async fn a_plan_starts_in_what_it_made_without_the_brief() {
        let (_dir, work) = repo("planned");
        let cwd = work.to_str().expect("a UTF-8 folder").to_owned();
        let plan = CheckoutPlan::NewWorktree { base: None };
        let place = place(Some(cwd), Some(plan), "Fix it")
            .await
            .expect("a place");
        assert_eq!(place.brief, None);
        let folder = place.folder.expect("the worktree");
        assert!(folder.ends_with(".worktrees/beep-fix-it"), "{folder}");
        assert!(
            place
                .note
                .expect("a note")
                .starts_with("Started on a new branch beep/fix-it")
        );
    }

    #[test]
    fn a_new_branch_switches_the_checkout() {
        let (_dir, work) = repo("branch");
        let plan = CheckoutPlan::NewBranch { base: None };
        let made = make(&work, &plan, "Fix the login bug!").expect("the branch");
        assert_eq!(made.branch, "beep/fix-the-login-bug");
        assert_eq!(made.folder, work);
        assert_eq!(made.start, Start::NewBranch);
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
        assert_eq!(made.start, Start::NewWorktree);
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
    fn a_named_worktree_takes_its_name_and_the_next_number() {
        let (_dir, work) = repo("named");
        let plan = CheckoutPlan::NamedWorktree {
            base: Some("main".into()),
            branch: "issue-52-crash-on-rotate".into(),
        };
        let made = make(&work, &plan, "Work on issue #52").expect("the worktree");
        assert_eq!(made.branch, "issue-52-crash-on-rotate");
        assert_eq!(
            made.folder,
            work.join(".worktrees/issue-52-crash-on-rotate")
        );
        assert_eq!(made.start, Start::NewWorktree);
        assert_eq!(current(&made.folder), "issue-52-crash-on-rotate");
        let again = make(&work, &plan, "Work on issue #52").expect("a second worktree");
        assert_eq!(again.branch, "issue-52-crash-on-rotate-2");
        let bad = CheckoutPlan::NamedWorktree {
            base: None,
            branch: "-x".into(),
        };
        assert!(make(&work, &bad, "go").is_err());
    }

    #[test]
    fn a_branch_worktree_is_reused_or_added_from_origin() {
        let (dir, work) = repo("continue");
        run(&work, &["branch", "login-copy"]);
        let clone = dir.path().join("clone");
        let from = work.to_str().expect("a UTF-8 path");
        let to = clone.to_str().expect("a UTF-8 path");
        run(dir.path(), &["clone", "--quiet", from, to]);
        // Pushed after the clone: only a fetch finds it.
        run(&work, &["branch", "late"]);
        let plan = |branch: &str| CheckoutPlan::BranchWorktree {
            branch: branch.into(),
        };
        let made = make(&clone.join("src"), &plan("late"), "go").expect("from origin");
        let tree = clone.join(".worktrees/late");
        assert_eq!(made.start, Start::AddedWorktree);
        assert_eq!(made.folder, tree.join("src"));
        assert_eq!(current(&tree), "late");
        assert_eq!(
            run(&tree, &["rev-parse", "--abbrev-ref", "late@{upstream}"]),
            "origin/late",
            "it tracks origin"
        );
        assert_eq!(
            made.note(),
            format!(
                "Started on the branch late, in a new worktree {}.",
                tree.join("src").display()
            )
        );
        let reused = make(&clone, &plan("late"), "go").expect("its worktree");
        assert_eq!(reused.start, Start::OwnWorktree);
        assert_eq!(reused.folder, tree);
        // A worktree made by hand elsewhere is found too.
        let other = dir.path().join("elsewhere");
        let path = other.to_str().expect("a UTF-8 path");
        run(&clone, &["worktree", "add", "--quiet", path, "login-copy"]);
        let found = make(&clone, &plan("login-copy"), "go").expect("the hand-made worktree");
        assert_eq!(found.start, Start::OwnWorktree);
        assert_eq!(
            fs::canonicalize(&found.folder).expect("a folder"),
            fs::canonicalize(&other).expect("a folder")
        );
        let error = make(&clone, &plan("nope"), "go").expect_err("no such branch");
        assert!(format!("{error:#}").contains("no branch nope"), "{error:#}");
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
