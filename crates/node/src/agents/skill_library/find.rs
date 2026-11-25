//! Finding libraries already on this machine: skills in the agents' own skills folders that are
//! Git checkouts, or links into one, grouped by the repository's remote. Blocks.

use super::store;
use crate::{agents::skill_disk, machine::provision::installed_by_beep};
use beep_proto::node::skill_library::FoundLibrary;
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fs,
    path::{Path, PathBuf},
};

/// The skills under `home` that come from a Git repository with a remote, by remote URL. Beep's
/// own skills and the folders in `ours` (what libraries installed) are left out, and so are
/// skills that sit in a repository holding the agent's whole folder, such as a dotfiles
/// checkout.
pub(super) fn find(home: &Path, ours: &BTreeSet<PathBuf>) -> Vec<FoundLibrary> {
    // Each repository's top folder, to its remote; none without one.
    let mut remotes: HashMap<PathBuf, Option<String>> = HashMap::new();
    let mut found: BTreeMap<String, (BTreeSet<String>, BTreeSet<String>)> = BTreeMap::new();

    for root in skill_disk::roots(home) {
        let Ok(entries) = fs::read_dir(&root) else {
            continue;
        };
        let label = label(home, &root);
        // The agent's own folder, such as `~/.claude`.
        let agent = root
            .strip_prefix(home)
            .ok()
            .and_then(|rel| rel.components().next())
            .and_then(|first| dunce::canonicalize(home.join(first)).ok());

        for entry in entries.flatten() {
            let path = entry.path();
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };

            if !skill_disk::valid_part(&name) || ours.contains(&path) {
                continue;
            }

            let Ok(text) = fs::read_to_string(path.join("SKILL.md")) else {
                continue;
            };

            if installed_by_beep(&text) {
                continue;
            }

            let Ok(resolved) = dunce::canonicalize(&path) else {
                continue;
            };
            let Some(top) = store::git(&resolved, &["rev-parse", "--show-toplevel"])
                .ok()
                .and_then(|out| dunce::canonicalize(out.trim()).ok())
            else {
                continue;
            };

            if agent.as_ref().is_some_and(|agent| agent.starts_with(&top)) {
                continue;
            }

            let remote = remotes.entry(top.clone()).or_insert_with(|| {
                store::git(&top, &["remote", "get-url", "origin"])
                    .ok()
                    .map(|url| url.trim().to_owned())
                    .filter(|url| !url.is_empty())
            });
            let Some(url) = remote.clone() else {
                continue;
            };
            let (skills, folders) = found.entry(url).or_default();
            skills.insert(name);
            folders.insert(label.clone());
        }
    }

    found
        .into_iter()
        .map(|(url, (skills, folders))| FoundLibrary {
            url,
            skills: skills.into_iter().collect(),
            folders: folders.into_iter().collect(),
        })
        .collect()
}

/// `root` as the user knows it on any system, such as `~/.claude/skills`.
fn label(home: &Path, root: &Path) -> String {
    match root.strip_prefix(home) {
        Ok(rel) => {
            let parts: Vec<String> = rel
                .components()
                .map(|part| part.as_os_str().to_string_lossy().into_owned())
                .collect();
            format!("~/{}", parts.join("/"))
        }
        Err(_) => root.display().to_string(),
    }
}

#[cfg(test)]
#[path = "../../tests/agents_skill_library_find.rs"]
mod tests;
