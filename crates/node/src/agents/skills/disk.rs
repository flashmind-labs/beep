//! The user's skills on this machine: the agents' skills folders under the
//! home folder, each skill a folder with a `SKILL.md`, hashed over its files.
//! Every function here blocks.

use crate::machine::provision::installed_by_beep;
use anyhow::{Context, Result, bail};
use beep_proto::node::skills::{Listed, SkillFile};
use serde_bytes::ByteBuf;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs, io,
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

/// The skills folders the agents Beep runs read, under the home folder:
/// Claude Code's, the shared `~/.agents/skills` (Codex, pi, Gemini CLI and
/// OpenCode read it), and each agent's own.
const READ: [&str; 6] = [
    ".claude/skills",
    ".agents/skills",
    ".codex/skills",
    ".pi/agent/skills",
    ".gemini/skills",
    ".config/opencode/skills",
];
/// The agents' folders that make `~/.agents/skills` worth writing to.
const AGENTS_READERS: [&str; 5] = [".agents", ".codex", ".pi", ".gemini", ".config/opencode"];

/// Marks a picked copy in the record of held names.
const KEPT: &str = "kept";

/// The most files one skill may have and still sync.
const MAX_FILES: usize = 256;
/// The most bytes one skill may hold and still sync.
const MAX_BYTES: u64 = 2 * 1024 * 1024;
/// How deep a skill's folders may go.
const MAX_DEPTH: usize = 8;

/// The skills found on disk.
#[derive(Debug, Default)]
pub(super) struct Scan {
    /// The user's own, one entry per content.
    pub listed: Vec<Listed>,
    /// Every name a skills folder holds, Beep's own and the ones too big
    /// to sync among them: a name here is never written.
    pub names: BTreeSet<String>,
}

/// Reads every skills folder under `home`.
pub(super) fn scan(home: &Path) -> Scan {
    let mut names = BTreeSet::new();
    // Name and hash, to that copy.
    let mut copies: BTreeMap<(String, String), Listed> = BTreeMap::new();
    for rel in READ {
        let root = home.join(rel);
        let Ok(entries) = fs::read_dir(&root) else {
            continue;
        };
        for entry in entries.flatten() {
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if !valid_part(&name) || !entry.path().is_dir() {
                continue;
            }
            names.insert(name.clone());
            match read(&entry.path()) {
                Ok(Some(found)) => {
                    let hash = hash(&found.files);
                    let copy = copies
                        .entry((name.clone(), hash.clone()))
                        .or_insert_with(|| Listed {
                            name,
                            hash,
                            folders: Vec::new(),
                            // The follow task fills these in from what it synced.
                            ver: 0,
                            edited: false,
                            kept: false,
                            changed: 0,
                            bytes: found.bytes,
                        });
                    copy.folders.push(label(rel));
                    copy.changed = copy.changed.max(found.changed);
                }
                Ok(None) => {}
                Err(e) => log::debug!("skill {name} in {} not read: {e:#}", label(rel)),
            }
        }
    }
    Scan {
        listed: copies.into_values().collect(),
        names,
    }
}

/// `~/` and `rel`, as the user knows the folder on any system.
fn label(rel: &str) -> String {
    format!("~/{rel}")
}

/// The files of the skill named `name` while its copy hashes to `hash`, from
/// the first skills folder that holds one.
pub(super) fn files_of(home: &Path, name: &str, hash: &str) -> Vec<SkillFile> {
    if !valid_part(name) {
        return Vec::new();
    }
    for rel in READ {
        if let Ok(Some(found)) = read(&home.join(rel).join(name))
            && self::hash(&found.files) == hash
        {
            return found.files;
        }
    }
    Vec::new()
}

/// A skill's files as read from its folder.
#[derive(Default)]
struct Found {
    /// Sorted by path.
    files: Vec<SkillFile>,
    bytes: u64,
    /// When the newest file changed, in milliseconds since the Unix epoch.
    changed: u64,
}

/// A skill's files; `None` when the folder is no skill the user made: no
/// `SKILL.md`, Beep's own, or too big to sync.
fn read(dir: &Path) -> Result<Option<Found>> {
    let skill = match fs::read(dir.join("SKILL.md")) {
        Ok(text) => text,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e).context("reading SKILL.md"),
    };
    if installed_by_beep(&String::from_utf8_lossy(&skill)) {
        return Ok(None);
    }
    let mut found = Found::default();
    if !walk(dir, "", 0, &mut found)? {
        log::debug!("{} is too big to sync", dir.display());
        return Ok(None);
    }
    found.files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(Some(found))
}

