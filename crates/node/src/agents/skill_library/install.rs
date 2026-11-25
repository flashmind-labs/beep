//! Installing library skills: each picked skill is copied from the library's copy into the
//! agents' own skills folders or a project checkout's `.claude/skills` and `.agents/skills`,
//! and every folder written is kept in a record with the hash it was written with. A copy the
//! user edited since is never replaced or removed. Every function here blocks.

use super::store;
use crate::agents::skill_disk;
use anyhow::{Context, Result};
use beep_proto::node::{mesh_state::SkillLibrary, skill_library::Problem, skills::SkillFile};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fmt, fs, io,
    path::{Path, PathBuf},
};

/// The record of what libraries installed, in the node's folder.
const RECORD: &str = "skill-library-installs";
/// The skills folders a project's library skills go into, in its checkout.
const PROJECT_FOLDERS: [&str; 2] = [".claude/skills", ".agents/skills"];
/// The lines around the installed skills in a checkout's Git exclude file.
const BLOCK_START: &str = "# Beep skill libraries: start";
const BLOCK_END: &str = "# Beep skill libraries: end";

/// Where a library skill is installed.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub(super) enum Place {
    /// The agents' own skills folders.
    Global,
    /// A project's checkout, by project name.
    Project(String),
    /// An assistant chat's folder and its threads' folders, by the name of the chat's folder.
    Chat(String),
}

impl fmt::Display for Place {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Place::Global => f.write_str("global"),
            Place::Project(project) => f.write_str(project),
            Place::Chat(chat) => write!(f, "chat {chat}"),
        }
    }
}

/// One skill folder a library installed, and the hash of what was written there.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Install {
    pub path: PathBuf,
    pub library: String,
    pub skill: String,
    pub place: Place,
    pub hash: String,
}

/// Every skill folder libraries installed here, by folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Record {
    file: PathBuf,
    pub installs: BTreeMap<PathBuf, Install>,
}

impl Record {
    /// The record in the node's folder `node_dir`; empty when there is none yet.
    pub(super) fn load(node_dir: &Path) -> Result<Self> {
        let file = node_dir.join(RECORD);
        let installs: Vec<Install> = match fs::read(&file) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .with_context(|| format!("reading {}", file.display()))?,
            Err(e) if e.kind() == io::ErrorKind::NotFound => Vec::new(),
            Err(e) => return Err(e).with_context(|| format!("reading {}", file.display())),
        };
        Ok(Self {
            file,
            installs: installs
                .into_iter()
                .map(|install| (install.path.clone(), install))
                .collect(),
        })
    }

    /// Writes the record beside its place and renames it in.
    pub(super) fn save(&self) -> Result<()> {
        let installs: Vec<&Install> = self.installs.values().collect();
        let text = serde_json::to_vec_pretty(&installs).context("encoding the record")?;

        if let Some(parent) = self.file.parent() {
            fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
        }

        let partial = self.file.with_extension("beep");
        fs::write(&partial, text).with_context(|| format!("writing {}", partial.display()))?;
        fs::rename(&partial, &self.file).with_context(|| format!("writing {}", self.file.display()))
    }

    /// The names of the skills installed in the agents' own skills folders.
    pub(super) fn global_names(&self) -> BTreeSet<String> {
        self.installs
            .values()
            .filter(|install| install.place == Place::Global)
            .map(|install| install.skill.clone())
            .collect()
    }
}

/// A library as one pass holds it: its copy at the revision, or none when it could not be
/// fetched, so what it installed stays as it is.
pub(super) struct Ready {
    pub name: String,
    pub library: SkillLibrary,
    pub store: Option<PathBuf>,
}

/// A folder on this machine that gets the skills picked for its place: a project's checkout, or
/// an assistant chat's folder or one of its threads' folders.
#[derive(Debug, Clone)]
pub(super) struct Checkout {
    pub place: Place,
    pub folder: PathBuf,
    pub keep_out_of_git: bool,
}

/// One skill folder a library should have installed.
#[derive(Debug, Clone)]
pub(super) struct Want {
    pub path: PathBuf,
    pub library: String,
    pub skill: String,
    pub place: Place,
    /// The skill's folder in the library's copy; none while the library could not be fetched.
    pub source: Option<PathBuf>,
}

