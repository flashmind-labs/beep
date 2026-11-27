//! The checkout state and branch reads, answered in process with gitoxide.
//! They print the text of `checkout_sync::local_state_script` and
//! `branches::branches_script`, which the desktop parses, without starting a
//! shell and a git process per branch on every ref change.

use anyhow::{Context, Result};
use beep_projects::branches::MAX_REFS;
use gix::{ObjectId, Repository, refs::FullName, remote::Direction};
use std::{
    collections::{HashMap, HashSet},
    fmt::Write,
    path::Path,
};

/// Bytes of decoded objects kept while one read walks the history.
const OBJECT_CACHE: usize = 4 << 20;

/// What `local_state_script` prints for the checkout at `root`.
pub(super) fn state(root: &Path) -> Result<String> {
    let mut repo = crate::agents::found::repository(root).context("no Git repository here")?;
    repo.object_cache_size_if_unset(OBJECT_CACHE);
    let Some(branch) = repo.head_name()? else {
        return Ok("@@detached\n".to_owned());
    };
    let mut out = format!("@@branch {}\n", branch.shorten());

    if let Some(upstream) = upstream(&repo, &branch)? {
        let (ahead, behind) = match repo.head_id() {
            Ok(head) => distance(&repo, head.detach(), upstream.1)?,
            Err(_) => return Ok(format!("{out}@@upstream {}\n@@ab \n", upstream.0)),
        };
        writeln!(out, "@@upstream {}", upstream.0)?;
        writeln!(out, "@@ab {ahead}\t{behind}")?;
    }

    Ok(out)
}

/// What `branches_script` prints for the checkout at `root`, with
/// `fallback` as the default branch when `origin` names none.
pub(super) fn branches(root: &Path, fallback: &str) -> Result<String> {
    let mut repo = crate::agents::found::repository(root).context("no Git repository here")?;
    // The distance walks read the same commits again for every branch.
    repo.object_cache_size_if_unset(OBJECT_CACHE);
    let default =
        crate::agents::found::default_branch(&repo).unwrap_or_else(|| fallback.to_owned());
    let base = [
        format!("refs/remotes/origin/{default}"),
        format!("refs/heads/{default}"),
    ]
    .iter()
    .find_map(|name| commit_of(&repo, name));
    let mut out = format!(
        "@@default {default}\n@@base {}\n",
        base.map(|id| id.to_string()).unwrap_or_default()
    );

    let worktrees = checked_out(&repo)?;
    let mut refs = Vec::new();

    for prefix in ["refs/heads/", "refs/remotes/origin/"] {
        for reference in repo.references()?.prefixed(prefix)? {
            let Ok(reference) = reference else {
                continue;
            };
            // A symbolic ref such as `origin/HEAD` names a branch listed itself.
            let Some(id) = reference.target().try_id().map(ToOwned::to_owned) else {
                continue;
            };
            let Ok(commit) = repo.find_commit(id) else {
                continue;
            };
            let at = commit.time().map(|time| time.seconds).unwrap_or(0);
            let subject = commit
                .message()
                .map(|message| message.summary().to_string())
                .unwrap_or_default();
            refs.push((at, reference.name().as_bstr().to_string(), id, subject));
        }
    }

    // Newest first, as `for-each-ref --sort=-committerdate`, ties by name.
    refs.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    refs.truncate(MAX_REFS as usize);

    for (at, name, id, subject) in &refs {
        let worktree = worktrees.get(name).map(String::as_str).unwrap_or_default();
        writeln!(out, "@@ref {name}\t{id}\t{at}\t{worktree}\t{subject}")?;
    }

    if let Some(base) = base {
        let mut seen = HashSet::new();

        for (_, _, id, _) in &refs {
            if !seen.insert(*id) {
                continue;
            }

            let (ahead, behind) = distance(&repo, *id, base)?;
            writeln!(out, "@@ab {id} {behind} {ahead}")?;
        }
    }

    Ok(out)
}

/// The upstream `branch` tracks, shortened as `rev-parse --abbrev-ref`
/// prints it, and its commit; `None` when it has none or it is not fetched.
fn upstream(repo: &Repository, branch: &FullName) -> Result<Option<(String, ObjectId)>> {
    let Some(tracking) = repo.branch_remote_tracking_ref_name(branch.as_ref(), Direction::Fetch)
    else {
        return Ok(None);
    };
    let Ok(tracking) = tracking else {
        return Ok(None);
    };
    let Some(id) = commit_of(repo, &tracking.as_bstr().to_string()) else {
        return Ok(None);
    };

    Ok(Some((tracking.shorten().to_string(), id)))
}

/// The commit ref `name` leads to, as `rev-parse --verify '<name>^{commit}'`.
fn commit_of(repo: &Repository, name: &str) -> Option<ObjectId> {
    Some(repo.find_reference(name).ok()?.peel_to_commit().ok()?.id)
}

/// Commits only `tip` has and commits only `other` has, as
/// `rev-list --left-right --count tip...other` counts them.
fn distance(repo: &Repository, tip: ObjectId, other: ObjectId) -> Result<(usize, usize)> {
    let only = |from: ObjectId, hidden: ObjectId| -> Result<usize> {
        let walk = repo.rev_walk([from]).with_hidden([hidden]).all()?;
        Ok(walk.count())
    };

    Ok((only(tip, other)?, only(other, tip)?))
}

/// The worktree each local branch is checked out in, by its full ref name,
/// as `for-each-ref`'s `%(worktreepath)`: the main checkout and every linked one.
fn checked_out(repo: &Repository) -> Result<HashMap<String, String>> {
    let mut found = HashMap::new();
    let main = gix::open(repo.common_dir()).context("opening the main checkout")?;

    if let (Some(folder), Ok(Some(branch))) = (main.workdir(), main.head_name()) {
        found.insert(branch.as_bstr().to_string(), folder.display().to_string());
    }

    for proxy in repo.worktrees()? {
        let Ok(folder) = proxy.base() else {
            continue;
        };
        let Ok(linked) = proxy.into_repo_with_possibly_inaccessible_worktree() else {
            continue;
        };
        let Ok(Some(branch)) = linked.head_name() else {
            continue;
        };
        found.insert(branch.as_bstr().to_string(), folder.display().to_string());
    }

    Ok(found)
}
