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

use super::scan::{Found, Stat};
use crate::files::paths;

mod branches;
mod recovery;
mod seed;

pub(super) use branches::BranchTip;

pub(super) const GIT: &str = ".git";
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

/// An object name. SHA-1 names fill 20 of the bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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

impl FromStr for Oid {
    type Err = anyhow::Error;

    fn from_str(hex: &str) -> Result<Self> {
        if !(hex.len() == 40 || hex.len() == 64) || !is_hex(hex) {
            bail!("invalid object id");
        }

        let mut bytes = [0; 32];

        for (at, byte) in bytes.iter_mut().take(hex.len() / 2).enumerate() {
            *byte = u8::from_str_radix(&hex[at * 2..at * 2 + 2], 16)?;
        }

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

        for byte in &self.0[..used] {
            write!(f, "{byte:02x}")?;
        }

        Ok(())
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
        let mut command = Command::new("git");
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
        let output = self
            .git()
            .args(args)
            .output()
            .context("running git, which a git repository needs to sync")?;
        if !output.status.success() {
            bail!(
                "git {} failed: {}",
                args.first().copied().unwrap_or_default(),
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }

        Ok(output)
    }

    /// Every ref that travels, as the list names them.
    pub(super) fn refs(&self) -> Result<Vec<RefNow>> {
        let format = "--format=%(objectname)%00%(creatordate:unix)%00%(symref)%00%(refname)";
        let output = self.run(&["for-each-ref", format])?;
        let mut refs = Vec::new();

        for line in output.stdout.split(|&b| b == b'\n') {
            let mut fields = line.split(|&b| b == 0).map(std::str::from_utf8);
            let (Some(Ok(oid)), Some(Ok(when)), Some(Ok("")), Some(Ok(name))) =
                (fields.next(), fields.next(), fields.next(), fields.next())
            else {
                continue;
            };
            let Some(oid) = oid.parse::<Oid>().ok() else {
                continue;
            };
            let path = format!("{PREFIX}{name}");

            if !is_ref(&path) || name == "refs/stash" || name.starts_with("refs/beep-recovery/") {
                continue;
            }

            let secs: i64 = when.parse().unwrap_or(0);
            refs.push(RefNow {
                found: Found {
                    path,
                    abs: self.root.clone(),
                    stat: ref_stat(secs.saturating_mul(1000)),
                },
                oid,
            });
        }

        Ok(refs)
    }

    /// The commit HEAD is at, if it is at one.
    pub(super) fn head(&self) -> Option<Oid> {
        let output = self
            .run(&["rev-parse", "-q", "--verify", "HEAD^{commit}"])
            .ok()?;
        String::from_utf8_lossy(&output.stdout).trim().parse().ok()
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

    /// Whether `a` is an ancestor of `b`; `None` when git cannot say, as when
    /// one of them has not come yet.
    pub(super) fn is_ancestor(&self, a: Oid, b: Oid) -> Option<bool> {
        let status = self
            .git()
            .args([
                "merge-base",
                "--is-ancestor",
                &a.to_string(),
                &b.to_string(),
            ])
            .output()
            .ok()?
            .status;
        match status.code() {
            Some(0) => Some(true),
            Some(1) => Some(false),
            _ => None,
        }
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

    /// Creates the committed working tree after bootstrap, without overwriting files.
    pub(super) fn materialize_checkout(&self) -> Result<()> {
        self.run(&["checkout-index", "--all"])?;
        Ok(())
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
mod tests;