/// The skill folders `ready` asks for: global picks in each of `global`, project and chat picks
/// in each checkout's skills folders.
pub(super) fn wants(ready: &[Ready], global: &[PathBuf], checkouts: &[Checkout]) -> Vec<Want> {
    let mut out = Vec::new();

    for lib in ready {
        let skills: Vec<(String, Option<PathBuf>)> = match &lib.store {
            Some(root) => store::skills_in(root, &lib.library.url)
                .into_iter()
                .map(|(name, dir)| (name, Some(dir)))
                .collect(),
            None => lib
                .library
                .skills
                .iter()
                .map(|skill| (skill.name.clone(), None))
                .collect(),
        };

        for (skill, source) in skills {
            let want = |path: PathBuf, place: Place| Want {
                path,
                library: lib.name.clone(),
                skill: skill.clone(),
                place,
                source: source.clone(),
            };

            if lib.library.global.has(&skill) {
                out.extend(global.iter().map(|dir| want(dir.join(&skill), Place::Global)));
            }

            for checkout in checkouts {
                let picks = match &checkout.place {
                    Place::Project(project) => lib.library.projects.get(project),
                    Place::Chat(chat) => lib.library.chats.get(chat),
                    Place::Global => None,
                };

                if picks.is_some_and(|pick| pick.has(&skill)) {
                    out.extend(PROJECT_FOLDERS.iter().map(|rel| {
                        want(checkout.folder.join(rel).join(&skill), checkout.place.clone())
                    }));
                }
            }
        }
    }

    out
}

/// What a skill folder holds now.
enum At {
    Missing,
    /// A skill whose files hash to this.
    Skill(String),
    /// Something else: no `SKILL.md`, Beep's own skill, too big, unreadable.
    Other,
}

fn at(path: &Path) -> At {
    match fs::symlink_metadata(path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => return At::Missing,
        Err(_) => return At::Other,
        Ok(_) => {}
    }

    match skill_disk::read(path) {
        Ok(Some(files)) => At::Skill(skill_disk::hash(&files)),
        _ => At::Other,
    }
}

/// Installs every one of `wants`, uninstalls what `record` holds that none asks for, and keeps
/// each checkout's Git exclude block in step. What could not be done, as problems.
pub(super) fn apply(record: &mut Record, mut wants: Vec<Want>, checkouts: &[Checkout]) -> Vec<Problem> {
    let mut problems = Vec::new();
    let mut desired = BTreeSet::new();
    let mut sources: HashMap<PathBuf, Option<(Vec<SkillFile>, String)>> = HashMap::new();
    // A folder stays with the library that installed it when two ask for it.
    wants.sort_by_key(|want| {
        let owner = record
            .installs
            .get(&want.path)
            .is_some_and(|install| install.library == want.library);
        !owner
    });

    for want in wants {
        let problem = |message: String| Problem {
            library: want.library.clone(),
            skill: Some(want.skill.clone()),
            place: want.place.to_string(),
            message,
        };

        if !desired.insert(want.path.clone()) {
            problems.push(problem(format!(
                "another library installs a skill named {} there",
                want.skill
            )));
            continue;
        }

        let Some(source) = &want.source else {
            continue;
        };
        let copy = sources.entry(source.clone()).or_insert_with(|| {
            skill_disk::read(source)
                .ok()
                .flatten()
                .map(|files| {
                    let hash = skill_disk::hash(&files);
                    (files, hash)
                })
        });
        let Some((files, hash)) = copy else {
            problems.push(problem(format!("{} cannot be read", want.skill)));
            continue;
        };

        match install(record, &want, files, hash) {
            Ok(None) => {}
            Ok(Some(message)) => problems.push(problem(message)),
            Err(e) => problems.push(problem(format!("{e:#}"))),
        }
    }

    uninstall(record, &desired);

    for checkout in checkouts {
        if !matches!(checkout.place, Place::Project(_)) {
            continue;
        }

        if let Err(e) = exclude(record, checkout) {
            log::debug!("skill libraries: the Git exclude of {} not written: {e:#}", checkout.place);
        }
    }

    problems
}

