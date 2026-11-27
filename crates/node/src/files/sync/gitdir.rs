//! The initial copy of a project's `.git`. After it finishes, Git stays
//! local and branches move through normal Git operations. Only what `kind`
//! names is copied, with each kind handled separately:
//!
//! - Objects are content-addressed and only ever add up. A peer's delete (its
//!   `git gc`) does not touch them here; this machine's own gc does.
//! - Refs are not files: they are read from git, packed or not, and written
//!   through git. Their value travels in the list itself.
//! - `HEAD` follows the working tree, the last writer winning.
//! - `config`, `description` and `info/*` are written where a machine has none
//!   and then belong to it: they hold machine settings (`core.ignorecase`).
//! - Everything else stays: locks and temporary files, `FETCH_HEAD` and the
//!   like, reflogs, `worktrees/`, hooks (as with `git clone`), and the index,
//!   whose stat data differs per machine. Each machine rebuilds its own.
//!
//! Everything here blocks.

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    fmt, fs,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    str::FromStr,
};

use super::disk::{Found, Stat};
use crate::fs::paths;

mod branches;
mod recovery;
mod seed;

pub(super) use branches::BranchTip;

pub(super) const GIT: &str = ".git";
/// The start of the name of a folder that holds a pack while it arrives.
const INCOMING: &str = "beep-incoming-";
const STALE_INCOMING: std::time::Duration = std::time::Duration::from_secs(24 * 60 * 60);
const PREFIX: &str = ".git/";
/// Where a ref's losing side is kept. `git log --all` shows it, `git gc` keeps
/// it, and `git branch` does not list it as a branch.
const CONFLICTS: &str = ".git/refs/beep-conflicts/";

/// What a path in `.git` is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    Object,
    Ref,
    Head,
    /// Replaced like any file, but never given a conflict copy.
    Shared,
    /// Written where this machine has none, and its own from then on.
    Once,
}

fn is_hex(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|b| b.is_ascii_hexdigit())
}

/// What `path` in the list is in `.git`; `None` for what never travels.
pub(super) fn kind(path: &str) -> Option<Kind> {
    let rest = path.strip_prefix(PREFIX)?;

    match rest {
        "HEAD" => return Some(Kind::Head),
        "shallow" => return Some(Kind::Shared),
        "config" | "description" | "info/exclude" | "info/attributes" => return Some(Kind::Once),
        _ => {}
    }

    if let Some(name) = rest.strip_prefix("refs/") {
        return is_ref_name(name).then_some(Kind::Ref);
    }

    let object = rest.strip_prefix("objects/")?;

    if let Some(pack) = object.strip_prefix("pack/") {
        let (stem, ext) = pack.rsplit_once('.')?;
        let is_pack = is_hex(stem.strip_prefix("pack-")?)
            && matches!(ext, "pack" | "idx" | "rev" | "mtimes" | "promisor");
        return is_pack.then_some(Kind::Object);
    }

    let (folder, name) = object.split_once('/')?;
    (folder.len() == 2 && is_hex(folder) && name.len() >= 38 && is_hex(name))
        .then_some(Kind::Object)
}

/// A ref that means the same on every machine: not git's own per-worktree
/// ones, and not a lock.
fn is_ref_name(name: &str) -> bool {
    !name.is_empty()
        && !["bisect/", "worktree/", "rewritten/"]
            .iter()
            .any(|own| name.starts_with(own))
        && name.split('/').all(|part| !part.ends_with(".lock"))
}

/// Whether a walk goes into folder `path`, `.git` or inside it.
pub(super) fn walks(path: &str) -> bool {
    let Some(rest) = path.strip_prefix(PREFIX) else {
        return path == GIT;
    };

    if rest == "info" || rest == "objects" || rest == "refs" {
        return true;
    }

    if let Some(name) = rest.strip_prefix("refs/") {
        return is_ref_name(name);
    }

    rest.strip_prefix("objects/")
        .is_some_and(|folder| folder == "pack" || (folder.len() == 2 && is_hex(folder)))
}

/// Whether `path` is the repository's config, which is rewritten on the way in.
pub(super) fn is_config(path: &str) -> bool {
    path == ".git/config"
}

