//! How the project's own `.git` travels. Only what `kind` names does, and
//! each kind by its own rule, because plain file sync would break a repository:
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
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};

use super::scan::{Found, Stat};

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

impl Oid {
    pub(super) fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub(super) fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    fn parse(hex: &str) -> Option<Self> {
        if !(hex.len() == 40 || hex.len() == 64) || !is_hex(hex) {
            return None;
        }
        let mut bytes = [0; 32];
        for (at, byte) in bytes.iter_mut().take(hex.len() / 2).enumerate() {
            *byte = u8::from_str_radix(hex.get(at * 2..at * 2 + 2)?, 16).ok()?;
        }
        Some(Self(bytes))
    }

    pub(super) fn hex(&self) -> String {
        let used = if self.0[20..].iter().all(|byte| *byte == 0) {
            20
        } else {
            32
        };
        let mut hex = String::with_capacity(used * 2);
        for byte in &self.0[..used] {
            // Writing to a string cannot fail.
            let _ = write!(hex, "{byte:02x}");
        }
        hex
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

impl Repo {
    /// The repository in `root`, once its `.git` has a `HEAD`.
    pub(super) fn open(root: &Path) -> Option<Self> {
        root.join(GIT).join("HEAD").is_file().then(|| Self {
            root: root.to_owned(),
        })
    }

    /// `git` on exactly this repository, however the node was started: never
    /// one found above a `.git` that is not complete yet. Hooks stay off, as
    /// this is sync moving what the user's git already did elsewhere.
    fn git(&self) -> Command {
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

    fn run(&self, args: &[&str]) -> Result<Output> {
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
            let Some(oid) = Oid::parse(oid) else {
                continue;
            };
            let path = format!("{PREFIX}{name}");
            if !is_ref(&path) {
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
        Oid::parse(String::from_utf8_lossy(&output.stdout).trim())
    }

    /// The ref HEAD names, unless it is detached.
    fn head_ref(&self) -> Option<String> {
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
    fn is_complete(&self, oid: Oid) -> bool {
        let hex = oid.hex();
        self.run(&["rev-list", "--objects", "--quiet", &hex, "--not", "--all"])
            .is_ok()
    }

    /// Whether `a` is an ancestor of `b`; `None` when git cannot say, as when
    /// one of them has not come yet.
    pub(super) fn is_ancestor(&self, a: Oid, b: Oid) -> Option<bool> {
        let status = self
            .git()
            .args(["merge-base", "--is-ancestor", &a.hex(), &b.hex()])
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
        let old = old.map(|old| old.hex()).unwrap_or_default();
        self.run(&[
            "update-ref",
            "--no-deref",
            "--create-reflog",
            "-m",
            "beep-sync: from another machine",
            name,
            &new.hex(),
            &old,
        ])?;
        Ok(())
    }

    /// Removes ref `name` if it is still `old`.
    pub(super) fn delete_ref(&self, name: &str, old: Oid) -> Result<()> {
        self.run(&["update-ref", "--no-deref", "-d", name, &old.hex()])?;
        Ok(())
    }

    pub(super) fn has_no_index(&self) -> bool {
        !self.root.join(GIT).join("index").exists()
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
mod tests {
    use super::*;

    #[test]
    fn only_what_means_the_same_everywhere_travels() {
        for (path, want) in [
            (".git/HEAD", Some(Kind::Head)),
            (".git/shallow", Some(Kind::Shared)),
            (".git/config", Some(Kind::Once)),
            (".git/info/exclude", Some(Kind::Once)),
            (".git/refs/heads/a/b", Some(Kind::Ref)),
            (".git/refs/stash", Some(Kind::Ref)),
            (
                ".git/objects/ab/0123456789012345678901234567890123456f",
                Some(Kind::Object),
            ),
            (".git/objects/pack/pack-0a1b.pack", Some(Kind::Object)),
            (".git/objects/pack/pack-0a1b.idx", Some(Kind::Object)),
            // Temporary, a lock, or a cache that names this machine's packs.
            (".git/objects/ab/tmp_obj_x", None),
            (".git/objects/pack/tmp_pack_x", None),
            (".git/objects/pack/.tmp-1-pack-0a1b.pack", None),
            (".git/objects/pack/pack-0a1b.keep", None),
            (".git/objects/pack/pack-0a1b.bitmap", None),
            (".git/objects/pack/multi-pack-index", None),
            (".git/objects/info/commit-graph", None),
            (".git/objects/info/alternates", None),
            (".git/refs/heads/main.lock", None),
            (".git/refs/heads/a.lock/b", None),
            (".git/refs/bisect/bad", None),
            (".git/index", None),
            (".git/index.lock", None),
            (".git/packed-refs", None),
            (".git/FETCH_HEAD", None),
            (".git/ORIG_HEAD", None),
            (".git/MERGE_HEAD", None),
            (".git/logs/HEAD", None),
            (".git/worktrees/x/HEAD", None),
            (".git/modules/x/HEAD", None),
            (".git/hooks/pre-commit", None),
            ("HEAD", None),
        ] {
            assert_eq!(kind(path), want, "{path}");
        }
    }

    #[test]
    fn object_names_of_either_length_read_back() {
        for hex in ["0a".repeat(20), "0a".repeat(32)] {
            assert_eq!(Oid::parse(&hex).unwrap().hex(), hex);
        }
        assert_eq!(Oid::parse("0a"), None);
        assert_eq!(Oid::parse(&"zz".repeat(20)), None);
    }

    #[test]
    fn a_conflict_ref_is_named_from_its_branch_and_back() {
        let copy = conflict_ref(".git/refs/heads/a/b", "01020304").unwrap();
        assert_eq!(copy, ".git/refs/beep-conflicts/heads/a/b-01020304");
        assert!(is_ref(&copy));
        assert_eq!(original_ref(&copy).as_deref(), Some(".git/refs/heads/a/b"));
        assert_eq!(conflict_ref(".git/HEAD", "01020304"), None);
        assert_eq!(original_ref(".git/refs/heads/a-01020304"), None);
    }
}