/// Installs one skill folder, records it, and says why it was left alone, if it was.
fn install(record: &mut Record, want: &Want, files: &[SkillFile], hash: &str) -> Result<Option<String>> {
    let now = at(&want.path);
    let recorded = record.installs.get(&want.path);

    match recorded {
        Some(old) if old.library == want.library && old.skill == want.skill => match now {
            At::Skill(found) if found == hash => {}
            At::Missing => write(&want.path, files)?,
            At::Skill(found) if found == old.hash => write(&want.path, files)?,
            // Edited here: an update waits until the user puts the copy back.
            _ if old.hash != hash => {
                return Ok(Some(format!(
                    "{} was edited here, so it is not updated",
                    want.skill
                )));
            }
            _ => return Ok(None),
        },
        Some(_) => {
            return Ok(Some(format!("a skill named {} is already there", want.skill)));
        }
        None => match now {
            At::Missing => write(&want.path, files)?,
            // The same skill, put there before the library was added: it is the library's now.
            At::Skill(found) if found == hash => {}
            _ => return Ok(Some(format!("a skill named {} is already there", want.skill))),
        },
    }

    record.installs.insert(
        want.path.clone(),
        Install {
            path: want.path.clone(),
            library: want.library.clone(),
            skill: want.skill.clone(),
            place: want.place.clone(),
            hash: hash.to_owned(),
        },
    );
    Ok(None)
}

/// Writes `files` as the skill folder `path`, in place of what is there: staged beside it and
/// renamed in, so an agent never reads half a skill.
pub(super) fn write(path: &Path, files: &[SkillFile]) -> Result<()> {
    let dir = path.parent().context("a skill folder has a parent")?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .context("a skill folder has a name")?;
    skill_disk::check(name, files)?;
    fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let partial = skill_disk::stage(dir, name, files)?;

    match fs::symlink_metadata(path) {
        Ok(_) => skill_disk::swap(&partial, dir, name),
        Err(_) => fs::rename(&partial, path).with_context(|| format!("writing {}", path.display())),
    }
}

/// Removes each recorded folder no library asks for any more, unless the user edited it; either
/// way it leaves the record.
fn uninstall(record: &mut Record, desired: &BTreeSet<PathBuf>) {
    let gone: Vec<Install> = record
        .installs
        .values()
        .filter(|install| !desired.contains(&install.path))
        .cloned()
        .collect();

    for install in gone {
        match at(&install.path) {
            At::Skill(found) if found == install.hash => {
                if let Err(e) = skill_disk::remove_entry(&install.path) {
                    log::warn!("skill libraries: {} not removed: {e:#}", install.path.display());
                    continue;
                }
            }
            At::Missing => {}
            _ => log::info!(
                "skill libraries: {} was edited here, so it stays",
                install.path.display()
            ),
        }

        record.installs.remove(&install.path);
    }
}

/// Lists the library skills installed in `checkout` in its Git exclude file, between Beep's
/// lines, or takes the lines out when none is installed or the project keeps them in Git.
fn exclude(record: &Record, checkout: &Checkout) -> Result<()> {
    let names: BTreeSet<&str> = record
        .installs
        .values()
        .filter(|install| install.place == checkout.place && install.path.starts_with(&checkout.folder))
        .map(|install| install.skill.as_str())
        .collect();
    let lines: Vec<String> = if checkout.keep_out_of_git {
        names
            .iter()
            .flat_map(|name| PROJECT_FOLDERS.iter().map(move |rel| format!("/{rel}/{name}/")))
            .collect()
    } else {
        Vec::new()
    };

    let Ok(found) = store::git(&checkout.folder, &["rev-parse", "--git-path", "info/exclude"]) else {
        return Ok(());
    };
    let file = checkout.folder.join(found.trim());
    let text = match fs::read_to_string(&file) {
        Ok(text) => text,
        Err(e) if e.kind() == io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e).with_context(|| format!("reading {}", file.display())),
    };
    let new = with_block(&text, &lines);

    if new == text {
        return Ok(());
    }

    if let Some(parent) = file.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }

    fs::write(&file, new).with_context(|| format!("writing {}", file.display()))
}

/// `text` with Beep's block holding `lines` at its end, or without the block when there are
/// none. Lines outside the block stay as they are.
pub(super) fn with_block(text: &str, lines: &[String]) -> String {
    let mut out = String::new();
    let mut inside = false;

    for line in text.lines() {
        match line.trim_end() {
            BLOCK_START => inside = true,
            BLOCK_END if inside => inside = false,
            _ if !inside => {
                out.push_str(line);
                out.push('\n');
            }
            _ => {}
        }
    }

    if !lines.is_empty() {
        out.push_str(BLOCK_START);
        out.push('\n');

        for line in lines {
            out.push_str(line);
            out.push('\n');
        }

        out.push_str(BLOCK_END);
        out.push('\n');
    }

    // Untouched when the block did not change, even if the file did not end in a newline.
    if out.trim_end() == text.trim_end() { text.to_owned() } else { out }
}

#[cfg(test)]
#[path = "../../tests/agents_skill_library_install.rs"]
mod tests;