pub(super) fn in_git(path: &str) -> bool {
    path.starts_with(PREFIX)
}

pub(super) fn is_ref(path: &str) -> bool {
    kind(path) == Some(Kind::Ref)
}

/// Whether a change under `path` is left for a later scan. Objects come before
/// the ref that names them, and `git gc` rewrites packs in bursts.
pub(super) fn is_quiet(path: &str) -> bool {
    path.replace('\\', "/").starts_with(".git/objects")
}

/// Whether a change at `path` means a ref moved, though the file never syncs.
pub(super) fn wakes(path: &Path) -> bool {
    path == Path::new(".git/packed-refs")
}

/// Whether `path` is Git metadata a Git read depends on: the branch, index,
/// refs, configuration, worktrees or exclusions, not objects or logs.
pub(super) fn is_metadata(path: &Path) -> bool {
    let mut parts = path.components().map(|part| part.as_os_str());
    parts.next() == Some(GIT.as_ref())
        && parts.next().is_some_and(|name| {
            [
                "HEAD",
                "index",
                "refs",
                "packed-refs",
                "config",
                "worktrees",
                "info",
                "commondir",
            ]
            .iter()
            .any(|known| name == *known)
        })
}

/// Whether a peer's delete leaves `path` here.
pub(super) fn is_kept_from_deletes(path: &str) -> bool {
    in_git(path) && !is_ref(path)
}

/// Whether what this machine has at `path` stays whatever the list says.
pub(super) fn is_written_once(path: &str) -> bool {
    matches!(kind(path), Some(Kind::Once | Kind::Object))
}

/// The ref that keeps the losing side of two commits to `path`, tagged with
/// its commit; `None` for what is no ref, whose loser is not kept.
pub(super) fn conflict_ref(path: &str, tag: &str) -> Option<String> {
    let name = is_ref(path).then(|| path.strip_prefix(".git/refs/"))??;
    let name = name.strip_prefix("beep-conflicts/").unwrap_or(name);
    Some(format!("{CONFLICTS}{name}-{tag}"))
}

/// The ref a `conflict_ref` kept the loser of.
pub(super) fn original_ref(path: &str) -> Option<String> {
    let name = path.strip_prefix(CONFLICTS)?;
    let (name, tag) = name.rsplit_once('-')?;
    (tag.len() == 8 && is_hex(tag)).then(|| format!(".git/refs/{name}"))
}

/// The name git knows the ref at `path` by.
pub(super) fn ref_name(path: &str) -> &str {
    path.strip_prefix(PREFIX).unwrap_or(path)
}

/// When the object `id` was made, in seconds, as `for-each-ref`'s
/// `creatordate` gives it: a commit's committer time, a tag's tagger time,
/// and 0 for anything else.
fn created(repo: &gix::Repository, id: &gix::oid) -> i64 {
    let Ok(object) = repo.find_object(id) else {
        return 0;
    };

    let seconds = match object.kind {
        gix::object::Kind::Commit => object.into_commit().committer().ok().map(|s| s.seconds()),
        gix::object::Kind::Tag => object
            .into_tag()
            .tagger()
            .ok()
            .flatten()
            .map(|s| s.seconds()),
        _ => None,
    };
    seconds.unwrap_or(0)
}

/// An object name. SHA-1 names fill 20 of the bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(super) struct Oid(#[serde(with = "serde_bytes")] [u8; 32]);

impl From<[u8; 32]> for Oid {
    fn from(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
}

impl AsRef<[u8; 32]> for Oid {
    fn as_ref(&self) -> &[u8; 32] {
        &self.0
    }
}

impl From<&gix::oid> for Oid {
    fn from(id: &gix::oid) -> Self {
        let mut bytes = [0; 32];
        bytes[..id.as_bytes().len()].copy_from_slice(id.as_bytes());
        Self(bytes)
    }
}

impl TryFrom<Oid> for gix::ObjectId {
    type Error = anyhow::Error;

    fn try_from(oid: Oid) -> Result<Self> {
        Ok(oid.to_string().parse()?)
    }
}

impl FromStr for Oid {
    type Err = anyhow::Error;