/// Adds the files under `dir` to `found`; false once the skill is past a
/// limit. Names starting with a dot (`.git`, `.DS_Store`) are left out.
fn walk(dir: &Path, prefix: &str, depth: usize, found: &mut Found) -> Result<bool> {
    if depth > MAX_DEPTH {
        return Ok(false);
    }
    for entry in fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
        let entry = entry?;
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if !valid_part(&name) {
            continue;
        }
        let path = entry.path();
        let rel = if prefix.is_empty() {
            name
        } else {
            format!("{prefix}/{name}")
        };
        // Follows links, as the agents do.
        let meta = fs::metadata(&path).with_context(|| format!("reading {}", path.display()))?;
        if meta.is_dir() {
            if !walk(&path, &rel, depth + 1, found)? {
                return Ok(false);
            }
        } else if meta.is_file() {
            found.bytes += meta.len();
            if found.files.len() >= MAX_FILES || found.bytes > MAX_BYTES {
                return Ok(false);
            }
            // A clock before 1970 reads as no time.
            if let Some(ms) = meta
                .modified()
                .ok()
                .and_then(|at| at.duration_since(UNIX_EPOCH).ok())
                .and_then(|since| u64::try_from(since.as_millis()).ok())
            {
                found.changed = found.changed.max(ms);
            }
            let data = fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
            found.files.push(SkillFile {
                path: rel,
                data: ByteBuf::from(data),
            });
        }
    }
    Ok(true)
}

/// SHA-256 over each file's path, length and bytes, in path order.
pub(super) fn hash(files: &[SkillFile]) -> String {
    let mut sorted: Vec<&SkillFile> = files.iter().collect();
    sorted.sort_by(|a, b| a.path.cmp(&b.path));
    let mut digest = Sha256::new();
    for file in sorted {
        digest.update(file.path.as_bytes());
        digest.update([0]);
        digest.update((file.data.len() as u64).to_le_bytes());
        digest.update(&file.data);
    }
    hex::encode(digest.finalize())
}

/// Whether `part` is a plain file or folder name, safe on every system: no
/// separators, no `..`, nothing hidden.
fn valid_part(part: &str) -> bool {
    !part.is_empty()
        && part.len() <= 255
        && !part.starts_with('.')
        && !part.chars().any(|c| {
            c.is_control() || matches!(c, '/' | '\\' | ':' | '<' | '>' | '"' | '|' | '?' | '*')
        })
}

/// The skills folders a skill from another machine goes into: Claude Code's
/// when `~/.claude` exists, `~/.agents/skills` when an agent that reads it is
/// there.
fn targets(home: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if home.join(".claude").is_dir() {
        out.push(home.join(".claude/skills"));
    }
    if AGENTS_READERS.iter().any(|rel| home.join(rel).is_dir()) {
        out.push(home.join(".agents/skills"));
    }
    out
}

/// Whether `files` are a skill's files Beep may write: plain names only.
fn check(name: &str, files: &[SkillFile]) -> Result<()> {
    if !valid_part(name) {
        bail!("{name:?} is not a skill name");
    }
    for file in files {
        if !file.path.split('/').all(valid_part) {
            bail!("{:?} is not a path in a skill", file.path);
        }
    }
    Ok(())
}

/// Writes `files` into `.<name>.beep` in `dir`, in place of any left there,
/// and returns that folder.
fn stage(dir: &Path, name: &str, files: &[SkillFile]) -> Result<PathBuf> {
    let partial = dir.join(format!(".{name}.beep"));
    remove(&partial)?;
    for file in files {
        let path = file
            .path
            .split('/')
            .fold(partial.clone(), |p, part| p.join(part));
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
        }
        fs::write(&path, &file.data).with_context(|| format!("writing {}", path.display()))?;
    }
    Ok(partial)
}

/// Removes the folder `path`; nothing when there is none.
fn remove(path: &Path) -> Result<()> {
    match fs::remove_dir_all(path) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => {
            Err(e).with_context(|| format!("removing {}", path.display()))
        }
        _ => Ok(()),
    }
}

/// Writes the skill `name` into each target folder that has nothing by that
/// name. Each copy is written beside its place and renamed, so an agent never
/// reads half a skill. Whether any was written.
pub(super) fn install(home: &Path, name: &str, files: &[SkillFile]) -> Result<bool> {
    check(name, files)?;
    let mut wrote = false;
    for dir in targets(home) {
        let place = dir.join(name);
        if fs::symlink_metadata(&place).is_ok() {
            continue;
        }
        let partial = stage(&dir, name, files)?;
        fs::rename(&partial, &place).with_context(|| format!("writing {}", place.display()))?;
        wrote = true;
    }
    Ok(wrote)
}

