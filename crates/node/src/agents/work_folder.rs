//! A session's working folder for the apps' Files tab (`WorkRoot`): `list`
//! and `tree` its folders with git's ignored and changed marks, `read` a file
//! a part at a time, and `write` files dropped on the session.

use super::media_file::Chunk;
use anyhow::{Context, Result, bail};
use beep_proto::{
    AGENT_FILE_CHUNK, GitChange, MAX_TREE_DEPTH, MAX_TREE_ENTRIES, MAX_WORK_ENTRIES,
    MAX_WORK_UPLOAD, WorkEntry, WorkFolder, WorkTree, media_mime,
};
use ignore::WalkBuilder;
use std::{
    collections::{HashMap, HashSet, VecDeque},
    fs::{self, OpenOptions},
    io::{ErrorKind, Read, Seek, SeekFrom, Write},
    path::{Component, Path, PathBuf},
    process::{Command, Stdio},
};

/// What a file being written is called until its last part is in, beside
/// it and hidden: `.notes.txt.beep-part`.
const PART: &str = ".beep-part";

/// The folder uploads go to by default; written into it, it gets a
/// `.gitignore` so they stay out of commits.
const BEEP_FOLDER: &str = ".beep";

/// A session's working folder, its symlinks resolved. Nothing outside it
/// is read or written: every path is checked once its symlinks resolve.
pub(super) struct WorkRoot {
    root: PathBuf,
}

impl WorkRoot {
    /// The folder `real`, its links already resolved and checked by
    /// `reach`. Refused when a link appeared on the way since.
    pub(super) fn at(real: &Path) -> Result<Self> {
        let shown = real.display();
        let root_dir =
            fs::canonicalize(real).with_context(|| format!("the folder {shown} is not there"))?;
        if root_dir != real {
            bail!("{shown} changed while it was checked");
        }
        if !root_dir.is_dir() {
            bail!("{shown} is not a folder");
        }
        Ok(Self { root: root_dir })
    }

    /// What `rel` (already checked by `names`) leads to inside the root,
    /// symlinks resolved; `None` when nothing is there.
    fn find(&self, rel: &Path) -> Result<Option<PathBuf>> {
        let full = match fs::canonicalize(self.root.join(rel)) {
            Ok(full) => full,
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e).with_context(|| format!("could not open {}", rel.display())),
        };
        if !full.starts_with(&self.root) {
            bail!("{} leads outside the session's folder", rel.display());
        }
        Ok(Some(full))
    }

    /// The folder `rel` inside the root, made with the ones on the way
    /// when missing. Each is checked as it comes, so a symlink out of the
    /// root stops it before anything is made beyond.
    fn make_folder(&self, rel: &Path) -> Result<PathBuf> {
        let mut at = self.root.clone();
        for name in rel.components() {
            let next = at.join(name);
            match fs::create_dir(&next) {
                Ok(()) => {}
                Err(e) if e.kind() == ErrorKind::AlreadyExists => {}
                Err(e) => {
                    return Err(e).with_context(|| format!("could not make {}", rel.display()));
                }
            }
            let real = fs::canonicalize(&next)
                .with_context(|| format!("could not open {}", rel.display()))?;
            if !real.starts_with(&self.root) {
                bail!("{} leads outside the session's folder", rel.display());
            }
            if !real.is_dir() {
                bail!("{} is not a folder", rel.display());
            }
            at = real;
        }
        Ok(at)
    }

    /// `full`, inside the root, as a path relative to it with `/` between
    /// names.
    fn relative(&self, full: &Path) -> String {
        full.strip_prefix(&self.root)
            .unwrap_or(full)
            .components()
            .map(|part| part.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/")
    }
}

/// `rel` as names inside the root: relative, `/` between names, no `..`;
/// empty names and `.` are dropped.
fn names(rel: &str) -> Result<PathBuf> {
    let mut out = PathBuf::new();
    for name in rel.split('/') {
        match name {
            "" | "." => continue,
            ".." => bail!("{rel} leads outside the session's folder"),
            _ => {}
        }
        let mut parts = Path::new(name).components();
        match (parts.next(), parts.next()) {
            (Some(Component::Normal(name)), None) => out.push(name),
            _ => bail!("{rel} is not a path inside the session's folder"),
        }
    }
    Ok(out)
}