    fn from_str(text: &str) -> Result<Self> {
        if !(text.len() == 40 || text.len() == 64) || !is_hex(text) {
            bail!("invalid object id");
        }

        let mut bytes = [0; 32];

        hex::decode_to_slice(text, &mut bytes[..text.len() / 2])?;

        Ok(Self(bytes))
    }
}

impl fmt::Display for Oid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let used = if self.0[20..].iter().all(|byte| *byte == 0) {
            20
        } else {
            32
        };

        f.write_str(&hex::encode(&self.0[..used]))
    }
}

/// The checkout advertised alongside a file list. An unborn repository has
/// no commit, while a folder without Git has no repository at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Checkout {
    pub(super) commit: Option<Oid>,
    pub(super) present: bool,
}

impl Checkout {
    pub(super) fn read(root: &Path) -> Self {
        Self {
            commit: Repo::open(root).and_then(|repo| repo.head()),
            present: root.join(GIT).is_dir(),
        }
    }

    pub(super) fn differs(self, other: Self) -> bool {
        self.present && other.present && self.commit != other.commit
    }
}

/// Holds Git's index lock only while files enter the working tree. Git
/// checkout, merge and commit cannot update the index during that batch.
pub(super) struct WriteGuard(Vec<PathBuf>);

impl WriteGuard {
    pub(super) fn acquire(root: &Path, expected: Option<Checkout>) -> Result<Self> {
        let mut guard = Self(Vec::new());
        let Some(expected) = expected else {
            return Ok(guard);
        };

        if is_busy(root) {
            bail!("waiting for the Git operation running on this machine");
        }

        if expected.present {
            let lock = root.join(GIT).join("index.lock");
            fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&lock)
                .context("waiting for Git's index lock")?;
            guard.0.push(lock);
        }

        if Checkout::read(root) != expected {
            bail!("the Git checkout changed before this batch could be applied");
        }

        Ok(guard)
    }

    /// Keep checkouts in the main and existing linked worktrees out of a branch update.
    pub(super) fn for_branches(root: &Path) -> Result<Self> {
        let mut guard = Self::acquire(root, Some(Checkout::read(root)))?;
        let linked = match fs::read_dir(root.join(GIT).join("worktrees")) {
            Ok(linked) => linked,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(guard),
            Err(e) => return Err(e).context("reading linked worktrees before a Git update"),
        };

        for entry in linked {
            let lock = entry?.path().join("index.lock");
            fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&lock)
                .context("waiting for a linked worktree's Git index lock")?;
            guard.0.push(lock);
        }

        Ok(guard)
    }
}

impl Drop for WriteGuard {
    fn drop(&mut self) {
        for lock in &self.0 {
            let _ = fs::remove_file(lock);
        }
    }
}

/// What the list's file record says for a ref, which has no file: only the
/// time, the commit's, by which concurrent edits are ranked.
pub(super) fn ref_stat(mtime_ms: i64) -> Stat {
    Stat {
        size: 0,
        mtime_ns: mtime_ms.saturating_mul(1_000_000),
        ctime_ns: 0,
        inode: 0,
        device: 0,
        executable: false,
    }
}

/// One ref as git has it now.
#[derive(Debug)]
pub(super) struct RefNow {
    pub(super) found: Found,
    pub(super) oid: Oid,
}

/// A git operation is under way here, and its index and refs are its own.
pub(super) fn is_busy(root: &Path) -> bool {
    let git = root.join(GIT);
    [
        "index.lock",
        "HEAD.lock",
        "packed-refs.lock",
        "MERGE_HEAD",
        "CHERRY_PICK_HEAD",
        "REVERT_HEAD",
        "BISECT_LOG",
        "rebase-merge",
        "rebase-apply",
    ]
    .iter()
    .any(|name| git.join(name).exists())
}

/// A blocked operation may need conflict resolution rather than more waiting.
/// Called on the blocking pool only, after `is_busy` has detected an operation.
pub(super) fn busy_reason(root: &Path) -> &'static str {
    let unmerged = Repo::open(root)
        .and_then(|repo| repo.run(&["diff", "--name-only", "--diff-filter=U"]).ok())
        .is_some_and(|out| !out.stdout.is_empty());
    if unmerged {
        "Git has unmerged conflicts; resolve them to resume sync"
    } else {
        "waiting for the Git operation running on this machine"
    }
}

