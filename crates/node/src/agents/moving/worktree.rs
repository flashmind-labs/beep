//! A session in a linked worktree moves as its branch and its uncommitted
//! work: the source sends the head commit and a patch of what is not
//! committed, the target makes a worktree on the same branch and applies it.
//! Everything here blocks: call it from `spawn_blocking`.

use anyhow::{Context, Result, bail};
use std::{
    io::Write,
    path::Path,
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

fn run(dir: &Path, args: &[&str], stdin: Option<&[u8]>) -> Result<Vec<u8>> {
    let mut child = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_TERMINAL_PROMPT", "0")
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
    let mut patch = run(dir, &["diff", "HEAD", "--binary", "--no-color"], None)
        .context("reading the uncommitted changes")?;
    let others = run(
        dir,
        &["ls-files", "--others", "--exclude-standard", "-z"],
        None,
    )
    .context("listing new files")?;
    for name in others.split(|byte| *byte == 0).filter(|n| !n.is_empty()) {
        let name = String::from_utf8_lossy(name).into_owned();
        // `--no-index` exits 1 when the files differ, which is what a new
        // file does against nothing.
        let out = Command::new("git")
            .args(["diff", "--no-index", "--binary", "--no-color", "--"])
            .arg(if cfg!(windows) { "NUL" } else { "/dev/null" })
            .arg(&name)
            .current_dir(dir)
            .stdin(Stdio::null())
            .output()
            .context("reading a new file")?;
        if !matches!(out.status.code(), Some(0 | 1)) {
            bail!(
                "git could not read {name}: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        patch.extend_from_slice(&out.stdout);
        if patch.len() > MAX_PATCH {
            bail!("the uncommitted changes are too large to move");
        }
    }
    if patch.len() > MAX_PATCH {
        bail!("the uncommitted changes are too large to move");
    }
    Ok(Work { head, patch })
}

/// Makes the worktree `path` of the checkout `main` on `branch`, at the
/// commit `work.head`, and applies the patch. Leaves nothing behind when
/// anything fails.
pub(super) fn attach(main: &Path, path: &Path, branch: &str, work: &Work) -> Result<()> {
    if branch.starts_with('-') {
        bail!("{branch} is not a branch name");
    }
    text(main, &["check-ref-format", "--branch", branch])
        .with_context(|| format!("{branch} is not a branch name"))?;
    if path.exists() {
        bail!("{} is taken on this machine", path.display());
    }
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
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).context("making the worktree's folder")?;
    }
    let made = path.to_string_lossy();
    run(main, &["worktree", "add", "--quiet", &made, branch], None)
        .context("adding the worktree")?;
    if work.patch.is_empty() {
        return Ok(());
    }
    let applied = run(
        path,
        &["apply", "--binary", "--whitespace=nowarn", "-"],
        Some(&work.patch),
    );
    if let Err(e) = applied {
        remove(main, path);
        return Err(e.context("the uncommitted changes do not apply here"));
    }
    Ok(())
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
}
