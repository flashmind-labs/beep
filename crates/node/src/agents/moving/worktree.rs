//! A session in a linked worktree moves as its branch and its uncommitted
//! work: the source sends the head commit and a patch of what is not
//! committed, the target makes a worktree on the same branch and applies it.
//! Everything here blocks: call it from `spawn_blocking`.

use crate::files::paths;
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
};

/// The largest patch of uncommitted work that moves.
pub(super) const MAX_PATCH: usize = 64 * 1024 * 1024;

/// The largest commit history sent for a move.
pub(super) const MAX_BUNDLE: usize = 256 * 1024 * 1024;

/// A bundle of the worktree's history, without any machine's Git metadata.
pub(super) fn bundle(dir: &Path, head: &str) -> Result<Vec<u8>> {
    if text(dir, &["rev-parse", "HEAD"])? != head {
        bail!("the worktree's commit changed during the move");
    }
    let bytes = capped(dir, &["bundle", "create", "-", "HEAD"], MAX_BUNDLE, &[])?;
    if text(dir, &["rev-parse", "HEAD"])? != head {
        bail!("the worktree's commit changed during the move");
    }
    Ok(bytes)
}

pub(super) fn has_commit(main: &Path, head: &str) -> bool {
    !head.is_empty()
        && head.bytes().all(|b| b.is_ascii_hexdigit())
        && run(
            main,
            &["cat-file", "-e", &format!("{head}^{{commit}}")],
            None,
        )
        .is_ok()
}

/// Imports objects only. Local branches and worktree registrations stay local.
pub(super) fn import(main: &Path, head: &str, bytes: &[u8]) -> Result<()> {
    let common = text(
        main,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )?;
    let mut file = tempfile::NamedTempFile::new_in(common)?;
    file.write_all(bytes)?;
    let path = file.path().to_string_lossy();
    run(main, &["bundle", "unbundle", &path], None).context("importing the worktree's commits")?;
    if !has_commit(main, head) {
        bail!("the source did not send the worktree's commit");
    }
    Ok(())
}

/// What a worktree holds beyond its branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Work {
    /// The commit checked out.
    pub(crate) head: String,
    /// Tracked changes and new files, as `git apply` takes them.
    pub(crate) patch: Vec<u8>,
}

impl Work {
    /// What the worktree held, as one hash: its commit and everything not
    /// committed in it.
    pub(super) fn fingerprint(&self) -> String {
        let mut hasher = blake3::Hasher::new();
        hasher.update(self.head.as_bytes());
        hasher.update(&self.patch);
        hasher.finalize().to_hex().to_string()
    }
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
        if name.ends_with('/') {
            bail!("{name} is a repository of its own, which cannot move in a patch");
        }
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
        // A file git could not read gives exit 1 and nothing: not an empty
        // patch, a file that would be missing over there.
        if out.is_empty() {
            bail!("{name} could not be read, so it cannot move");
        }
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
    match (paths::canonicalize(a), paths::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

/// Whether a worktree of the checkout `main` can be on `branch` at `path`:
/// the branch is checked out nowhere here, and the folder is free or holds
/// a clean detached worktree of this checkout (one a session left) at a
/// commit the branch contains, with no session running in it
/// (`running_here`). Done before the source ends anything.
pub(super) fn check(main: &Path, path: &Path, branch: &str, running_here: bool) -> Result<Placed> {
    Ok(match spot(main, path, branch, running_here)? {
        Spot::Free => Placed::Made,
        Spot::Detached { prior } => Placed::Reused { prior },
    })
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
        // A folder that was deleted but is still registered would make
        // `worktree add` fail. Nothing in git's admin folders is touched
        // here (a prune would drop every worktree whose folder is away at
        // this moment): the user is told how to clear the one entry.
        if worktrees.iter().any(|(there, _)| there == path) {
            bail!(
                "{} is missing but still registered as a worktree: run `git worktree prune` in {}",
                path.display(),
                main.display()
            );
        }
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
    Ok(snapshot(dir)?.fingerprint())
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

/// Takes back what `attach` did, only when the folder is proven to hold
/// nothing but what it put there: exactly `applied` (a fingerprint), or,
/// when `applied` is empty because `attach` did not finish, nothing at all.
/// Anything else is the user's, and stays.
pub(super) fn undo(main: &Path, path: &Path, placed: &Placed, applied: &str) -> Undone {
    if !path.exists() {
        // Nothing was made, or it is gone.
        return Undone::Removed;
    }
    if applied.is_empty() {
        match text(path, &["status", "--porcelain"]) {
            Ok(status) if status.is_empty() => {}
            Ok(_) => {
                return Undone::Left(format!(
                    "{} holds changes and no record of what was put there",
                    path.display()
                ));
            }
            Err(e) => return Undone::Left(format!("{} was not read: {e:#}", path.display())),
        }
    } else {
        match fingerprint(path) {
            Ok(now) if now == applied => {}
            Ok(_) => return Undone::Left(format!("{} holds changes made since", path.display())),
            Err(e) => return Undone::Left(format!("{} was not read: {e:#}", path.display())),
        }
    }
    // Files git ignores (build output, `.env`) are not in the fingerprint:
    // a folder this move made that holds any stays. A reused folder had its
    // own before the move, and `clean` keeps them.
    if *placed == Placed::Made {
        match run(
            path,
            &[
                "ls-files",
                "--others",
                "--ignored",
                "--exclude-standard",
                "-z",
            ],
            None,
        ) {
            Ok(ignored) if ignored.is_empty() => {}
            Ok(_) => {
                return Undone::Left(format!("{} holds files git ignores", path.display()));
            }
            Err(e) => return Undone::Left(format!("{e:#}")),
        }
    }
    match placed {
        Placed::Made => {
            // Proven to hold only the patch: take it out, then the plain
            // remove, which refuses anything left.
            let made = path.to_string_lossy();
            let gone = run(path, &["reset", "--hard", "--quiet"], None)
                .and_then(|_| run(path, &["clean", "-fdq"], None))
                .and_then(|_| run(main, &["worktree", "remove", &made], None));
            match gone {
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

/// Once the session runs on the target: detaches `dir` like `detach`, and,
/// if its fingerprint is exactly what was sent (`sent`), takes the sent
/// changes out of it too, so a session that comes back finds a clean
/// folder. A folder that holds anything else is left as it is.
pub(super) fn detach_clean(dir: &Path, sent: Option<&str>) -> Result<bool> {
    detach(dir)?;
    let Some(sent) = sent else {
        return Ok(false);
    };
    if fingerprint(dir).ok().as_deref() != Some(sent) {
        return Ok(false);
    }
    run(dir, &["checkout", "--quiet", "--force", "--detach"], None)?;
    run(dir, &["clean", "-fdq"], None)?;
    Ok(true)
}

#[cfg(test)]
#[path = "../../tests/agents_moving_worktree.rs"]
mod tests;
