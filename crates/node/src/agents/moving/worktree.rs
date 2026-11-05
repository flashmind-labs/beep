//! A session in a linked worktree moves as its branch and its uncommitted
//! work: the source sends the head commit and a patch of what is not
//! committed, the target makes a worktree on the same branch and applies it.
//! Everything here blocks: call it from `spawn_blocking`.

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
};

/// The largest patch of uncommitted work that moves.
pub(super) const MAX_PATCH: usize = 64 * 1024 * 1024;

/// What a worktree holds beyond its branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Work {
    /// The commit checked out.
    pub(crate) head: String,
    /// Tracked changes and new files, as `git apply` takes them.
    pub(crate) patch: Vec<u8>,
}

/// How the worktree for a move is got.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) enum Placed {
    /// Made for the move.
    Made,
    /// The detached worktree a session left here earlier, put on the branch
    /// again; `prior` is the commit it was at.
    Reused { prior: String },
}

/// What `attach` did, and how to know later that the worktree still holds
/// only what it put there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Attached {
    pub(super) placed: Placed,
    /// `fingerprint` of the worktree once the patch was applied.
    pub(super) applied: String,
}

/// What the folder for the worktree allows.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Spot {
    Free,
    /// A clean detached worktree of this checkout is there, at `prior`.
    Detached {
        prior: String,
    },
}

/// Git's diff options that do not depend on the user's configuration.
const DIFF_OPTIONS: [&str; 6] = [
    "--binary",
    "--no-color",
    "--no-ext-diff",
    "--no-textconv",
    "--src-prefix=a/",
    "--dst-prefix=b/",
];

fn command(dir: &Path, args: &[&str]) -> Command {
    let mut command = Command::new("git");
    command
        .args(args)
        .current_dir(dir)
        .env("GIT_TERMINAL_PROMPT", "0");
    command
}

