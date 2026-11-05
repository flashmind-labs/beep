//! A session in a linked worktree moves as its branch and its uncommitted
//! work: the source sends the head commit and a patch of what is not
//! committed, the target makes a worktree on the same branch and applies it.
//! Everything here blocks: call it from `spawn_blocking`.

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(super) enum Placed {
    /// Made for the move.
    Made,
    /// The detached worktree a session left here earlier, put on the branch
    /// again. `stashed`: what it held was set aside in the repository's
    /// stash first.
    Reused { stashed: bool },
}

/// What the folder for the worktree allows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Spot {
    Free,
    /// A detached worktree of this checkout is there.
    Detached,
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
    if let (Some(bytes), Some(mut pipe)) = (stdin, child.stdin.take()) {
        pipe.write_all(bytes).context("sending to git")?;
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

/// What `git args` printed, at most `cap` bytes: git is stopped at the
/// first byte over, so a huge file is never held whole. Exit codes in `ok`
/// besides 0 count as success.
fn capped(dir: &Path, args: &[&str], cap: usize, ok: &[i32]) -> Result<Vec<u8>> {
    let mut child = command(dir, args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .context("running git")?;
    let mut out = Vec::new();
    let reader = child.stdout.take().context("git has no output")?;
    reader
        .take(cap as u64 + 1)
        .read_to_end(&mut out)
        .context("reading git's output")?;
    if out.len() > cap {
        // Stopping git closes its output; it is not waited on for more.
        let _ = child.kill();
        let _ = child.wait();
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
/// a detached worktree of this checkout (one a session left). Done before
/// the source ends anything.
pub(super) fn check(main: &Path, path: &Path, branch: &str) -> Result<()> {
    spot(main, path, branch).map(|_| ())
}

fn spot(main: &Path, path: &Path, branch: &str) -> Result<Spot> {
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
    if detached {
        Ok(Spot::Detached)
    } else {
        bail!("{} is taken on this machine", path.display())
    }
}

/// Puts the session's worktree on `branch` at `path` in the checkout `main`,
/// at the commit `work.head`, and applies the patch. Leaves nothing changed
/// when anything fails.
pub(super) fn attach(main: &Path, path: &Path, branch: &str, work: &Work) -> Result<Placed> {
    let spot = spot(main, path, branch)?;
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
    let reference = format!("refs/heads/{branch}");
    match text(main, &["rev-parse", "--verify", "-q", &reference]) {
        Ok(tip) if tip == head => {}
        Ok(tip) => {
            // Behind the source: the source's commits are here already
            // (that is what its files waited for), and its branch cannot
            // reach this machine while its worktree holds it.
            if run(main, &["merge-base", "--is-ancestor", &tip, head], None).is_err() {
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
        Spot::Detached => {
            let dirty = !text(path, &["status", "--porcelain"])?.is_empty();
            if dirty {
                // What the session left behind is kept, not lost.
                run(
                    path,
                    &[
                        "stash",
                        "push",
                        "--include-untracked",
                        "--quiet",
                        "-m",
                        "left behind when the session moved back",
                    ],
                    None,
                )
                .context("setting aside what the old worktree holds")?;
            }
            let on = run(path, &["checkout", "--quiet", branch], None);
            if let Err(e) = on {
                undo(main, path, Placed::Reused { stashed: dirty });
                return Err(e.context("putting the worktree on the branch"));
            }
            Placed::Reused { stashed: dirty }
        }
    };
    if work.patch.is_empty() {
        return Ok(placed);
    }
    let applied = run(
        path,
        &["apply", "--binary", "--whitespace=nowarn", "-"],
        Some(&work.patch),
    );
    if let Err(e) = applied {
        undo(main, path, placed);
        return Err(e.context("the uncommitted changes do not apply here"));
    }
    Ok(placed)
}

/// Takes the worktree back to how it was before `attach`: one made for the
/// move goes, one reused is detached again with what it held put back.
pub(super) fn undo(main: &Path, path: &Path, placed: Placed) {
    match placed {
        Placed::Made => remove(main, path),
        Placed::Reused { stashed } => {
            if let Err(e) = detach(path) {
                log::warn!("the worktree of a move stays on its branch: {e:#}");
                return;
            }
            if stashed && let Err(e) = run(path, &["stash", "pop", "--quiet"], None) {
                log::warn!("what the worktree held stays in the stash: {e:#}");
            }
        }
    }
}

/// Takes away a worktree this machine made for a move that did not finish.
pub(super) fn remove(main: &Path, path: &Path) {
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
        attach(&main, &target, "feat", &work).expect("attached");
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
        let failed = attach(&main, &target, "feat", &work).expect_err("does not apply");
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
        let failed = attach(&main, &target, "feat", &work).expect_err("diverged");
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
        let failed = attach(&main, &target, "feat", &work).expect_err("checked out");
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

    #[test]
    fn a_detached_worktree_a_session_left_is_reused_and_what_it_held_is_kept() {
        let (dir, wt) = source();
        let main = dir.path().join("main");
        let work = snapshot(&wt).expect("a snapshot");
        // The session moved away: its worktree is detached and still holds
        // the changes. It moves back to the same folder.
        detach(&wt).expect("detached");
        check(&main, &wt, "feat").expect("the folder can be reused");
        let placed = attach(&main, &wt, "feat", &work).expect("reused");
        assert_eq!(placed, Placed::Reused { stashed: true });
        assert_eq!(git(&wt, &["symbolic-ref", "--short", "HEAD"]), "feat");
        assert_eq!(fs::read_to_string(wt.join("a.txt")).unwrap(), "two\n");
        assert!(git(&main, &["stash", "list"]).contains("left behind"));
    }

    #[test]
    fn a_patch_that_does_not_apply_puts_a_reused_worktree_back_as_it_was() {
        let (dir, wt) = source();
        let main = dir.path().join("main");
        let mut work = snapshot(&wt).expect("a snapshot");
        work.patch =
            b"diff --git a/zzz b/zzz\n--- a/zzz\n+++ b/zzz\n@@ -1 +1 @@\n-x\n+y\n".to_vec();
        detach(&wt).expect("detached");
        let failed = attach(&main, &wt, "feat", &work).expect_err("does not apply");
        assert!(format!("{failed:#}").contains("do not apply"), "{failed:#}");
        assert!(
            run(&wt, &["symbolic-ref", "-q", "HEAD"], None).is_err(),
            "detached again"
        );
        assert_eq!(fs::read_to_string(wt.join("a.txt")).unwrap(), "two\n");
        assert!(wt.join("new.txt").exists(), "what it held is back");
    }
}
