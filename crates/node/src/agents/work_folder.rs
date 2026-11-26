//! A session's working folder for the apps' Files tab (`WorkRoot`): `list`,
//! `tree`, `read` a part at a time, `hashes` of files on show and `write`
//! dropped files. The Mac app reads a synced project's copy through
//! `local_tree`, `local_read` and `local_hashes`.

mod actions;
pub(super) use actions::edit;

use super::media_file::Chunk;
use crate::files::paths;
use anyhow::{Context, Result, bail};
use beep_proto::{
    AGENT_FILE_CHUNK, AgentFileChunk, GitChange, MAX_TREE_DEPTH, MAX_TREE_ENTRIES,
    MAX_WORK_ENTRIES, WorkEntry, WorkFolder, WorkTree, media_mime,
};
use ignore::WalkBuilder;
use std::{
    sync::Arc,
    collections::{HashMap, HashSet, VecDeque},
    fs::{self, OpenOptions},
    io::{ErrorKind, Read, Seek, SeekFrom, Write},
    path::{Component, Path, PathBuf},
    process::Stdio,
    sync::{LazyLock, Mutex},
    time::{Duration, SystemTime},
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
        let root_dir = paths::canonicalize(real)
            .with_context(|| format!("the folder {shown} is not there"))?;
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
    pub(super) fn find(&self, rel: &Path) -> Result<Option<PathBuf>> {
        let full = match paths::canonicalize(self.root.join(rel)) {
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
    pub(super) fn make_folder(&self, rel: &Path) -> Result<PathBuf> {
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

            let real = paths::canonicalize(&next)
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
/// `ignored`, `.git` always. `kept` is git's status of the whole root when
/// the caller has it, so no git runs for what it covers. Blocks on the file
/// system, and on git without `kept`.
pub(super) fn list(
    root: &WorkRoot,
    path: &str,
    ignored: bool,
    kept: Option<&GitStatus>,
) -> Result<WorkFolder> {
    let dir = open_folder(root, path)?;
    let status = status_in(root, &dir, kept);
    let changes = status.as_ref().map(|s| changes_in(&s.out, &s.prefix));
    read_folder(&dir, root.relative(&dir), ignored, changes.as_ref())
}

/// The folder `path` inside `root` and those under it, `depth` levels in
/// all, each as `list` gives it. Folders git ignores and symlinks are not
/// read into, and it stops past `MAX_TREE_ENTRIES` entries. Blocks on the
/// file system, and on git without `kept`.
pub(super) fn tree(
    root: &WorkRoot,
    path: &str,
    ignored: bool,
    depth: u32,
    kept: Option<&GitStatus>,
) -> Result<WorkTree> {
    let top = open_folder(root, path)?;
    let status = status_in(root, &top, kept);
    let depth = depth.clamp(1, MAX_TREE_DEPTH);
    let top_rel = root.relative(&top);
    // Each folder by its path under `top`, and how many levels down it is.
    let mut queue = VecDeque::from([(String::new(), 1)]);
    let mut folders: Vec<WorkFolder> = Vec::new();
    let mut entries = 0;
    let mut truncated = false;
    let mut fresh: Option<Option<GitStatus>> = None;

    while let Some((sub, level)) = queue.pop_front() {
        let dir = top.join(&sub);
        // Every folder reads the top's status, as ever. A kept one may be
        // stale under a folder the watch skips: those read git once, fresh.
        let skipped = !sub.is_empty() && !crate::events::paths::reports(&top, &dir);
        let status = match (&status, kept) {
            (Some(had), Some(kept)) if skipped && Arc::ptr_eq(&had.out, &kept.out) => {
                fresh.get_or_insert_with(|| git_status(&top)).as_ref()
            }
            _ => status.as_ref(),
        };
        let changes = status.map(|s| changes_in(&s.out, &format!("{}{}", s.prefix, slashed(&sub))));
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
pub(super) fn open_folder(root: &WorkRoot, path: &str) -> Result<PathBuf> {
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
#[derive(Clone)]
pub(super) struct GitStatus {
    pub(super) out: Arc<str>,
    prefix: String,
}

/// What git sees changed under `dir`, read from `kept`, the status of the
/// whole `root`, when `dir` lies in the same repository: a folder with its
/// own `.git` (a nested repository, a submodule or a linked worktree) is
/// asked in itself, as git's status at the root only names it. So is one a
/// rename crosses the edge of, see `straddles`, and one under a folder the
/// checkout's watch skips (`target`, `node_modules`, ...), where a change
/// would not drop the kept status.
///
/// Gaps that remain: a kept status does not hear a change to a `.gitignore`
/// above a root that is a subfolder, to the global excludes file, or to a
/// submodule's own metadata, and is read again after `KEPT_FOR` at the latest.
fn status_in(root: &WorkRoot, dir: &Path, kept: Option<&GitStatus>) -> Option<GitStatus> {
    let own = dir
        .ancestors()
        .take_while(|folder| *folder != root.root)
        .any(|folder| fs::symlink_metadata(folder.join(".git")).is_ok());

    let prefix = |kept: &GitStatus| format!("{}{}", kept.prefix, slashed(&root.relative(dir)));

    let watched = dir == root.root || crate::events::paths::reports(&root.root, dir);

    match kept {
        Some(kept) if watched && !own && !straddles(&kept.out, &prefix(kept)) => Some(GitStatus {
            out: kept.out.clone(),
            prefix: prefix(kept),
        }),
        _ => git_status(dir),
    }
}

/// `None` outside a checkout or without git, where there is nothing to
/// mark.
pub(super) fn git_status(dir: &Path) -> Option<GitStatus> {
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
        out: out.into(),
        prefix: prefix.trim_end_matches('\n').to_owned(),
    })
}

/// What `git <args>` printed in `dir`, when it succeeded. Takes no locks,
/// so a running agent's own git commands are never in its way.
pub(crate) fn git(dir: &Path, args: &[&str]) -> Option<String> {
    let out = crate::process::background_command("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output();
    crate::process::finished("git", &out);
    let out = out.ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// The entries of `git status --porcelain=v1 -z` output: each one's code and
/// path from the checkout's top, and for a rename or copy where it came from.
fn records(status: &str) -> impl Iterator<Item = (&str, &str, Option<&str>)> {
    let mut fields = status.split('\0');

    std::iter::from_fn(move || {
        loop {
            let field = fields.next()?;
            let (Some(code), Some(path)) = (field.get(..2), field.get(3..)) else {
                continue;
            };
            let from = code
                .contains(['R', 'C'])
                .then(|| fields.next())
                .flatten();

            return Some((code, path, from));
        }
    })
}

/// `git status --porcelain=v1 -z` output, its paths from the checkout's
/// top, read for the folder `prefix` (`src/`, or empty for the top): each
/// name in it with the strongest change under it.
fn changes_in(status: &str, prefix: &str) -> HashMap<String, GitChange> {
    let mut out: HashMap<String, GitChange> = HashMap::new();

    for (code, path, _) in records(status) {
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

/// Whether a rename or copy in `status` has one end under `prefix` and the
/// other outside it. Git's status in the folder alone sees that as an added
/// or deleted file, which the status of the whole checkout does not say.
fn straddles(status: &str, prefix: &str) -> bool {
    records(status).any(|(_, path, from)| {
        from.is_some_and(|from| path.starts_with(prefix) != from.starts_with(prefix))
    })
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

/// The BLAKE3 hash of each of `paths` inside `root`, found as `read` finds
/// them, in order: `None` for one that is not there or is not a file. A file
/// whose size and modification time are as they were is not read again.
/// Blocks on the file system.
pub(super) fn hashes(root: &WorkRoot, paths: &[String]) -> Vec<Option<[u8; 32]>> {
    paths
        .iter()
        .map(|path| match hash(root, path) {
            Ok(hash) => hash,
            Err(e) => {
                log::debug!("could not hash {path}: {e:#}");
                None
            }
        })
        .collect()
}

/// The hashes of `paths` inside the folder `root` on this machine, as
/// `hashes` gives them. Blocks on the file system.
pub fn local_hashes(root: &Path, paths: &[String]) -> Result<Vec<Option<[u8; 32]>>> {
    Ok(hashes(&local_root(root)?, paths))
}

/// Files hashed lately, by their full path, with the size and time they had.
static HASHED: LazyLock<Mutex<HashMap<PathBuf, Hashed>>> = LazyLock::new(Mutex::default);
/// How many files `HASHED` keeps before it starts again.
const MAX_HASHED: usize = 256;

#[derive(Clone, Copy, PartialEq)]
struct Hashed {
    size: u64,
    modified: SystemTime,
    hash: [u8; 32],
}

fn hash(root: &WorkRoot, path: &str) -> Result<Option<[u8; 32]>> {
    let Some(full) = root.find(&names(path)?)? else {
        return Ok(None);
    };
    let mut file = crate::files::fsat::open_read(&full)?;
    let meta = file.metadata()?;
    if !meta.is_file() {
        return Ok(None);
    }

    let (size, modified) = (meta.len(), meta.modified().ok());
    let kept = crate::lock(&HASHED).get(&full).copied();

    if let (Some(kept), Some(modified)) = (kept, modified)
        && kept.size == size
        && kept.modified == modified
    {
        return Ok(Some(kept.hash));
    }

    let mut hasher = blake3::Hasher::new();
    hasher.update_reader(&mut file)?;
    let hash = *hasher.finalize().as_bytes();

    // A file whose time cannot be read is hashed each time.
    if let Some(modified) = modified {
        let mut held = crate::lock(&HASHED);
        if held.len() >= MAX_HASHED {
            held.clear();
        }
        held.insert(
            full,
            Hashed {
                size,
                modified,
                hash,
            },
        );
    }

    Ok(Some(hash))
}

/// The folder `path` inside the folder `root` on this machine and those
/// under it, as `tree` reads them. Blocks on the file system and git.
pub fn local_tree(root: &Path, path: &str, ignored: bool, depth: u32) -> Result<WorkTree> {
    tree(&local_root(root)?, path, ignored, depth, None)
}

/// Part of the file `path` inside the folder `root` on this machine, as
/// `read` gives it. Blocks on the file system.
pub fn local_read(
    root: &Path,
    path: &str,
    offset: u64,
    len: u32,
) -> Result<Option<AgentFileChunk>> {
    let chunk = read(&local_root(root)?, path, offset, len)?;
    Ok(chunk.map(|chunk| AgentFileChunk {
        size: chunk.size,
        mime: chunk.mime.to_owned(),
        data: chunk.data,
    }))
}

fn local_root(root: &Path) -> Result<WorkRoot> {
    let real = paths::canonicalize(root)
        .with_context(|| format!("the folder {} is not there", root.display()))?;
    WorkRoot::at(&real)
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

        if folder == uploads(&root.root)
            && let Err(e) = prune(&folder, SystemTime::now())
        {
            log::warn!("could not clean old uploads: {e:#}");
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

/// An upload nobody read or wrote for this long goes, whatever its session.
pub(super) const UNUSED: Duration = Duration::from_secs(3 * 24 * 60 * 60);
/// Removes files in a project's `.beep/uploads` unused for three days.
pub(super) fn prune(uploads: &Path, now: SystemTime) -> Result<()> {
    let entries = match fs::read_dir(uploads) {
        Ok(entries) => entries,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.into()),
    };

    for entry in entries {
        let entry = entry?;

        if entry.file_type()?.is_file() && idle(now, &entry.metadata()?) >= UNUSED {
            fs::remove_file(entry.path())?;
        }
    }

    Ok(())
}

/// How long since a file was last read or written.
pub(super) fn idle(now: SystemTime, meta: &fs::Metadata) -> Duration {
    let modified = meta.modified().ok();
    let used = meta.accessed().ok().max(modified).unwrap_or(now);

    now.duration_since(used).unwrap_or_default()
}

/// Where files dropped on a new agent's prompt go in its folder.
pub(super) fn uploads(root: &Path) -> PathBuf {
    root.join(BEEP_FOLDER).join("uploads")
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
#[path = "../tests/agents_work_folder.rs"]
mod tests;