/// The folder `path` inside `root`: what git ignores left out unless
/// `ignored`, `.git` always. Blocks on the file system and git.
pub(super) fn list(root: &WorkRoot, path: &str, ignored: bool) -> Result<WorkFolder> {
    let dir = open_folder(root, path)?;
    let status = git_status(&dir);
    let changes = status.as_ref().map(|s| changes_in(&s.out, &s.prefix));
    read_folder(&dir, root.relative(&dir), ignored, changes.as_ref())
}

/// The folder `path` inside `root` and those under it, `depth` levels in
/// all, each as `list` gives it. Folders git ignores and symlinks are not
/// read into, and it stops past `MAX_TREE_ENTRIES` entries. Blocks on the
/// file system and git.
pub(super) fn tree(root: &WorkRoot, path: &str, ignored: bool, depth: u32) -> Result<WorkTree> {
    let top = open_folder(root, path)?;
    let status = git_status(&top);
    let depth = depth.clamp(1, MAX_TREE_DEPTH);
    let top_rel = root.relative(&top);
    // Each folder by its path under `top`, and how many levels down it is.
    let mut queue = VecDeque::from([(String::new(), 1)]);
    let mut folders: Vec<WorkFolder> = Vec::new();
    let mut entries = 0;
    let mut truncated = false;
    while let Some((sub, level)) = queue.pop_front() {
        let dir = top.join(&sub);
        let changes = status
            .as_ref()
            .map(|s| changes_in(&s.out, &format!("{}{}", s.prefix, slashed(&sub))));
        let folder = match read_folder(&dir, joined(&top_rel, &sub), ignored, changes.as_ref()) {
            Ok(folder) => folder,
            // A folder under the top that cannot be read is left unread;
            // opening it says why.
            Err(_) if !sub.is_empty() => continue,
            Err(e) => return Err(e),
        };
        entries += folder.entries.len();
        if entries > MAX_TREE_ENTRIES && !folders.is_empty() {
            truncated = true;
            break;
        }
        if level < depth {
            for entry in folder.entries.iter().filter(|e| e.folder && !e.ignored) {
                let next = joined(&sub, &entry.name);
                if fs::symlink_metadata(top.join(&next)).is_ok_and(|meta| meta.is_dir()) {
                    queue.push_back((next, level + 1));
                }
            }
        }
        folders.push(folder);
    }
    Ok(WorkTree { folders, truncated })
}

/// `a/b`, or either alone when the other is empty.
fn joined(a: &str, b: &str) -> String {
    match (a.is_empty(), b.is_empty()) {
        (true, _) => b.to_owned(),
        (_, true) => a.to_owned(),
        _ => format!("{a}/{b}"),
    }
}

/// `sub/`, or empty for the top.
fn slashed(sub: &str) -> String {
    if sub.is_empty() {
        String::new()
    } else {
        format!("{sub}/")
    }
}

/// The folder `path` inside `root`, its symlinks resolved.
fn open_folder(root: &WorkRoot, path: &str) -> Result<PathBuf> {
    let dir = root
        .find(&names(path)?)?
        .with_context(|| format!("{path} is not there"))?;
    if !dir.is_dir() {
        bail!("{path} is not a folder");
    }
    Ok(dir)
}