/// A ref to bring in line.
#[derive(Debug)]
pub(super) struct RefMove {
    pub(super) path: String,
    /// `None` to remove it.
    pub(super) new: Option<Oid>,
    /// What the last scan saw.
    pub(super) old: Option<Oid>,
    /// `old` is kept as a conflict ref: do not go back from it to its ancestor.
    pub(super) stays_ahead: bool,
}

/// What a ref's move touched.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Stay {
    HeadMoved,
    HeadStayed,
}

/// Brings one ref in line; `None` when it stays where it is for now.
pub(super) fn move_ref(root: &Path, change: &RefMove) -> Result<Option<Stay>> {
    if is_busy(root) {
        bail!("waiting for the git operation running on this machine");
    }

    let name = ref_name(&change.path);
    // A folder with no `refs` is not a repository to git yet.
    for folder in ["refs", "objects"] {
        fs::create_dir_all(root.join(GIT).join(folder))
            .with_context(|| format!("making .git/{folder}"))?;
    }

    let repo = Repo::open(root).context("HEAD has not come yet")?;

    if repo.is_linked_to(name) {
        bail!("{name} is checked out in a linked worktree on this machine");
    }

    let is_head = repo.head_ref().as_deref() == Some(name);

    match change.new {
        Some(new) => {
            if change.stays_ahead
                && let Some(old) = change.old
                && repo.is_ancestor(new, old) == Some(true)
            {
                return Ok(None);
            }

            if !repo.is_complete(new) {
                bail!("waiting for the objects of {name}");
            }

            repo.set_ref(name, new, change.old)?;
        }
        None => {
            if is_head {
                bail!("{name} is what HEAD names on this machine");
            }

            if let Some(old) = change.old {
                repo.delete_ref(name, old)?;
            }
        }
    }

    Ok(Some(if is_head {
        Stay::HeadMoved
    } else {
        Stay::HeadStayed
    }))
}

/// A pack on its way in: the bytes as they come, then Git's index of them,
/// in one folder that is removed with it.
pub(super) struct IncomingPack {
    dir: tempfile::TempDir,
}

impl IncomingPack {
    /// Where the bytes go as they arrive.
    pub(super) fn received(&self) -> PathBuf {
        self.dir.path().join("received.pack")
    }
}

/// The repository in a project's root.
#[derive(Debug)]
pub(super) struct Repo {
    root: PathBuf,
}

#[derive(Default)]
pub(super) struct CheckoutPaths {
    pub(super) clean: HashSet<String>,
    pub(super) changed: HashSet<String>,
}

impl Repo {
    /// Tracked paths that match HEAD in both the index and working tree.
    pub(super) fn checkout_paths(&self) -> Result<CheckoutPaths> {
        let dirty = self.run(&[
            "diff",
            "HEAD",
            "--name-only",
            "-z",
            "--ignore-submodules=none",
        ])?;
        let changed: HashSet<_> = dirty
            .stdout
            .split(|b| *b == 0)
            .filter(|path| !path.is_empty())
            .map(|path| String::from_utf8(path.to_vec()))
            .collect::<std::result::Result<_, _>>()?;
        let clean = self
            .tracked()?
            .into_iter()
            .filter(|path| !changed.contains(path))
            .collect();
        Ok(CheckoutPaths { clean, changed })
    }

    pub(super) fn require_clean(&self) -> Result<()> {
        if is_busy(&self.root) {
            bail!("{}", busy_reason(&self.root));
        }

        if self.has_local_changes()? {
            bail!(
                "Git commits differ; commit or stash staged, unstaged and untracked changes before syncing"
            );
        }

        Ok(())
    }

    pub(super) fn has_local_changes(&self) -> Result<bool> {
        let status = self.run(&[
            "status",
            "--porcelain=v1",
            "--untracked-files=all",
            "--ignore-submodules=none",
        ])?;
        Ok(!status.stdout.is_empty())
    }