/// Puts `files` in place of the skill `name` in each of `folders` (labels as
/// `scan` gives them) whose copy still hashes to `old`, so an edit made here
/// since is kept. The new copy is staged beside the old one, then the two
/// are swapped: an agent reads the old skill or the new one, never half.
/// How many copies were replaced.
pub(super) fn replace(
    home: &Path,
    name: &str,
    files: &[SkillFile],
    folders: &[String],
    old: &str,
) -> Result<usize> {
    check(name, files)?;
    let mut replaced = 0;
    for rel in READ.iter().filter(|rel| folders.contains(&label(rel))) {
        let dir = home.join(rel);
        let place = dir.join(name);
        if !matches!(read(&place), Ok(Some(now)) if hash(&now.files) == old) {
            continue;
        }
        let partial = stage(&dir, name, files)?;
        let aside = dir.join(format!(".{name}.beep-old"));
        remove(&aside)?;
        fs::rename(&place, &aside).with_context(|| format!("moving {}", place.display()))?;
        if let Err(e) = fs::rename(&partial, &place) {
            // The old copy goes back rather than leave no skill.
            if let Err(back) = fs::rename(&aside, &place) {
                log::warn!("{} not put back: {back}", place.display());
            }
            return Err(e).with_context(|| format!("writing {}", place.display()));
        }
        remove(&aside)?;
        replaced += 1;
    }
    Ok(replaced)
}

/// Puts the copy of `name` that hashes to `hash` in place of each of `old`,
/// the other copies here, as the user kept it.
pub(super) fn keep(home: &Path, name: &str, hash: &str, old: &[Listed]) -> Result<()> {
    let files = files_of(home, name, hash);
    if files.is_empty() {
        bail!("the copy kept is gone or changed");
    }
    for copy in old {
        replace(home, name, &files, &copy.folders, &copy.hash)?;
    }
    Ok(())
}

/// The copy of a skill this machine last synced: its hash and how many
/// edits it has been through in the mesh.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Base {
    pub hash: String,
    pub ver: u64,
    /// The user picked it to end a conflict.
    pub kept: bool,
}

/// The skill names this machine has held, each with the copy it last synced,
/// in a file of the node's own: one line per name, then a tab, the hash, a
/// tab and the version when it has one, and a tab and `kept` when the user
/// picked it. A name in it is never fetched again, so a skill the user
/// deleted here stays deleted.
#[derive(Default)]
pub(super) struct Seen {
    file: PathBuf,
    names: BTreeMap<String, Option<Base>>,
    /// Changed since it was last written.
    dirty: bool,
}

impl Seen {
    pub(super) fn load(file: &Path) -> Result<Self> {
        let text = match fs::read_to_string(file) {
            Ok(text) => text,
            Err(e) if e.kind() == io::ErrorKind::NotFound => String::new(),
            Err(e) => return Err(e).with_context(|| format!("reading {}", file.display())),
        };
        let names = text
            .lines()
            .filter(|line| !line.is_empty())
            .map(|line| {
                let mut parts = line.split('\t');
                let name = parts.next().unwrap_or_default().to_owned();
                let base = match (parts.next(), parts.next().map(str::parse)) {
                    (Some(hash), Some(Ok(ver))) => Some(Base {
                        hash: hash.to_owned(),
                        ver,
                        kept: parts.next() == Some(KEPT),
                    }),
                    _ => None,
                };
                (name, base)
            })
            .collect();
        Ok(Self {
            file: file.to_owned(),
            names,
            dirty: false,
        })
    }

    pub(super) fn contains(&self, name: &str) -> bool {
        self.names.contains_key(name)
    }

    pub(super) fn base(&self, name: &str) -> Option<&Base> {
        self.names.get(name).and_then(Option::as_ref)
    }

    /// Keeps `name` as held, with no copy synced yet when it is new.
    pub(super) fn hold(&mut self, name: &str) {
        if !self.names.contains_key(name) {
            self.names.insert(name.to_owned(), None);
            self.dirty = true;
        }
    }

    /// Records `base` as the copy of `name` last synced.
    pub(super) fn set(&mut self, name: &str, base: Base) {
        if self.base(name) != Some(&base) {
            self.names.insert(name.to_owned(), Some(base));
            self.dirty = true;
        }
    }