/// The folder `dir`, called `path`, with what git sees changed in it.
fn read_folder(
    dir: &Path,
    path: String,
    ignored: bool,
    changes: Option<&HashMap<String, GitChange>>,
) -> Result<WorkFolder> {
    let kept = not_ignored(dir);
    let mut entries = Vec::new();
    let items = fs::read_dir(dir).with_context(|| format!("could not list {path}"))?;
    for item in items {
        let item = item.with_context(|| format!("could not list {path}"))?;
        let name = item.file_name();
        // A name that is not UTF-8 cannot be named back to the node.
        let Some(name) = name.to_str() else { continue };
        if name == ".git" {
            continue;
        }
        let is_ignored = !kept.contains(name);
        if is_ignored && !ignored {
            continue;
        }
        // A symlink shows as what it leads to; a broken one as itself.
        let meta = match fs::metadata(item.path()) {
            Ok(meta) => meta,
            Err(_) => item
                .metadata()
                .with_context(|| format!("could not read {name} in {path}"))?,
        };
        let folder = meta.is_dir();
        entries.push(WorkEntry {
            name: name.to_owned(),
            folder,
            size: if folder { 0 } else { meta.len() },
            ignored: is_ignored,
            change: changes.and_then(|c| c.get(name).copied()),
        });
    }
    entries.sort_by(|a, b| {
        b.folder
            .cmp(&a.folder)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            .then_with(|| a.name.cmp(&b.name))
    });
    let more = entries.len().saturating_sub(MAX_WORK_ENTRIES);
    entries.truncate(MAX_WORK_ENTRIES);
    Ok(WorkFolder {
        path,
        entries,
        more: u32::try_from(more).unwrap_or(u32::MAX),
        git: changes.is_some(),
    })
}

/// The names in `dir` git does not ignore, by the `.gitignore` files on
/// the way from the checkout, its excludes and the global ones. Outside a
/// checkout, every name.
fn not_ignored(dir: &Path) -> HashSet<String> {
    WalkBuilder::new(dir)
        .max_depth(Some(1))
        .hidden(false)
        .ignore(false)
        .parents(true)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .require_git(true)
        .follow_links(false)
        .build()
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.depth() == 1)
        .filter_map(|entry| entry.file_name().to_str().map(str::to_owned))
        .collect()
}

/// What git sees changed under `dir`: `git status` output and `dir`'s
/// path from the checkout's top, as `changes_in` reads them.
struct GitStatus {
    out: String,
    prefix: String,
}

/// `None` outside a checkout or without git, where there is nothing to
/// mark.
fn git_status(dir: &Path) -> Option<GitStatus> {
    let prefix = git(dir, &["rev-parse", "--show-prefix"])?;
    let out = git(
        dir,
        &[
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=normal",
            "--ignored=no",
            "--",
            ".",
        ],
    )?;
    Some(GitStatus {
        out,
        prefix: prefix.trim_end_matches('\n').to_owned(),
    })
}