fn run(dir: &Path, args: &[&str], stdin: Option<&[u8]>) -> Result<Vec<u8>> {
    let mut child = command(dir, args)
        .stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("running git")?;
    if let (Some(bytes), Some(mut pipe)) = (stdin, child.stdin.take())
        && let Err(e) = pipe.write_all(bytes)
    {
        stop(&mut child);
        return Err(e).context("sending to git");
    }
    let out = child.wait_with_output().context("waiting for git")?;
    if !out.status.success() {
        bail!(
            "git {} failed: {}",
            args.first().copied().unwrap_or_default(),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(out.stdout)
}

/// Stops a git that is not wanted any more and waits for it, so none is left
/// behind.
fn stop(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

/// What `git args` printed, at most `cap` bytes: git is stopped at the
/// first byte over, so a huge file is never held whole. Exit codes in `ok`
/// besides 0 count as success. Output cut short by an error or by git
/// failing is an error, never a shorter patch.
fn capped(dir: &Path, args: &[&str], cap: usize, ok: &[i32]) -> Result<Vec<u8>> {
    let mut child = command(dir, args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .context("running git")?;
    let Some(reader) = child.stdout.take() else {
        stop(&mut child);
        bail!("git has no output");
    };
    let mut out = Vec::new();
    if let Err(e) = reader.take(cap as u64 + 1).read_to_end(&mut out) {
        stop(&mut child);
        return Err(e).context("reading git's output");
    }
    if out.len() > cap {
        stop(&mut child);
        bail!("the uncommitted changes are too large to move");
    }
    let status = child.wait().context("waiting for git")?;
    if !status.success() && !status.code().is_some_and(|code| ok.contains(&code)) {
        bail!("git {} failed", args.first().copied().unwrap_or_default());
    }
    Ok(out)
}

fn text(dir: &Path, args: &[&str]) -> Result<String> {
    Ok(String::from_utf8_lossy(&run(dir, args, None)?)
        .trim()
        .to_owned())
}

/// The head commit of the worktree at `dir`, and a patch of everything not
/// committed in it: staged and unstaged changes, and files git does not
/// track yet (those it ignores stay).
pub(super) fn snapshot(dir: &Path) -> Result<Work> {
    let head = text(dir, &["rev-parse", "HEAD"]).context("reading the worktree's commit")?;
    let mut args = vec!["diff", "HEAD"];
    args.extend(DIFF_OPTIONS);
    let mut patch =
        capped(dir, &args, MAX_PATCH, &[]).context("reading the uncommitted changes")?;
    let others = run(
        dir,
        &["ls-files", "--others", "--exclude-standard", "-z"],
        None,
    )
    .context("listing new files")?;
    for name in others.split(|byte| *byte == 0).filter(|n| !n.is_empty()) {
        let name = String::from_utf8_lossy(name).into_owned();
        let left = MAX_PATCH.saturating_sub(patch.len());
        // A file over the budget is not read at all.
        let size = std::fs::symlink_metadata(dir.join(&name)).map_or(0, |meta| meta.len());
        if size > left as u64 {
            bail!("the uncommitted changes are too large to move");
        }
        let null = if cfg!(windows) { "NUL" } else { "/dev/null" };
        let mut args = vec!["diff", "--no-index"];
        args.extend(DIFF_OPTIONS);
        args.extend(["--", null, &name]);
        // `--no-index` exits 1 when the files differ, which is what a new
        // file does against nothing.
        let out = capped(dir, &args, left, &[1]).with_context(|| format!("reading {name}"))?;
        patch.extend_from_slice(&out);
    }
    Ok(Work { head, patch })
}

/// The worktrees of the checkout `main` with the branch each has checked
/// out, none for a detached one.
fn listed(main: &Path) -> Result<Vec<(PathBuf, Option<String>)>> {
    let out = text(main, &["worktree", "list", "--porcelain"])?;
    let mut found: Vec<(PathBuf, Option<String>)> = Vec::new();
    for line in out.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            found.push((PathBuf::from(path), None));
        } else if let Some(branch) = line.strip_prefix("branch refs/heads/")
            && let Some(last) = found.last_mut()
        {
            last.1 = Some(branch.to_owned());
        }
    }
    Ok(found)
}

fn same_folder(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

/// Whether a worktree of the checkout `main` can be on `branch` at `path`:
/// the branch is checked out nowhere here, and the folder is free or holds
/// a clean detached worktree of this checkout (one a session left) at a
/// commit the branch contains, with no session running in it
/// (`running_here`). Done before the source ends anything.
pub(super) fn check(main: &Path, path: &Path, branch: &str, running_here: bool) -> Result<()> {
    spot(main, path, branch, running_here).map(|_| ())
}

fn spot(main: &Path, path: &Path, branch: &str, running_here: bool) -> Result<Spot> {
    if branch.starts_with('-') {
        bail!("{branch} is not a branch name");
    }
    text(main, &["check-ref-format", "--branch", branch])
        .with_context(|| format!("{branch} is not a branch name"))?;
    let worktrees = listed(main)?;
    if let Some((there, _)) = worktrees
        .iter()
        .find(|(_, on)| on.as_deref() == Some(branch))
    {
        bail!("{branch} is checked out in {} here", there.display());
    }
    if !path.exists() {
        return Ok(Spot::Free);
    }
    let detached = worktrees
        .iter()
        .any(|(there, on)| on.is_none() && same_folder(there, path));
    if !detached {
        bail!("{} is taken on this machine", path.display());
    }
    if running_here {
        bail!("a session runs in {}", path.display());
    }
    if !text(path, &["status", "--porcelain"])?.is_empty() {
        bail!(
            "{} holds changes of its own: commit or discard them first",
            path.display()
        );
    }
    let prior = text(path, &["rev-parse", "HEAD"])?;
    let reference = format!("refs/heads/{branch}");
    let held = text(main, &["rev-parse", "--verify", "-q", &reference])
        .ok()
        .is_some_and(|tip| contains(main, &tip, &prior));
    if !held {
        bail!(
            "{} is at a commit {branch} does not contain: commit or discard it first",
            path.display()
        );
    }
    Ok(Spot::Detached { prior })
}

/// Whether commit `tip` has `ancestor` in its history.
fn contains(dir: &Path, tip: &str, ancestor: &str) -> bool {
    run(dir, &["merge-base", "--is-ancestor", ancestor, tip], None).is_ok()
}

/// What a worktree holds, as one hash: its commit and everything not
/// committed in it.
pub(super) fn fingerprint(dir: &Path) -> Result<String> {
    let work = snapshot(dir)?;
    let mut hasher = blake3::Hasher::new();
    hasher.update(work.head.as_bytes());
    hasher.update(&work.patch);
    Ok(hasher.finalize().to_hex().to_string())
}

/// Puts the session's worktree on `branch` at `path` in the checkout `main`,
/// at the commit `work.head`, and applies the patch. Leaves nothing changed
/// when anything fails. No stash is ever made: a reused worktree must be
/// clean already.
pub(super) fn attach(
    main: &Path,
    path: &Path,
    branch: &str,
    work: &Work,
    running_here: bool,
) -> Result<Attached> {
    let spot = spot(main, path, branch, running_here)?;
    let head = work.head.as_str();
    if head.is_empty() || !head.bytes().all(|b| b.is_ascii_hexdigit()) {
        bail!("the source sent no commit");
    }
    run(
        main,
        &["cat-file", "-e", &format!("{head}^{{commit}}")],
        None,
    )
    .context("the worktree's commit has not come here yet")?;
    if let Spot::Detached { prior } = &spot
        && !contains(main, head, prior)
    {
        bail!(
            "{} is at a commit the session's branch does not contain",
            path.display()
        );
    }
    let reference = format!("refs/heads/{branch}");
    match text(main, &["rev-parse", "--verify", "-q", &reference]) {
        Ok(tip) if tip == head => {}
        Ok(tip) => {
            // Behind the source: the source's commits are here already
            // (that is what its files waited for).
            if !contains(main, head, &tip) {
                bail!("{branch} here has commits the source's {branch} does not");
            }
            run(main, &["update-ref", &reference, head, &tip], None)?;
        }
        Err(_) => {
            run(main, &["update-ref", &reference, head], None)?;
        }
    }
    let placed = match spot {
        Spot::Free => {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).context("making the worktree's folder")?;
            }
            let made = path.to_string_lossy();
            run(main, &["worktree", "add", "--quiet", &made, branch], None)
                .context("adding the worktree")?;
            Placed::Made
        }
        Spot::Detached { prior } => {
            run(path, &["checkout", "--quiet", branch], None)
                .context("putting the worktree on the branch")?;
            Placed::Reused { prior }
        }
    };
    if !work.patch.is_empty() {
        let applied = run(
            path,
            &["apply", "--binary", "--whitespace=nowarn", "-"],
            Some(&work.patch),
        );
        if let Err(e) = applied {
            // The apply changed nothing, so the folder is as it was made
            // or found: this undo needs no proof.
            match &placed {
                Placed::Made => remove(main, path),
                Placed::Reused { prior } => {
                    if let Err(back) = run(path, &["checkout", "--quiet", "--detach", prior], None)
                    {
                        log::warn!("{} stays on its branch: {back:#}", path.display());
                    }
                }
            }
            return Err(e.context("the uncommitted changes do not apply here"));
        }
    }
    let applied = fingerprint(path).context("noting what the worktree holds")?;
    Ok(Attached { placed, applied })
}

/// What `undo` did.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Undone {
    /// The worktree made for the move is gone.
    Removed,
    /// The reused worktree is detached at its old commit again.
    Reset,
    /// The folder was left as it is, and why.
    Left(String),
}