    /// The file's text when it needs writing, and from then on it counts as
    /// written.
    pub(super) fn unsaved(&mut self) -> Option<Saved> {
        if !self.dirty {
            return None;
        }
        self.dirty = false;
        let mut text = String::new();
        for (name, base) in &self.names {
            text.push_str(name);
            if let Some(base) = base {
                text.push_str(&format!("\t{}\t{}", base.hash, base.ver));
                if base.kept {
                    text.push_str(&format!("\t{KEPT}"));
                }
            }
            text.push('\n');
        }
        Some(Saved {
            file: self.file.clone(),
            text,
        })
    }
}

/// The record's text, to write off the async workers.
pub(super) struct Saved {
    file: PathBuf,
    text: String,
}

impl Saved {
    /// Writes the record beside its place and renames it in.
    pub(super) fn write(&self) -> Result<()> {
        if let Some(parent) = self.file.parent() {
            fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
        }
        let partial = self.file.with_extension("beep");
        fs::write(&partial, &self.text)
            .with_context(|| format!("writing {}", partial.display()))?;
        fs::rename(&partial, &self.file).with_context(|| format!("writing {}", self.file.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn skill(home: &Path, rel: &str, name: &str, text: &str) {
        let dir = home.join(rel).join(name);
        fs::create_dir_all(dir.join("refs")).unwrap();
        fs::write(dir.join("SKILL.md"), text).unwrap();
        fs::write(dir.join("refs/more.md"), "more\n").unwrap();
        fs::write(dir.join(".DS_Store"), "junk").unwrap();
    }

    #[test]
    fn copies_alike_list_once_and_beeps_own_are_left_out() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path();
        skill(
            home,
            ".claude/skills",
            "notes",
            "---\nname: notes\n---\nA\n",
        );
        skill(
            home,
            ".agents/skills",
            "notes",
            "---\nname: notes\n---\nA\n",
        );
        skill(home, ".pi/agent/skills", "style", "B\n");
        skill(home, ".claude/skills", "style", "C\n");
        let beeps = "---\nname: github\n---\n<!-- Installed by Beep. Beep keeps this file current until you edit it. sha256:00 -->\n";
        skill(home, ".claude/skills", "github", beeps);
        let found = scan(home);
        let notes: Vec<_> = found.listed.iter().filter(|l| l.name == "notes").collect();
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].folders, ["~/.claude/skills", "~/.agents/skills"]);
        assert_eq!(found.listed.iter().filter(|l| l.name == "style").count(), 2);
        assert!(!found.listed.iter().any(|l| l.name == "github"));
        assert!(found.names.contains("github"));

        // What is fetched installs into a home with nothing by that name.
        let files = files_of(home, "notes", &notes[0].hash);
        assert_eq!(files.len(), 2, "the hidden file is left out");
        let other = tempfile::tempdir().unwrap();
        fs::create_dir_all(other.path().join(".claude")).unwrap();
        fs::create_dir_all(other.path().join(".codex")).unwrap();
        assert!(install(other.path(), "notes", &files).unwrap());
        let there = scan(other.path());
        assert_eq!(there.listed.len(), 1);
        assert_eq!(there.listed[0].hash, notes[0].hash);
        assert!(!install(other.path(), "notes", &files).unwrap());
    }

    #[test]
    fn a_kept_copy_replaces_the_other_folders() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path();
        skill(home, ".claude/skills", "style", "kept\n");
        skill(home, ".pi/agent/skills", "style", "edited apart\n");
        skill(home, ".agents/skills", "style", "another\n");
        let found = scan(home);
        assert_eq!(found.listed.len(), 3);
        let kept = found
            .listed
            .iter()
            .find(|l| l.folders == ["~/.claude/skills"])
            .unwrap();
        assert!(kept.changed > 0 && kept.bytes == 10);
        let old: Vec<Listed> = found
            .listed
            .iter()
            .filter(|l| l.hash != kept.hash)
            .cloned()
            .collect();
        keep(home, "style", &kept.hash, &old).unwrap();
        let after = scan(home);
        assert_eq!(after.listed.len(), 1);
        assert_eq!(after.listed[0].hash, kept.hash);
        assert_eq!(after.listed[0].folders.len(), 3);
        assert!(keep(home, "style", "gone", &[]).is_err());
    }

    #[test]
    fn unsafe_names_and_paths_are_refused() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir_all(temp.path().join(".claude")).unwrap();
        let file = |path: &str| SkillFile {
            path: path.to_owned(),
            data: ByteBuf::from(b"x".to_vec()),
        };
        assert!(install(temp.path(), "../out", &[file("SKILL.md")]).is_err());
        assert!(install(temp.path(), "ok", &[file("../SKILL.md")]).is_err());
        assert!(install(temp.path(), "ok", &[file("a\\..\\b")]).is_err());
        assert!(!temp.path().join("out").exists());
    }
}