/// What `git <args>` printed in `dir`, when it succeeded. Takes no locks,
/// so a running agent's own git commands are never in its way.
pub(in crate::agents) fn git(dir: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// `git status --porcelain=v1 -z` output, its paths from the checkout's
/// top, read for the folder `prefix` (`src/`, or empty for the top): each
/// name in it with the strongest change under it.
fn changes_in(status: &str, prefix: &str) -> HashMap<String, GitChange> {
    let mut out: HashMap<String, GitChange> = HashMap::new();
    let mut fields = status.split('\0');
    while let Some(field) = fields.next() {
        let (Some(code), Some(path)) = (field.get(..2), field.get(3..)) else {
            continue;
        };
        // A rename or copy names where it came from next.
        if code.contains(['R', 'C']) {
            fields.next();
        }
        let Some(inside) = path.strip_prefix(prefix) else {
            continue;
        };
        let Some(name) = inside.split('/').next().filter(|name| !name.is_empty()) else {
            continue;
        };
        let change = match code {
            "??" => GitChange::Untracked {},
            "AA" | "DD" => GitChange::Conflicted {},
            _ if code.contains('U') => GitChange::Conflicted {},
            _ if code.starts_with('A') => GitChange::Added {},
            _ => GitChange::Modified {},
        };
        out.entry(name.to_owned())
            .and_modify(|had| *had = (*had).max(change))
            .or_insert(change);
    }
    out
}

/// Up to `len` bytes, at most `AGENT_FILE_CHUNK`, of the file `path` inside
/// `root` from `offset`; `None` when there is no such file. Blocks on the
/// file system.
pub(super) fn read(root: &WorkRoot, path: &str, offset: u64, len: u32) -> Result<Option<Chunk>> {
    let Some(full) = root.find(&names(path)?)? else {
        return Ok(None);
    };
    let mut file =
        crate::files::fsat::open_read(&full).with_context(|| format!("could not open {path}"))?;
    let meta = file
        .metadata()
        .with_context(|| format!("could not read {path}"))?;
    if !meta.is_file() {
        bail!("{path} is not a file");
    }
    let size = meta.len();
    let mut data = Vec::new();
    if offset < size {
        let want = (size - offset)
            .min(u64::from(len))
            .min(AGENT_FILE_CHUNK as u64);
        file.seek(SeekFrom::Start(offset))
            .and_then(|_| file.take(want).read_to_end(&mut data))
            .with_context(|| format!("could not read {path}"))?;
    }
    Ok(Some(Chunk {
        size,
        mime: media_mime(path).unwrap_or("application/octet-stream"),
        data,
    }))
}

/// One part of a file written to `path` inside `root`, as
/// `WriteWorkFile` sends it; the path it writes to, relative to `root`.
/// The first part takes a free name; the file shows under it once the
/// last part is in. Blocks on the file system.
pub(super) fn write(
    root: &WorkRoot,
    path: &str,
    offset: u64,
    size: u64,
    data: &[u8],
) -> Result<String> {
    if size > MAX_WORK_UPLOAD {
        bail!(
            "the file is {} MB, over the {} MB a drop uploads",
            size.div_ceil(1024 * 1024),
            MAX_WORK_UPLOAD / (1024 * 1024)
        );
    }
    if data.len() > AGENT_FILE_CHUNK {
        bail!("a part is over {} bytes", AGENT_FILE_CHUNK);
    }
    let end = offset
        .checked_add(u64::try_from(data.len())?)
        .filter(|end| *end <= size)
        .context("a part runs past the file's size")?;
    let rel = names(path)?;
    let (Some(asked), Some(within)) = (rel.file_name().and_then(|n| n.to_str()), rel.parent())
    else {
        bail!("{path} names no file");
    };
    if asked.ends_with(PART) {
        bail!("{asked} is not a name a file can take here");
    }
    let (folder, name) = if offset == 0 {
        let folder = root.make_folder(within)?;
        if within.starts_with(BEEP_FOLDER) {
            keep_out_of_git(&root.root.join(BEEP_FOLDER))?;
        }
        let name = free_name(&folder, asked)?;
        (folder, name)
    } else {
        let folder = root
            .find(within)?
            .with_context(|| format!("the upload of {path} stopped part way; drop it again"))?;
        (folder, asked.to_owned())
    };
    let part = folder.join(format!(".{name}{PART}"));
    let mut file = if offset == 0 {
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&part)
            .with_context(|| format!("could not write {path}"))?
    } else {
        // Not followed through a symlink, and only where the last part ended.
        let had = fs::symlink_metadata(&part).ok();
        if !had.is_some_and(|meta| meta.is_file() && meta.len() == offset) {
            bail!("the upload of {path} stopped part way; drop it again");
        }
        OpenOptions::new()
            .append(true)
            .open(&part)
            .with_context(|| format!("could not write {path}"))?
    };
    file.write_all(data)
        .with_context(|| format!("could not write {path}"))?;
    drop(file);
    let to = folder.join(&name);
    if end == size {
        if fs::symlink_metadata(&to).is_ok() {
            fs::remove_file(&part).with_context(|| format!("could not clear {path}"))?;
            bail!("{name} appeared while it uploaded; drop it again");
        }
        fs::rename(&part, &to).with_context(|| format!("could not write {path}"))?;
    }
    Ok(root.relative(&to))
}

/// `asked`, or the first of `stem 2.ext`, `stem 3.ext`, ... that neither a
/// file in `folder` nor one being written there has.
fn free_name(folder: &Path, asked: &str) -> Result<String> {
    let (stem, ext) = match asked.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => (stem, format!(".{ext}")),
        _ => (asked, String::new()),
    };
    let taken = |name: &str| {
        fs::symlink_metadata(folder.join(name)).is_ok()
            || fs::symlink_metadata(folder.join(format!(".{name}{PART}"))).is_ok()
    };
    if !taken(asked) {
        return Ok(asked.to_owned());
    }
    (2..1000)
        .map(|n| format!("{stem} {n}{ext}"))
        .find(|name| !taken(name))
        .with_context(|| format!("there are too many files named like {asked}"))
}