    /// Moves only to the advertised commit. Never merges divergent histories
    /// or checks out another branch.
    pub(super) fn fast_forward(&self, before: Checkout, target: Oid, branch: &str) -> Result<()> {
        self.require_clean()?;

        if Checkout::read(&self.root) != before || self.head_ref().as_deref() != Some(branch) {
            bail!("the Git checkout changed while fetching; retrying sync");
        }

        if before
            .commit
            .is_none_or(|own| self.is_ancestor(own, target) != Some(true))
        {
            bail!("Git commits differ; histories diverged or this checkout is ahead");
        }

        self.run(&[
            "-c",
            "merge.autostash=false",
            "merge",
            "--ff-only",
            "--no-edit",
            &target.to_string(),
        ])?;
        Ok(())
    }

    /// The repository in `root`, once its `.git` has a `HEAD`. The root is
    /// kept plain: Git for Windows takes no `\\?\` path.
    pub(super) fn open(root: &Path) -> Option<Self> {
        root.join(GIT).join("HEAD").is_file().then(|| Self {
            root: paths::plain(root),
        })
    }

    /// `git` on exactly this repository, however the node was started: never
    /// one found above a `.git` that is not complete yet. Hooks stay off, as
    /// this is sync moving what the user's git already did elsewhere.
    pub(super) fn git(&self) -> Command {
        let mut command = crate::process::background_command("git");
        for name in [
            "GIT_DIR",
            "GIT_WORK_TREE",
            "GIT_INDEX_FILE",
            "GIT_COMMON_DIR",
            "GIT_OBJECT_DIRECTORY",
            "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        ] {
            command.env_remove(name);
        }
        command
            .env("GIT_OPTIONAL_LOCKS", "0")
            .current_dir(&self.root)
            .arg("--git-dir")
            .arg(self.root.join(GIT))
            .arg("--work-tree")
            .arg(&self.root)
            .args([
                "-c",
                "core.hooksPath=/dev/null",
                "-c",
                "core.fsmonitor=false",
            ])
            .stdin(Stdio::null());
        command
    }

