//! Beep's agent skills, written by `install` into Beep's own skills folder,
//! `beep_dir` (`~/.config/beep/skills`), never into an agent's. `brief` lists
//! that folder's skills for the sessions Beep starts, and tells them to use
//! the `report` skill for their final response when it is installed.

use crate::custom_agents::parse;
use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    env, fs, io,
    path::{Path, PathBuf},
};

/// Carries the SHA-256 of the rest of the file. Beep replaces a file only
/// while it matches: a file without it is the user's own, and one that no
/// longer matches was edited, so both stay as they are.
const MARKER: &str =
    "<!-- Installed by Beep. Beep keeps this file current until you edit it. sha256:";
const MARKER_END: &str = " -->";

/// Whether `text` is a skill file Beep installed, edited since or not.
pub(crate) fn installed_by_beep(text: &str) -> bool {
    text.contains(MARKER)
}

/// A skill as Beep ships it.
pub(super) struct Skill {
    pub(super) name: &'static str,
    pub(super) text: &'static str,
}

pub(super) const SKILLS: &[Skill] = &[
    Skill {
        name: "github",
        text: include_str!("../../../skills/github/SKILL.md"),
    },
    Skill {
        name: "forgejo",
        text: include_str!("../../../skills/forgejo/SKILL.md"),
    },
    Skill {
        name: "beep_join_team",
        text: include_str!("../../../skills/beep_join_team/SKILL.md"),
    },
    Skill {
        name: REPORT,
        text: include_str!("../../../skills/report/SKILL.md"),
    },
];

/// The skill sessions use to generate their final response.
const REPORT: &str = "report";

const REPORT_RULE: &str = "Use the `report` skill to generate your final response \
    when you finish a task or stop before it is done.";

pub fn beep_dir() -> Option<PathBuf> {
    let home = dirs::home_dir();
    crate::config_home(&|name| env::var_os(name), home.as_deref())
        .map(|dir| dir.join("beep").join("skills"))
}

pub(crate) fn brief(dir: &Path) -> Option<String> {
    let mut has_report = false;
    let mut lines: Vec<String> = fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter_map(|entry| {
            let path = entry.path().join("SKILL.md");
            let text = fs::read_to_string(&path).ok()?;
            let found = parse::markdown(&text);
            let name = found
                .name
                .unwrap_or_else(|| entry.file_name().to_string_lossy().into_owned());

            if name == REPORT {
                has_report = true;
            }

            Some(format!(
                "- {name}: {} (`{}`)",
                found.description,
                path.display()
            ))
        })
        .collect();

    if lines.is_empty() {
        return None;
    }

    lines.sort();
    let listed = format!(
        "Beep keeps skills of its own in `{}`, outside your own skills list. \
         When a task fits one, read its SKILL.md and follow it:\n{}",
        dir.display(),
        lines.join("\n")
    );

    Some(if has_report {
        format!("## Beep skills\n\n{REPORT_RULE}\n\n{listed}")
    } else {
        format!("## Beep skills\n\n{listed}")
    })
}

/// The skill files the node has written, one path per line in a file of
/// the node's own, so one the user deleted stays deleted.
pub(super) struct Installed {
    file: PathBuf,
    paths: BTreeSet<PathBuf>,
}

impl Installed {
    /// The record kept in `file`; empty when there is none yet.
    pub(super) fn load(file: &Path) -> Result<Self> {
        let paths = match fs::read_to_string(file) {
            Ok(text) => text.lines().map(PathBuf::from).collect(),
            Err(e) if e.kind() == io::ErrorKind::NotFound => BTreeSet::new(),
            Err(e) => return Err(e).with_context(|| format!("reading {}", file.display())),
        };
        Ok(Self {
            file: file.to_owned(),
            paths,
        })
    }

    fn add(&mut self, path: PathBuf) -> Result<()> {
        if !self.paths.insert(path) {
            return Ok(());
        }

        let mut text = String::new();

        for path in &self.paths {
            text.push_str(&path.to_string_lossy());
            text.push('\n');
        }

        if let Some(parent) = self.file.parent() {
            fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
        }

        fs::write(&self.file, text).with_context(|| format!("writing {}", self.file.display()))
    }
}

/// What became of one skill file.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Outcome {
    Written,
    /// Beep's file, already current.
    Current,
    /// The user's own file, or Beep's that they edited.
    Kept,
    /// Beep wrote it once and the user deleted it.
    Deleted,
}

/// Installs `skill` as `<dir>/<name>/SKILL.md`, unless `installed` says it
/// was written before and is gone now.
pub(super) fn install(dir: &Path, skill: &Skill, installed: &mut Installed) -> Result<Outcome> {
    let path = dir.join(skill.name).join("SKILL.md");
    let existing = match fs::read_to_string(&path) {
        Ok(text) => Some(text),
        Err(e) if e.kind() == io::ErrorKind::NotFound && installed.paths.contains(&path) => {
            return Ok(Outcome::Deleted);
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        // Not text, or unreadable: not Beep's to replace.
        Err(e) if e.kind() == io::ErrorKind::InvalidData => return Ok(Outcome::Kept),
        Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
    };
    let wanted = marked(skill.text);

    match existing {
        Some(text) if text == wanted => {
            // Written by a node from before the record, or the record lost.
            installed.add(path)?;
            return Ok(Outcome::Current);
        }
        Some(text) if !ours_unedited(&text) => return Ok(Outcome::Kept),
        _ => {}
    }

    let parent = path.parent().context("a skill file has a folder")?;
    fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    // Written beside it and renamed, so an agent never reads half a file.
    let partial = parent.join(".SKILL.md.beep");
    fs::write(&partial, &wanted).with_context(|| format!("writing {}", partial.display()))?;
    fs::rename(&partial, &path).with_context(|| format!("writing {}", path.display()))?;
    installed.add(path)?;
    Ok(Outcome::Written)
}

/// `text` with the marker line after its front matter.
fn marked(text: &str) -> String {
    let line = format!("{MARKER}{}{MARKER_END}\n", sha256(text));
    let at = front_matter_end(text);
    format!("{}{line}{}", &text[..at], &text[at..])
}

/// Whether `text` has Beep's marker and still hashes to it.
fn ours_unedited(text: &str) -> bool {
    let Some(start) = text.find(MARKER) else {
        return false;
    };
    let Some(len) = text[start..].find('\n') else {
        return false;
    };
    let line = &text[start..start + len];
    let Some(hash) = line
        .strip_prefix(MARKER)
        .and_then(|rest| rest.strip_suffix(MARKER_END))
    else {
        return false;
    };
    let rest = format!("{}{}", &text[..start], &text[start + len + 1..]);
    sha256(&rest) == hash
}

/// Where the YAML front matter ends: after its closing `---` line, or 0
/// when the text has none.
fn front_matter_end(text: &str) -> usize {
    let Some(body) = text.strip_prefix("---\n") else {
        return 0;
    };

    match body.find("\n---\n") {
        Some(at) => 4 + at + "\n---\n".len(),
        None => 0,
    }
}

fn sha256(text: &str) -> String {
    hex::encode(Sha256::digest(text.as_bytes()))
}

#[cfg(test)]
#[path = "../../tests/machine_provision_skills.rs"]
mod tests;