/// Gives the `.beep` folder a `.gitignore` that leaves all of it out, once.
fn keep_out_of_git(beep: &Path) -> Result<()> {
    let at = beep.join(".gitignore");
    match OpenOptions::new().write(true).create_new(true).open(&at) {
        Ok(mut file) => file
            .write_all(b"*\n")
            .with_context(|| format!("could not write {}", at.display())),
        Err(e) if e.kind() == ErrorKind::AlreadyExists => Ok(()),
        Err(e) => Err(e).with_context(|| format!("could not write {}", at.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::checkout::tests::{has_git, run as run_git};
    use tempfile::TempDir;

    fn root_in(dir: &TempDir) -> WorkRoot {
        WorkRoot::at(&fs::canonicalize(dir.path()).expect("a temp folder resolves"))
            .expect("a temp folder is a root")
    }

    fn names_of(folder: &WorkFolder) -> Vec<&str> {
        folder.entries.iter().map(|e| e.name.as_str()).collect()
    }

    #[test]
    fn paths_stay_inside_the_root() {
        assert_eq!(names("src/./lib.rs").unwrap(), PathBuf::from("src/lib.rs"));
        assert_eq!(names("").unwrap(), PathBuf::new());
        for bad in ["../secret", "src/../../etc", "a/.."] {
            let e = names(bad).unwrap_err();
            assert!(e.to_string().contains("outside"), "{bad}: {e:#}");
        }
        // A leading slash is only an empty name; the path stays relative.
        assert_eq!(names("/etc/passwd").unwrap(), PathBuf::from("etc/passwd"));
    }

    #[test]
    fn leaves_out_what_git_ignores_unless_asked() {
        let dir = TempDir::new().unwrap();
        // The checkout's `.git` is what makes `.gitignore` count.
        fs::create_dir_all(dir.path().join(".git")).unwrap();
        fs::write(dir.path().join(".gitignore"), "target/\n*.log\n").unwrap();
        fs::create_dir_all(dir.path().join("target/debug")).unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("build.log"), "").unwrap();
        fs::write(dir.path().join("src/run.log"), "").unwrap();
        fs::write(dir.path().join("src/main.rs"), "").unwrap();
        let root = root_in(&dir);
        assert_eq!(
            names_of(&list(&root, "", false).unwrap()),
            ["src", ".gitignore"]
        );
        let all = list(&root, "", true).unwrap();
        assert_eq!(names_of(&all), ["src", "target", ".gitignore", "build.log"]);
        let ignored: Vec<_> = all
            .entries
            .iter()
            .filter(|e| e.ignored)
            .map(|e| e.name.as_str())
            .collect();
        assert_eq!(ignored, ["target", "build.log"]);
        // The root's `.gitignore` counts in a folder under it too.
        assert_eq!(names_of(&list(&root, "src", false).unwrap()), ["main.rs"]);
    }

    #[test]
    fn marks_what_git_sees_changed() {
        if !has_git() {
            eprintln!("skipped: no git");
            return;
        }
        let dir = TempDir::new().unwrap();
        let at = dir.path();
        run_git(at, &["init", "-q"]);
        fs::create_dir_all(at.join("src/deep")).unwrap();
        fs::write(at.join("src/deep/lib.rs"), "one").unwrap();
        fs::write(at.join("README.md"), "one").unwrap();
        run_git(at, &["add", "."]);
        run_git(at, &["commit", "-qm", "first"]);
        fs::write(at.join("src/deep/lib.rs"), "two").unwrap();
        fs::write(at.join("new.txt"), "").unwrap();
        fs::create_dir_all(at.join("notes")).unwrap();
        fs::write(at.join("notes/a.md"), "").unwrap();
        let root = root_in(&dir);
        let top = list(&root, "", false).unwrap();
        assert!(top.git);
        let change =
            |f: &WorkFolder, name: &str| f.entries.iter().find(|e| e.name == name).unwrap().change;
        assert_eq!(change(&top, "src"), Some(GitChange::Modified {}));
        assert_eq!(change(&top, "notes"), Some(GitChange::Untracked {}));
        assert_eq!(change(&top, "new.txt"), Some(GitChange::Untracked {}));
        assert_eq!(change(&top, "README.md"), None);
        let src = list(&root, "src", false).unwrap();
        assert_eq!(change(&src, "deep"), Some(GitChange::Modified {}));
    }

    fn paths_of(tree: &WorkTree) -> Vec<&str> {
        tree.folders.iter().map(|f| f.path.as_str()).collect()
    }

    #[test]
    fn a_tree_reads_levels_down_to_its_depth() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".git")).unwrap();
        fs::write(dir.path().join(".gitignore"), "target/\n").unwrap();
        fs::create_dir_all(dir.path().join("src/deep/deeper")).unwrap();
        fs::create_dir_all(dir.path().join("target/debug")).unwrap();
        fs::write(dir.path().join("src/deep/lib.rs"), "").unwrap();
        let root = root_in(&dir);
        let two = tree(&root, "", false, 2).unwrap();
        assert_eq!(paths_of(&two), ["", "src"]);
        assert!(!two.truncated);
        let three = tree(&root, "", false, 3).unwrap();
        assert_eq!(paths_of(&three), ["", "src", "src/deep"]);
        assert_eq!(names_of(&three.folders[2]), ["deeper", "lib.rs"]);
        // From a folder inside, paths stay relative to the root.
        assert_eq!(
            paths_of(&tree(&root, "src", false, 2).unwrap()),
            ["src", "src/deep"]
        );
        // What git ignores is listed when asked, but not read into.
        let all = tree(&root, "", true, 3).unwrap();
        assert_eq!(names_of(&all.folders[0]), ["src", "target", ".gitignore"]);
        assert_eq!(paths_of(&all), ["", "src", "src/deep"]);
    }

    #[test]
    fn a_tree_stops_past_its_entries() {
        let dir = TempDir::new().unwrap();
        for folder in ["a", "b", "c"] {
            let at = dir.path().join(folder);
            fs::create_dir_all(&at).unwrap();
            for n in 0..MAX_WORK_ENTRIES {
                fs::write(at.join(n.to_string()), "").unwrap();
            }
        }
        let got = tree(&root_in(&dir), "", false, 3).unwrap();
        assert_eq!(paths_of(&got), ["", "a", "b"]);
        assert!(got.truncated);
    }

    #[test]
    fn a_tree_marks_changes_in_each_folder() {
        if !has_git() {
            eprintln!("skipped: no git");
            return;
        }
        let dir = TempDir::new().unwrap();
        let at = dir.path();
        run_git(at, &["init", "-q"]);
        fs::create_dir_all(at.join("src/deep")).unwrap();
        fs::write(at.join("src/deep/lib.rs"), "one").unwrap();
        fs::write(at.join("src/main.rs"), "one").unwrap();
        run_git(at, &["add", "."]);
        run_git(at, &["commit", "-qm", "first"]);
        fs::write(at.join("src/deep/lib.rs"), "two").unwrap();
        let got = tree(&root_in(&dir), "src", false, 2).unwrap();
        let change =
            |f: &WorkFolder, name: &str| f.entries.iter().find(|e| e.name == name).unwrap().change;
        assert_eq!(
            change(&got.folders[0], "deep"),
            Some(GitChange::Modified {})
        );
        assert_eq!(change(&got.folders[0], "main.rs"), None);
        assert_eq!(
            change(&got.folders[1], "lib.rs"),
            Some(GitChange::Modified {})
        );
        assert!(got.folders[1].git);
    }

    #[test]
    fn reads_status_by_the_first_name_under_the_folder() {
        let status = "M  src/a.rs\0?? src/new/\0A  src/b.rs\0UU src/c.rs\0R  src/d.rs\0old/d.rs\0 M top.rs\0";
        let got = changes_in(status, "src/");
        assert_eq!(got.get("a.rs"), Some(&GitChange::Modified {}));
        assert_eq!(got.get("new"), Some(&GitChange::Untracked {}));
        assert_eq!(got.get("b.rs"), Some(&GitChange::Added {}));
        assert_eq!(got.get("c.rs"), Some(&GitChange::Conflicted {}));
        assert_eq!(got.get("d.rs"), Some(&GitChange::Modified {}));
        assert_eq!(got.len(), 5, "{got:?}");
        let top = changes_in(status, "");
        assert_eq!(top.get("src"), Some(&GitChange::Conflicted {}));
        assert_eq!(top.get("top.rs"), Some(&GitChange::Modified {}));
        assert_eq!(top.get("old"), None);
    }

    #[test]
    fn reads_any_file_a_part_at_a_time() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("notes.txt"), "0123456789").unwrap();
        let root = root_in(&dir);
        let chunk = read(&root, "notes.txt", 2, 3).unwrap().unwrap();
        assert_eq!(chunk.size, 10);
        assert_eq!(chunk.data, b"234");
        assert_eq!(chunk.mime, "application/octet-stream");
        assert!(read(&root, "gone.txt", 0, 10).unwrap().is_none());
        assert!(read(&root, "../notes.txt", 0, 10).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_out_of_the_root_is_refused() {
        let dir = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        fs::write(outside.path().join("secret.txt"), "secret").unwrap();
        std::os::unix::fs::symlink(outside.path(), dir.path().join("out")).unwrap();
        std::os::unix::fs::symlink(
            outside.path().join("secret.txt"),
            dir.path().join("link.txt"),
        )
        .unwrap();
        let root = root_in(&dir);
        for e in [
            list(&root, "out", true).unwrap_err(),
            read(&root, "out/secret.txt", 0, 10).unwrap_err(),
            read(&root, "link.txt", 0, 10).unwrap_err(),
            write(&root, "out/dropped.txt", 0, 2, b"hi").unwrap_err(),
        ] {
            assert!(e.to_string().contains("outside"), "{e:#}");
        }
        assert!(!outside.path().join("dropped.txt").exists());
    }

    #[test]
    fn uploads_land_in_beep_uploads_under_a_free_name() {
        let dir = TempDir::new().unwrap();
        let root = root_in(&dir);
        let first = write(&root, ".beep/uploads/build.log", 0, 3, b"one").unwrap();
        assert_eq!(first, ".beep/uploads/build.log");
        let second = write(&root, ".beep/uploads/build.log", 0, 3, b"two").unwrap();
        assert_eq!(second, ".beep/uploads/build 2.log");
        assert_eq!(
            fs::read(dir.path().join(".beep/uploads/build 2.log")).unwrap(),
            b"two"
        );
        assert_eq!(
            fs::read_to_string(dir.path().join(".beep/.gitignore")).unwrap(),
            "*\n"
        );
    }

    #[test]
    fn a_file_in_parts_shows_once_its_last_part_is_in() {
        let dir = TempDir::new().unwrap();
        let root = root_in(&dir);
        let at = write(&root, "src/data.bin", 0, 6, b"abc").unwrap();
        assert_eq!(at, "src/data.bin");
        assert!(!dir.path().join("src/data.bin").exists());
        // A part out of order is refused, and the next one goes on.
        assert!(write(&root, &at, 4, 6, b"ef").is_err());
        assert_eq!(write(&root, &at, 3, 6, b"def").unwrap(), at);
        assert_eq!(
            fs::read(dir.path().join("src/data.bin")).unwrap(),
            b"abcdef"
        );
        assert!(!dir.path().join("src/.data.bin.beep-part").exists());
        // No `.gitignore` outside `.beep`.
        assert!(!dir.path().join("src/.gitignore").exists());
    }

    #[test]
    fn refuses_uploads_too_large_or_outside() {
        let dir = TempDir::new().unwrap();
        let root = root_in(&dir);
        let big = write(&root, "big.bin", 0, MAX_WORK_UPLOAD + 1, b"x").unwrap_err();
        assert!(big.to_string().contains("MB"), "{big:#}");
        assert!(write(&root, "short.bin", 0, 1, b"xy").is_err());
        let out = write(&root, "../escape.txt", 0, 1, b"x").unwrap_err();
        assert!(out.to_string().contains("outside"), "{out:#}");
        assert!(write(&root, "", 0, 1, b"x").is_err());
    }
}