/// Takes back what `attach` did, only when the folder still holds exactly
/// what it put there (`applied`): anything else is the user's, and stays.
pub(super) fn undo(main: &Path, path: &Path, placed: &Placed, applied: &str) -> Undone {
    match fingerprint(path) {
        Ok(now) if now == applied => {}
        Ok(_) => return Undone::Left(format!("{} holds changes made since", path.display())),
        Err(e) => return Undone::Left(format!("{} was not read: {e:#}", path.display())),
    }
    match placed {
        // Proven to hold only the patch, so forcing removes nothing else.
        Placed::Made => {
            let made = path.to_string_lossy();
            match run(main, &["worktree", "remove", "--force", &made], None) {
                Ok(_) => Undone::Removed,
                Err(e) => Undone::Left(format!("{e:#}")),
            }
        }
        Placed::Reused { prior } => {
            let back = run(
                path,
                &["checkout", "--quiet", "--force", "--detach", prior],
                None,
            )
            .and_then(|_| run(path, &["clean", "-fdq"], None));
            match back {
                Ok(_) => Undone::Reset,
                Err(e) => Undone::Left(format!("{e:#}")),
            }
        }
    }
}

/// Takes away a worktree this machine just made, which holds nothing yet.
fn remove(main: &Path, path: &Path) {
    let made = path.to_string_lossy();
    if let Err(e) = run(main, &["worktree", "remove", "--force", &made], None) {
        log::warn!("the worktree made for a move stays: {e:#}");
    }
}

