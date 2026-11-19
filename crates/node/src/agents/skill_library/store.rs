//! The node's copy of each library, under `skill-libraries` in its folder: a clone per Git
//! repository, checked out at the library's revision, on the node that read the repository. Every
//! other node keeps a plain copy per revision of the skills it installs, taken from a machine that
//! has them. Also listing a repository's skills. Every function here blocks.

use crate::agents::{custom_agents::parse::skill_description, skills::disk};
use anyhow::{Context, Result, bail};
use beep_proto::node::{mesh_state::LibrarySkill, skills::SkillFile};
use serde_bytes::ByteBuf;
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
    node_dir.join(ROOT).join(folder(url))
}

/// The clone's folder name for the repository at `url`, which its plain copies start with.
fn folder(url: &str) -> String {
    let digest = hex::encode(Sha256::digest(url.as_bytes()));
    digest[..16].to_owned()
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

/// Whether this node clones the repository at `url`: it read it when the library was added.
pub(super) fn cloned(node_dir: &Path, url: &str) -> bool {
    git_root(node_dir, url).join(".git").is_dir()
}

/// The plain copy of the library at `url` and `revision`: a folder per skill.
pub(super) fn copy_root(node_dir: &Path, url: &str, revision: &str) -> Result<PathBuf> {
    let commit = (7..=64).contains(&revision.len()) && revision.chars().all(|c| c.is_ascii_hexdigit());

    if !commit {
        bail!("{revision:?} is not a commit");
    }

    Ok(node_dir.join(ROOT).join(format!("{}-{revision}", folder(url))))
}

/// The files of the library's skill `skill` at `revision` on this node: from the plain copy, or
/// from the clone's objects, so a clone not yet checked out at `revision` serves it too. `None`
/// when this node has neither, or the skill is too big to send.
pub(super) fn skill_at(
    node_dir: &Path,
    url: &str,
    revision: &str,
    skill: &str,
) -> Result<Option<Vec<SkillFile>>> {
    if !disk::valid_part(skill) {
        return Ok(None);
    }

    let copied = copy_root(node_dir, url, revision)?.join(skill);

    if copied.join(SKILL_MD).is_file() {
        return disk::read(&copied);
    }

    let root = git_root(node_dir, url);

    if !root.join(".git").is_dir()
        || git(&root, &["cat-file", "-e", &format!("{revision}^{{commit}}")]).is_err()
    {
        return Ok(None);
    }

    let listed = git(&root, &["ls-tree", "-r", "-z", "--name-only", revision])?;
    let paths: Vec<String> = listed.split('\0').map(str::to_owned).collect();
    let Some(rel) = skill_dirs(paths.iter().cloned()).remove(skill) else {
        return Ok(None);
    };
    let prefix = format!("{rel}/");
    let mut files = Vec::new();
    let mut bytes = 0;

    for path in &paths {
        let Some(inner) = path.strip_prefix(&prefix) else {
            continue;
        };

        if !inner.split('/').all(disk::valid_part) {
            continue;
        }

        let data = git_bytes(&root, &["cat-file", "blob", &format!("{revision}:{path}")])?;
        bytes += data.len() as u64;

        if files.len() >= disk::MAX_FILES || bytes > disk::MAX_BYTES {
            return Ok(None);
        }

        files.push(SkillFile {
            path: inner.to_owned(),
            data: ByteBuf::from(data),
        });
    }

    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(Some(files))
}

/// Removes the plain copies of the library at `url` from revisions other than `revision`.
pub(super) fn prune_copies(node_dir: &Path, url: &str, revision: &str) {
    let prefix = format!("{}-", folder(url));
    let Ok(entries) = fs::read_dir(node_dir.join(ROOT)) else {
        return;
    };

    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(rest) = name.to_str().and_then(|name| name.strip_prefix(&prefix)) else {
            continue;
        };

        if rest != revision
            && let Err(e) = fs::remove_dir_all(entry.path())
        {
            log::debug!("skill libraries: {} not removed: {e}", entry.path().display());
        }
    }
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
    git_bytes(dir, args).map(|out| String::from_utf8_lossy(&out).into_owned())
}

/// The bytes `git <args>` printed in `dir`.
fn git_bytes(dir: &Path, args: &[&str]) -> Result<Vec<u8>> {
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

    Ok(out.stdout)
}

/// The skills among `paths` (files under a library's root, `/` between parts): each folder with
/// a `SKILL.md`, by folder name, to its path from the root. Hidden folders other than an agent's
/// skills folder at the root (such as `.claude/skills`), skills inside other skills and names
/// taken by a skill nearer the root are left out.
pub(super) fn skill_dirs(paths: impl IntoIterator<Item = String>) -> BTreeMap<String, String> {
    let mut dirs: Vec<String> = paths
        .into_iter()
        .filter_map(|path| path.strip_suffix(&format!("/{SKILL_MD}")).map(str::to_owned))
        .filter(|dir| {
            let parts: Vec<&str> = dir.split('/').collect();
            let plain = match parts.as_slice() {
                [agent, "skills", rest @ ..]
                    if agent.strip_prefix('.').is_some_and(disk::valid_part) =>
                {
                    rest
                }
                all => all,
            };

            parts.len() <= MAX_DEPTH && !plain.is_empty() && plain.iter().all(|part| disk::valid_part(part))
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
            } else if depth < MAX_DEPTH && path.is_dir() {
                if disk::valid_part(&name) {
                    stack.push((path, rel, depth + 1));
                } else if depth == 0 && path.join("skills").is_dir() {
                    // An agent's skills folder, such as `.claude/skills`.
                    stack.push((path.join("skills"), format!("{rel}/skills"), 2));
                }
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
