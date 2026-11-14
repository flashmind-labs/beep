//! The node's copy of each library, under `skill-libraries` in its folder: a clone per Git
//! repository, checked out at the library's revision. Also listing a repository's skills.
//! Every function here blocks.

use crate::agents::{custom_agents::parse::skill_description, skills::disk};
use anyhow::{Context, Result, bail};
use beep_proto::node::mesh_state::LibrarySkill;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::Stdio,
};

/// The folder in the node's folder that holds the libraries.
const ROOT: &str = "skill-libraries";
/// The ref a fetch of the remote's default branch lands on.
const HEAD_REF: &str = "refs/beep/head";
/// How deep under a library's root a skill may sit.
const MAX_DEPTH: usize = 6;
/// The file that makes a folder a skill.
const SKILL_MD: &str = "SKILL.md";

/// The clone of the repository at `url`, one per repository, so inspecting it and installing
/// from it share one download.
pub(super) fn git_root(node_dir: &Path, url: &str) -> PathBuf {
    let digest = hex::encode(Sha256::digest(url.as_bytes()));
    node_dir.join(ROOT).join(&digest[..16])
}

/// Fetches the default branch of the repository at `url` and lists the skills at its newest
/// commit: that commit and the skills.
pub(super) fn inspect_git(node_dir: &Path, url: &str) -> Result<(String, Vec<LibrarySkill>)> {
    let root = git_root(node_dir, url);
    clone(&root, url)?;
    git(&root, &["fetch", "--quiet", "origin", &format!("+HEAD:{HEAD_REF}")])?;
    let commit = git(&root, &["rev-parse", &format!("{HEAD_REF}^{{commit}}")])?
        .trim()
        .to_owned();
    let listed = git(&root, &["ls-tree", "-r", "-z", "--name-only", &commit])?;
    let mut skills = Vec::new();

    for (name, rel) in skill_dirs(listed.split('\0').map(str::to_owned)) {
        let text = git(&root, &["show", &format!("{commit}:{rel}/{SKILL_MD}")])?;
        skills.push(LibrarySkill {
            name,
            description: skill_description(&text),
        });
    }

    Ok((commit, skills))
}

/// Puts the clone of `url` at `revision`: cloned when missing, fetched when it lacks the commit.
pub(super) fn ensure_git(node_dir: &Path, url: &str, revision: &str) -> Result<PathBuf> {
    let root = git_root(node_dir, url);
    clone(&root, url)?;
    let commit = format!("{revision}^{{commit}}");

    if git(&root, &["cat-file", "-e", &commit]).is_err() {
        git(&root, &["fetch", "--quiet", "origin"])?;
    }
    // A commit on no branch, such as one a branch moved past and force-pushed away.
    if git(&root, &["cat-file", "-e", &commit]).is_err() {
        git(&root, &["fetch", "--quiet", "origin", revision])?;
    }

    let head = git(&root, &["rev-parse", "--verify", "--quiet", "HEAD"]).unwrap_or_default();
    // A fresh clone is on a branch with nothing checked out; the store is only ever detached.
    let detached = git(&root, &["symbolic-ref", "--quiet", "HEAD"]).is_err();

    if head.trim() != revision || !detached {
        git(&root, &["checkout", "--quiet", "--detach", "--force", revision])?;
    }

    Ok(root)
}

/// Clones `url` into `root` unless it is there. The clone is made beside `root` and renamed in,
/// so two clones at once leave one whole.
fn clone(root: &Path, url: &str) -> Result<()> {
    if root.join(".git").is_dir() {
        return Ok(());
    }

    let parent = root.parent().context("a library folder has a parent")?;
    fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    let temp = tempfile::Builder::new()
        .prefix(".clone")
        .tempdir_in(parent)
        .with_context(|| format!("creating a folder in {}", parent.display()))?;
    let into = temp.path().join("repo");
    let into_text = into.to_str().context("the library folder is not text")?;
    git(parent, &["clone", "--quiet", "--no-checkout", url, into_text])?;

    if let Err(e) = fs::rename(&into, root)
        && !root.join(".git").is_dir()
    {
        return Err(e).with_context(|| format!("writing {}", root.display()));
    }

    Ok(())
}

/// What `git <args>` printed in `dir`; its error output when it failed.
pub(super) fn git(dir: &Path, args: &[&str]) -> Result<String> {
    let out = crate::run::process::background_command("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .output();
    crate::run::process::finished("git", &out);
    let out = out.context("running git")?;

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

/// The skills among `paths` (files under a library's root, `/` between parts): each folder with
/// a `SKILL.md`, by folder name, to its path from the root. Hidden folders, skills inside other
/// skills and names taken by a skill nearer the root are left out.
pub(super) fn skill_dirs(paths: impl IntoIterator<Item = String>) -> BTreeMap<String, String> {
    let mut dirs: Vec<String> = paths
        .into_iter()
        .filter_map(|path| path.strip_suffix(&format!("/{SKILL_MD}")).map(str::to_owned))
        .filter(|dir| {
            let parts: Vec<&str> = dir.split('/').collect();
            parts.len() <= MAX_DEPTH && parts.iter().all(|part| disk::valid_part(part))
        })
        .collect();
    dirs.sort_by(|a, b| (a.matches('/').count(), a).cmp(&(b.matches('/').count(), b)));
    let mut found: BTreeMap<String, String> = BTreeMap::new();

    for dir in dirs {
        let inside = found
            .values()
            .any(|skill| dir.starts_with(&format!("{skill}/")));
        let name = dir.rsplit('/').next().unwrap_or_default().to_owned();

        if !inside && !found.contains_key(&name) {
            found.insert(name, dir);
        }
    }

    found
}

/// The paths of the `SKILL.md` files under `root`, `/` between parts, following links as the
/// agents do, no deeper than a skill may sit.
fn skill_files(root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![(root.to_path_buf(), String::new(), 0)];

    while let Some((dir, prefix, depth)) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };

        for entry in entries.flatten() {
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            let rel = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{prefix}/{name}")
            };
            let path = entry.path();

            if name == SKILL_MD && path.is_file() {
                out.push(rel);
            } else if disk::valid_part(&name) && depth < MAX_DEPTH && path.is_dir() {
                stack.push((path, rel, depth + 1));
            }
        }
    }

    out
}

/// The skills in the library copy at `root`, by name, to their folders.
pub(super) fn skills_in(root: &Path) -> BTreeMap<String, PathBuf> {
    skill_dirs(skill_files(root))
        .into_iter()
        .map(|(name, rel)| {
            let dir = rel.split('/').fold(root.to_path_buf(), |p, part| p.join(part));
            (name, dir)
        })
        .collect()
}

#[cfg(test)]
#[path = "../../tests/agents_skill_library_store.rs"]
pub(super) mod tests;