/// Leaves the branch to the machine the session moved to: the worktree at
/// `dir` stays, at the same commit, on no branch. A ref checked out in a
/// linked worktree is not moved by sync.
pub(super) fn detach(dir: &Path) -> Result<()> {
    run(dir, &["checkout", "--quiet", "--detach"], None).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, path::PathBuf};
    use tempfile::TempDir;

    fn git(dir: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
            .args(args)
            .current_dir(dir)
            .output()
            .expect("git runs");
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_owned()
    }

    /// A checkout with one commit on `main` and a worktree on `feat` with
    /// a changed file, a staged new file, a deleted file and an untracked
    /// one.
    fn source() -> (TempDir, PathBuf) {
        let dir = TempDir::new().expect("a temp folder");
        let main = dir.path().join("main");
        fs::create_dir(&main).expect("a folder");
        git(&main, &["init", "-q", "--initial-branch=main"]);
        // A user's own diff settings must not change what moves.
        git(&main, &["config", "diff.noprefix", "true"]);
        fs::write(main.join("a.txt"), "one\n").expect("a file");
        fs::write(main.join("gone.txt"), "bye\n").expect("a file");
        fs::write(main.join("bin.dat"), [0u8, 1, 2, 3]).expect("a file");
        git(&main, &["add", "."]);
        git(&main, &["commit", "-q", "-m", "first"]);
        let wt = dir.path().join("wt");
        git(
            &main,
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                "feat",
                wt.to_str().expect("utf8"),
            ],
        );
        fs::write(wt.join("a.txt"), "two\n").expect("a change");
        fs::write(wt.join("bin.dat"), [9u8, 8, 7]).expect("a change");
        fs::write(wt.join("staged.txt"), "s\n").expect("a file");
        git(&wt, &["add", "staged.txt"]);
        fs::remove_file(wt.join("gone.txt")).expect("a deletion");
        fs::write(wt.join("new.txt"), "n\n").expect("an untracked file");
        (dir, wt)
    }

    #[test]
    fn uncommitted_work_follows_the_branch_into_a_new_worktree() {
        let (dir, wt) = source();
        let work = snapshot(&wt).expect("a snapshot");
        // The same repository stands in for the target's checkout, which
        // has the commit and, once the source lets go, the branch.
        let main = dir.path().join("main");
        let target = dir.path().join("target-wt");
        git(&wt, &["checkout", "-q", "--detach"]);
        attach(&main, &target, "feat", &work, false).expect("attached");
        assert_eq!(git(&target, &["symbolic-ref", "--short", "HEAD"]), "feat");
        assert_eq!(fs::read_to_string(target.join("a.txt")).unwrap(), "two\n");
        assert_eq!(fs::read(target.join("bin.dat")).unwrap(), [9u8, 8, 7]);
        assert_eq!(
            fs::read_to_string(target.join("staged.txt")).unwrap(),
            "s\n"
        );
        assert_eq!(fs::read_to_string(target.join("new.txt")).unwrap(), "n\n");
        assert!(!target.join("gone.txt").exists());
        // The source's own worktree lost nothing.
        assert_eq!(fs::read_to_string(wt.join("a.txt")).unwrap(), "two\n");
        assert!(wt.join("new.txt").exists());
    }

    #[test]
    fn a_patch_that_does_not_apply_leaves_no_worktree_behind() {
        let (dir, wt) = source();
        let mut work = snapshot(&wt).expect("a snapshot");
        work.patch =
            b"diff --git a/zzz b/zzz\n--- a/zzz\n+++ b/zzz\n@@ -1 +1 @@\n-x\n+y\n".to_vec();
        let main = dir.path().join("main");
        let target = dir.path().join("target-wt");
        git(&wt, &["checkout", "-q", "--detach"]);
        let failed = attach(&main, &target, "feat", &work, false).expect_err("does not apply");
        assert!(format!("{failed:#}").contains("do not apply"), "{failed:#}");
        assert!(!target.exists());
        assert!(!git(&main, &["worktree", "list"]).contains("target-wt"));
    }

    #[test]
    fn detaching_frees_the_branch_and_keeps_the_files() {
        let (dir, wt) = source();
        let main = dir.path().join("main");
        let other = dir.path().join("other");
        let at = other.to_str().expect("utf8");
        let busy = run(&main, &["worktree", "add", "--quiet", at, "feat"], None);
        assert!(busy.is_err(), "the branch is held by the worktree");
        detach(&wt).expect("detached");
        assert!(wt.join("new.txt").exists());
        assert_eq!(fs::read_to_string(wt.join("a.txt")).unwrap(), "two\n");
        run(&main, &["worktree", "add", "--quiet", at, "feat"], None).expect("the branch is free");
    }

    #[test]
    fn a_branch_with_other_commits_here_is_refused() {
        let (dir, wt) = source();
        let main = dir.path().join("main");
        let work = snapshot(&wt).expect("a snapshot");
        git(&wt, &["checkout", "-q", "--detach"]);
        git(&main, &["checkout", "-q", "feat"]);
        fs::write(main.join("side.txt"), "x\n").expect("a file");
        git(&main, &["add", "side.txt"]);
        git(&main, &["commit", "-q", "-m", "side"]);
        git(&main, &["checkout", "-q", "main"]);
        let target = dir.path().join("target-wt");
        let failed = attach(&main, &target, "feat", &work, false).expect_err("diverged");
        assert!(format!("{failed:#}").contains("has commits"), "{failed:#}");
        assert!(!target.exists());
    }

    #[test]
    fn a_branch_checked_out_elsewhere_is_refused_before_its_ref_moves() {
        let (dir, wt) = source();
        let main = dir.path().join("main");
        // The session's worktree is ahead of the branch (a commit on a
        // detached head), and here the branch is checked out in another
        // worktree.
        git(&wt, &["checkout", "-q", "--detach"]);
        git(&wt, &["commit", "-q", "-a", "-m", "ahead"]);
        let work = snapshot(&wt).expect("a snapshot");
        let other = dir.path().join("other");
        git(
            &main,
            &["worktree", "add", "-q", other.to_str().unwrap(), "feat"],
        );
        let tip = git(&main, &["rev-parse", "feat"]);
        assert_ne!(tip, work.head);
        let target = dir.path().join("target-wt");
        let failed = attach(&main, &target, "feat", &work, false).expect_err("checked out");
        assert!(
            format!("{failed:#}").contains("checked out in"),
            "{failed:#}"
        );
        assert_eq!(
            git(&main, &["rev-parse", "feat"]),
            tip,
            "the branch did not move under the other worktree"
        );
        assert!(!target.exists());
    }

    /// Puts the changes the session left behind out of a detached worktree.
    fn clean(wt: &Path) {
        git(wt, &["checkout", "-q", "--force", "--detach"]);
        git(wt, &["clean", "-fdq"]);
    }

    /// A stash entry of the user's own, on the checkout.
    fn foreign_stash(main: &Path) -> String {
        fs::write(main.join("a.txt"), "mine\n").expect("a change");
        git(main, &["stash", "push", "-q", "-m", "the user's own"]);
        git(main, &["stash", "list"])
    }

    #[test]
    fn a_dirty_detached_worktree_is_refused_by_name_and_left_alone() {
        let (dir, wt) = source();
        let main = dir.path().join("main");
        let work = snapshot(&wt).expect("a snapshot");
        detach(&wt).expect("detached");
        let failed = check(&main, &wt, "feat", false).expect_err("dirty");
        assert!(
            format!("{failed:#}").contains(&wt.display().to_string()),
            "{failed:#}"
        );
        assert!(attach(&main, &wt, "feat", &work, false).is_err());
        assert_eq!(fs::read_to_string(wt.join("a.txt")).unwrap(), "two\n");
        assert!(run(&wt, &["symbolic-ref", "-q", "HEAD"], None).is_err());
    }

    #[test]
    fn a_detached_head_with_a_commit_of_its_own_is_refused() {
        let (dir, wt) = source();
        let main = dir.path().join("main");
        let work = snapshot(&wt).expect("a snapshot");
        detach(&wt).expect("detached");
        clean(&wt);
        fs::write(wt.join("own.txt"), "x\n").expect("a file");
        git(&wt, &["add", "own.txt"]);
        git(&wt, &["commit", "-q", "-m", "own"]);
        let head = git(&wt, &["rev-parse", "HEAD"]);
        let failed = attach(&main, &wt, "feat", &work, false).expect_err("not contained");
        assert!(
            format!("{failed:#}").contains("does not contain"),
            "{failed:#}"
        );
        assert_eq!(
            git(&wt, &["rev-parse", "HEAD"]),
            head,
            "the commit is still there"
        );
    }

    #[test]
    fn a_worktree_a_session_runs_in_is_not_reused() {
        let (dir, wt) = source();
        let main = dir.path().join("main");
        detach(&wt).expect("detached");
        clean(&wt);
        let failed = check(&main, &wt, "feat", true).expect_err("a session runs there");
        assert!(
            format!("{failed:#}").contains("a session runs"),
            "{failed:#}"
        );
        check(&main, &wt, "feat", false).expect("free of sessions");
    }

    #[test]
    fn a_clean_detached_worktree_is_reused_and_the_stash_is_never_touched() {
        let (dir, wt) = source();
        let main = dir.path().join("main");
        let work = snapshot(&wt).expect("a snapshot");
        detach(&wt).expect("detached");
        clean(&wt);
        let stash = foreign_stash(&main);
        let got = attach(&main, &wt, "feat", &work, false).expect("reused");
        assert!(matches!(got.placed, Placed::Reused { .. }));
        assert_eq!(git(&wt, &["symbolic-ref", "--short", "HEAD"]), "feat");
        assert_eq!(fs::read_to_string(wt.join("a.txt")).unwrap(), "two\n");
        assert_eq!(git(&main, &["stash", "list"]), stash);
        assert_eq!(undo(&main, &wt, &got.placed, &got.applied), Undone::Reset);
        assert!(run(&wt, &["symbolic-ref", "-q", "HEAD"], None).is_err());
        assert_eq!(git(&wt, &["status", "--porcelain"]), "");
        assert_eq!(git(&main, &["stash", "list"]), stash, "undo left it too");
    }

    #[test]
    fn a_patch_that_does_not_apply_puts_a_reused_worktree_back_as_it_was() {
        let (dir, wt) = source();
        let main = dir.path().join("main");
        let mut work = snapshot(&wt).expect("a snapshot");
        work.patch =
            b"diff --git a/zzz b/zzz\n--- a/zzz\n+++ b/zzz\n@@ -1 +1 @@\n-x\n+y\n".to_vec();
        detach(&wt).expect("detached");
        clean(&wt);
        let at = git(&wt, &["rev-parse", "HEAD"]);
        let failed = attach(&main, &wt, "feat", &work, false).expect_err("does not apply");
        assert!(format!("{failed:#}").contains("do not apply"), "{failed:#}");
        assert!(
            run(&wt, &["symbolic-ref", "-q", "HEAD"], None).is_err(),
            "detached again"
        );
        assert_eq!(git(&wt, &["rev-parse", "HEAD"]), at);
        assert_eq!(git(&wt, &["status", "--porcelain"]), "");
    }

    #[test]
    fn undo_removes_a_worktree_unchanged_since_the_patch_and_leaves_one_with_edits() {
        let (dir, wt) = source();
        let main = dir.path().join("main");
        let work = snapshot(&wt).expect("a snapshot");
        detach(&wt).expect("detached");
        let stash = foreign_stash(&main);
        let target = dir.path().join("target-wt");
        let got = attach(&main, &target, "feat", &work, false).expect("made");
        assert_eq!(got.placed, Placed::Made);
        assert_eq!(
            undo(&main, &target, &got.placed, &got.applied),
            Undone::Removed
        );
        assert!(!target.exists());
        assert_eq!(git(&main, &["stash", "list"]), stash);

        // Made again, and then edited: the folder is the user's now.
        let got = attach(&main, &target, "feat", &work, false).expect("made again");
        fs::write(target.join("a.txt"), "edited since\n").expect("an edit");
        let left = undo(&main, &target, &got.placed, &got.applied);
        assert!(matches!(left, Undone::Left(_)), "{left:?}");
        assert_eq!(
            fs::read_to_string(target.join("a.txt")).unwrap(),
            "edited since\n"
        );
    }

    #[test]
    fn the_patch_is_read_with_a_cap_and_git_is_not_left_running() {
        let (_dir, wt) = source();
        // Far over a tiny cap: stopped, an error, no shorter patch.
        let failed = capped(&wt, &["diff", "HEAD", "--binary"], 3, &[]).expect_err("too large");
        assert!(format!("{failed:#}").contains("too large"), "{failed:#}");
        // A cap that fits gives all of it.
        let whole = capped(&wt, &["diff", "HEAD", "--binary"], MAX_PATCH, &[]).expect("fits");
        assert!(whole.len() > 3);
    }
}