    pub(super) fn run(&self, args: &[&str]) -> Result<Output> {
        let output = self.git().args(args).output();
        crate::process::finished("git", &output);
        let output = output.context("running git, which a git repository needs to sync")?;
        if !output.status.success() {
            bail!(
                "git {} failed: {}",
                args.first().copied().unwrap_or_default(),
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }

        Ok(output)
    }

    fn pack_dir(&self) -> PathBuf {
        self.root.join(GIT).join("objects").join("pack")
    }

    /// A folder for a pack while it arrives, so Git reads only a whole one.
    /// A transfer cut short leaves nothing behind. It sits with the packs,
    /// where writes wake no scan and the sync copies nothing.
    pub(super) fn incoming_pack(&self) -> Result<IncomingPack> {
        let packs = self.pack_dir();
        fs::create_dir_all(&packs)?;
        // A node stopped mid-transfer leaves its folder; one a day old is
        // no transfer's any more.
        for entry in fs::read_dir(&packs)?.flatten() {
            let stale = entry.file_name().to_string_lossy().starts_with(INCOMING)
                && entry
                    .metadata()
                    .and_then(|meta| meta.modified())
                    .is_ok_and(|at| at.elapsed().is_ok_and(|age| age > STALE_INCOMING));

            if stale {
                let _ = fs::remove_dir_all(entry.path());
            }
        }

        let dir = tempfile::Builder::new()
            .prefix(INCOMING)
            .tempdir_in(packs)
            .context("making room for a Git pack")?;
        Ok(IncomingPack { dir })
    }

    /// Indexes the whole pack that came into `incoming`, and only then
    /// moves it in with the others. Whatever Git leaves on failure goes
    /// with the folder.
    pub(super) fn index_pack(&self, incoming: &IncomingPack) -> Result<()> {
        let fixed = incoming.dir.path().join("fixed.pack");
        let output = self
            .git()
            .args(["index-pack", "--stdin", "--fix-thin"])
            .arg(&fixed)
            .stdin(fs::File::open(incoming.received())?)
            .output()
            .context("running git index-pack")?;

        if !output.status.success() {
            bail!(
                "Git could not read the object pack: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let name = stdout
            .split_whitespace()
            .last()
            .filter(|name| is_hex(name))
            .context("Git did not name the pack")?;
        let packs = self.pack_dir();

        // Git finds a pack by its index, so the index moves in last.
        for ext in ["pack", "rev", "idx"] {
            let from = fixed.with_extension(ext);

            if from.exists() {
                fs::rename(&from, packs.join(format!("pack-{name}.{ext}")))?;
            }
        }

        Ok(())
    }

    /// Packs objects together once received packs pile up, after their refs
    /// are in place; below `gc.autoPackLimit` packs it returns at once. It
    /// stays in the foreground, so it finishes or fails here.
    pub(super) fn collect(&self) {
        if is_busy(&self.root) {
            return;
        }

        if let Err(e) = self.run(&["-c", "gc.autoDetach=false", "gc", "--auto", "--quiet"]) {
            log::warn!("{}: {e:#}", self.root.display());
        }
    }

    /// This repository for questions about refs and history, answered in
    /// process instead of by a git process. Like [`Self::git`], it ignores the
    /// environment and global config that could point it elsewhere.
    pub(super) fn gix(&self) -> Result<gix::Repository> {
        crate::fs::git_read::repository(&self.root).context("opening the Git repository")
    }

    /// Every ref that travels, as the list names them. The sync asks this on
    /// every scan and peer catalog, so it is read in process.
    pub(super) fn refs(&self) -> Result<Vec<RefNow>> {
        let repo = self.gix()?;
        let mut refs = Vec::new();

        for reference in repo.references()?.all()? {
            let Ok(reference) = reference else {
                continue;
            };
            // A symbolic ref names another ref, which is listed itself.
            let Some(id) = reference.target().try_id().map(ToOwned::to_owned) else {
                continue;
            };
            let Ok(name) = std::str::from_utf8(reference.name().as_bstr()) else {
                continue;
            };
            let path = format!("{PREFIX}{name}");

            if !is_ref(&path) || name == "refs/stash" || name.starts_with("refs/beep-recovery/") {
                continue;
            }

            refs.push(RefNow {
                found: Found {
                    path,
                    abs: self.root.clone(),
                    stat: ref_stat(created(&repo, &id).saturating_mul(1000)),
                },
                oid: Oid::from(id.as_ref()),
            });
        }

        refs.sort_by(|a, b| a.found.path.cmp(&b.found.path));
        Ok(refs)
    }

    /// The commit HEAD is at, if it is at one. Git is asked again only once
    /// `HEAD` or the ref it names changes.
    pub(super) fn head(&self) -> Option<Oid> {
        crate::fs::git_read::head(&self.root, || {
            let id = self.gix().ok()?.head_commit().ok()?.id;
            Some(id.to_string())
        })?
        .parse()
        .ok()
    }

    /// The ref HEAD names, unless it is detached.
    pub(super) fn head_ref(&self) -> Option<String> {
        let head = fs::read_to_string(self.root.join(GIT).join("HEAD")).ok()?;
        Some(head.strip_prefix("ref:")?.trim().to_owned())
    }

    /// Whether a linked worktree here has ref `name` checked out. Git would
    /// move it from under that worktree's index and files.
    fn is_linked_to(&self, name: &str) -> bool {
        let Ok(linked) = fs::read_dir(self.root.join(GIT).join("worktrees")) else {
            return false;
        };
        linked.flatten().any(|worktree| {
            fs::read_to_string(worktree.path().join("HEAD"))
                .is_ok_and(|head| head.strip_prefix("ref:").map(str::trim) == Some(name))
        })
    }

    /// The tracked files, by path relative to the root.
    pub(super) fn tracked(&self) -> Result<Vec<String>> {
        let output = self.run(&["ls-files", "-z"])?;
        Ok(output
            .stdout
            .split(|&b| b == 0)
            .filter_map(|path| std::str::from_utf8(path).ok())
            .filter(|path| !path.is_empty())
            .map(str::to_owned)
            .collect())
    }

    /// Whether `oid` and all it reaches is here, which is what git itself asks
    /// before it lets a pushed ref in.
    pub(super) fn is_complete(&self, oid: Oid) -> bool {
        let hex = oid.to_string();
        self.run(&["rev-list", "--objects", "--quiet", &hex, "--not", "--all"])
            .is_ok()
    }

    /// Whether commit `oid` is here.
    pub(super) fn has_commit(&self, oid: Oid) -> bool {
        let Ok(repo) = self.gix() else {
            return false;
        };
        gix::ObjectId::try_from(oid).is_ok_and(|id| repo.find_commit(id).is_ok())
    }

    /// Whether `a` is an ancestor of `b`; `None` when git cannot say, as when
    /// one of them has not come yet.
    pub(super) fn is_ancestor(&self, a: Oid, b: Oid) -> Option<bool> {
        let repo = self.gix().ok()?;
        let a = gix::ObjectId::try_from(a).ok()?;
        let b = gix::ObjectId::try_from(b).ok()?;

        if !repo.has_object(a) || !repo.has_object(b) {
            return None;
        }

        if a == b {
            return Some(true);
        }

        // An error means part of a history has not come.
        let cache = repo.commit_graph_if_enabled().ok()?;
        let mut graph = repo.revision_graph(cache.as_ref());
        Some(
            gix_revision::merge_base(a, &[b], &mut graph)
                .ok()?
                .is_some_and(|bases| *bases.first() == a),
        )
    }

    /// Sets ref `name` to `new` if it is still `old`.
    pub(super) fn set_ref(&self, name: &str, new: Oid, old: Option<Oid>) -> Result<()> {
        let old = old.map(|old| old.to_string()).unwrap_or_default();
        self.run(&[
            "update-ref",
            "--no-deref",
            "--create-reflog",
            "-m",
            "beep-sync: from another machine",
            name,
            &new.to_string(),
            &old,
        ])?;
        Ok(())
    }

    /// Removes ref `name` if it is still `old`.
    pub(super) fn delete_ref(&self, name: &str, old: Oid) -> Result<()> {
        self.run(&["update-ref", "--no-deref", "-d", name, &old.to_string()])?;
        Ok(())
    }

    /// Empty object and ref directories have no entries in the file list.
    /// Make them once the initial list is complete, including an unborn repo.
    pub(super) fn finish_copy(&self) -> Result<bool> {
        for name in ["objects", "refs"] {
            fs::create_dir_all(self.root.join(GIT).join(name))?;
        }

        self.run(&["rev-parse", "--git-dir"])?;
        Ok(if self.head().is_some() {
            !self.has_no_index()
        } else {
            self.head_ref().is_some() && self.refs()?.is_empty()
        })
    }

    pub(super) fn has_no_index(&self) -> bool {
        !self.root.join(GIT).join("index").exists()
    }

    /// Creates the committed working tree after bootstrap, without overwriting files: one that
    /// is there and differs stays, a local change.
    pub(super) fn materialize_checkout(&self) -> Result<()> {
        // Git fails for each file it leaves alone; only a tracked file still missing is a failure.
        let Err(left) = self.run(&["checkout-index", "--all"]) else {
            return Ok(());
        };

        if self.run(&["ls-files", "--deleted"])?.stdout.is_empty() {
            return Ok(());
        }

        Err(left)
    }

    /// Makes the index HEAD's tree, with this machine's stat data, and puts
    /// tracked links back, which sync does not carry as files. Staged changes
    /// on this machine become unstaged; the files stay as they are.
    pub(super) fn rebuild_index(&self) -> Result<()> {
        self.run(&["read-tree", "HEAD"])?;
        // Says so by its exit code when files differ, which they may.
        let _ = self.run(&["update-index", "-q", "--refresh"]);
        let staged = self.run(&["ls-files", "-s", "-z"])?;

        for line in staged.stdout.split(|&b| b == 0) {
            let Ok(line) = std::str::from_utf8(line) else {
                continue;
            };
            let Some((meta, path)) = line.split_once('\t') else {
                continue;
            };
            let there = self.root.join(path);

            if fs::symlink_metadata(&there).is_ok() {
                continue;
            }

            if meta.starts_with("120000") {
                // A folder left to come; the next rebuild gets it.
                if let Err(e) = self.run(&["checkout-index", "-q", "--", path]) {
                    log::debug!("the link {path} is not back: {e:#}");
                }
            } else if meta.starts_with("160000") {
                // A submodule stays on its machine; git wants its folder.
                fs::create_dir_all(&there)
                    .with_context(|| format!("making the folder of {path}"))?;
            }
        }

        Ok(())
    }
}

#[cfg(test)]
#[path = "../../tests/files_sync_gitdir.rs"]
pub(super) mod tests;
