//! The git side of the diff: runs git in a checkout and reads what it says.

use super::DiffParser;
use crate::files::paths;
use anyhow::{Context, Result, bail};
use beep_proto::{MAX_DIFF_FILES, WorkDiff, node::agent::DiffAsk};
use std::{
    fs,
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
};
use tempfile::TempDir;

/// The most of git's error kept for the message.
const MAX_ERROR: usize = 2_000;
/// The longest branch name taken.
const MAX_REF: usize = 250;
/// What git calls the tree with nothing in it, the base before the first
/// commit.
const EMPTY_TREE: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";

/// The diff `ask` names, in `root`, already resolved and in reach. Git
/// runs with `--relative`, so nothing outside `root` shows. Blocks on git
/// and the file system.
pub(in crate::agents) fn read(root: &Path, ask: &DiffAsk) -> Result<WorkDiff> {
    if !root.is_dir() {
        bail!("{} is not a folder", root.display());
    }

    let inside = git(root, &["rev-parse", "--is-inside-work-tree"])
        .ok()
        .is_some_and(|out| out.trim() == "true");
    if !inside {
        bail!("{} is not in a git checkout", root.display());
    }

    let (dir, head) = match branch_ref(&ask.head)? {
        None => (root.to_owned(), None),
        Some(branch) => match worktree_of(root, branch)? {
            Some(worktree) => (worktree, None),
            None => (root.to_owned(), Some(resolve(root, branch)?)),
        },
    };
    let base = match branch_ref(&ask.base)? {
        Some(base) => Some(resolve(&dir, base)?),
        None => default_base(&dir),
    };
    let tip = head.as_deref().unwrap_or("HEAD");
    let has_head = is_commit(&dir, "HEAD");
    let (label, merge_base) = match (&base, has_head || head.is_some()) {
        (_, false) => ("HEAD".to_owned(), None),
        (Some(base), true) => {
            let found = git(&dir, &["merge-base", base, tip])
                .with_context(|| format!("{tip} has no history in common with {base}"))?;
            (base.clone(), Some(found.trim().to_owned()))
        }
        (None, true) => {
            let found = git(&dir, &["rev-parse", "HEAD"])?;
            ("HEAD".to_owned(), Some(found.trim().to_owned()))
        }
    };
    let from = merge_base.as_deref().unwrap_or(EMPTY_TREE);
    let mut parser = super::parser(ask);
    let context = format!("--unified={}", super::context(ask));
    let mut args = vec![
        "-c",
        "core.quotepath=off",
        "diff",
        "--no-color",
        &context,
        "--no-ext-diff",
        "--no-textconv",
        "-M",
        "--relative",
        "--src-prefix=a/",
        "--dst-prefix=b/",
        from,
    ];
    if let Some(head) = &head {
        args.push(head);
    }
    args.push("--");
    let working_tree = head.is_none();
    // Untracked files go into a copy of the index as intent to add, so
    // git's rename detection sees a file moved without `git add`.
    let index = working_tree.then(|| untracked_index(&dir)).flatten();
    let index_path = index.as_ref().map(|index| index.path.as_path());
    stream_git(&dir, &args, index_path, |line| parser.line(line))?;

    if working_tree && index.is_none() {
        add_untracked(&dir, &mut parser)?;
    }

    let parsed = parser.finish();
    Ok(WorkDiff {
        root: dir.to_string_lossy().into_owned(),
        base: label,
        merge_base: merge_base.unwrap_or_default(),
        working_tree,
        files: parsed.files,
        additions: parsed.additions,
        deletions: parsed.deletions,
        more: parsed.more,
    })
}

/// A branch name as asked: `None` when empty; refused when git could take
/// it for an option or it is not a name.
fn branch_ref(text: &str) -> Result<Option<&str>> {
    let text = text.trim();

    if text.is_empty() {
        return Ok(None);
    }

    let bad = text.len() > MAX_REF
        || text.starts_with('-')
        || text.contains("..")
        || text.contains(|c: char| {
            c.is_control()
                || c.is_whitespace()
                || matches!(c, ':' | '~' | '^' | '?' | '*' | '[' | '\\')
        });
    if bad {
        bail!("{text} is not a branch name");
    }

    Ok(Some(text))
}

/// Whether `name` is a commit git knows here.
fn is_commit(dir: &Path, name: &str) -> bool {
    git(
        dir,
        &[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("{name}^{{commit}}"),
        ],
    )
    .is_ok()
}

/// `name` as a commit git knows here, preferring the local branch.
fn resolve(dir: &Path, name: &str) -> Result<String> {
    let remote = format!("origin/{name}");

    for candidate in [name, remote.as_str()] {
        if is_commit(dir, candidate) {
            return Ok(candidate.to_owned());
        }
    }

    bail!("git knows no branch {name} here")
}

/// The checkout's local default branch: the first of `main` and `master` there is.
fn default_base(dir: &Path) -> Option<String> {
    ["main", "master"]
        .into_iter()
        .find(|name| is_commit(dir, name))
        .map(str::to_owned)
}

/// The worktree `branch` is checked out in, when it is one inside `root`
/// (or `root` itself).
fn worktree_of(root: &Path, branch: &str) -> Result<Option<PathBuf>> {
    let list = git(root, &["worktree", "list", "--porcelain"])?;
    let want = format!("branch refs/heads/{branch}");
    let mut at: Option<&str> = None;

    for line in list.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            at = Some(path);
        } else if line == want
            && let Some(path) = at
            && let Ok(real) = paths::canonicalize(path)
            && real.starts_with(root)
        {
            return Ok(Some(real));
        }
    }

    Ok(None)
}

/// A copy of the checkout's index in a temporary folder, with its untracked
/// files added as intent to add. `None` when there are none, or git refuses.
struct UntrackedIndex {
    _dir: TempDir,
    path: PathBuf,
}

fn untracked_index(dir: &Path) -> Option<UntrackedIndex> {
    let list = git(dir, &["ls-files", "--others", "--exclude-standard", "-z"]).ok()?;
    // A folder here is a repository of its own, which `git add` would embed.
    let files: Vec<&str> = list
        .split('\0')
        .filter(|rel| !rel.is_empty() && !rel.ends_with('/'))
        .take(MAX_DIFF_FILES + 1)
        .collect();
    if files.is_empty() {
        return None;
    }

    let current = git(
        dir,
        &["rev-parse", "--path-format=absolute", "--git-path", "index"],
    )
    .ok()?;
    let temp = TempDir::new().ok()?;
    let path = temp.path().join("index");
    let current = Path::new(current.trim());

    if current.exists() {
        fs::copy(current, &path).ok()?;
    }

    let mut args = vec!["add", "--intent-to-add", "--"];
    args.extend(files);
    let added = git_command(dir, &args)
        .env("GIT_INDEX_FILE", &path)
        .env("GIT_LITERAL_PATHSPECS", "1")
        .output();
    crate::process::finished("git", &added);
    if !added.ok()?.status.success() {
        return None;
    }

    Some(UntrackedIndex { _dir: temp, path })
}

/// Adds the files git does not track and does not ignore, as added files.
fn add_untracked(dir: &Path, parser: &mut DiffParser) -> Result<()> {
    let list = git(dir, &["ls-files", "--others", "--exclude-standard", "-z"])?;

    for rel in list
        .split('\0')
        .filter(|rel| !rel.is_empty())
        .take(MAX_DIFF_FILES + 1)
    {
        let Some(text) = untracked_text(dir, rel, parser.file_bytes())? else {
            continue;
        };

        match text {
            Untracked::Binary => parser.untracked(rel, 0, true),
            Untracked::Text(text) => {
                let ends = text.ends_with('\n');
                let body = text.strip_suffix('\n').unwrap_or(&text);
                let lines: Vec<&str> = if body.is_empty() && ends {
                    vec![""]
                } else if body.is_empty() {
                    Vec::new()
                } else {
                    body.split('\n').collect()
                };
                let count = u32::try_from(lines.len()).unwrap_or(u32::MAX);
                parser.untracked(rel, count, false);

                for line in &lines {
                    parser.line(&format!("+{line}"));
                }

                if !ends && !lines.is_empty() {
                    parser.line("\\ No newline at end of file");
                }
            }
        }
    }

    Ok(())
}

enum Untracked {
    Binary,
    Text(String),
}

/// The untracked file `rel` in `dir`: a link as its target, as git shows
/// it, never followed; `None` for anything but a file or a link. Reads at
/// most `cap` bytes and one more, so a larger file shows as too large.
fn untracked_text(dir: &Path, rel: &str, cap: usize) -> Result<Option<Untracked>> {
    let path = dir.join(rel);
    let meta = match fs::symlink_metadata(&path) {
        Ok(meta) => meta,
        // Gone since git listed it.
        Err(_) => return Ok(None),
    };

    if meta.file_type().is_symlink() {
        let target = fs::read_link(&path).with_context(|| format!("could not read {rel}"))?;
        return Ok(Some(Untracked::Text(target.to_string_lossy().into_owned())));
    }

    if !meta.is_file() {
        return Ok(None);
    }

    let file = fs::File::open(&path).with_context(|| format!("could not open {rel}"))?;
    let mut data = Vec::new();
    file.take(u64::try_from(cap).unwrap_or(u64::MAX).saturating_add(1))
        .read_to_end(&mut data)
        .with_context(|| format!("could not read {rel}"))?;
    if data.iter().take(8_000).any(|&b| b == 0) {
        return Ok(Some(Untracked::Binary));
    }

    Ok(Some(Untracked::Text(
        String::from_utf8_lossy(&data).into_owned(),
    )))
}

fn git_command(dir: &Path, args: &[&str]) -> Command {
    let mut command = crate::process::background_command("git");
    command
        .args(args)
        .current_dir(dir)
        .env("GIT_TERMINAL_PROMPT", "0")
        // Reading takes no locks, so a running agent's own git is never in
        // its way.
        .env("GIT_OPTIONAL_LOCKS", "0")
        // Callers match git's English messages.
        .env("LC_ALL", "C")
        .stdin(Stdio::null());
    command
}

/// What `git <args>` printed in `dir`; its error when it failed.
pub(in crate::agents) fn git(dir: &Path, args: &[&str]) -> Result<String> {
    let out = git_command(dir, args).output();
    crate::process::finished("git", &out);
    let out = out.context("could not run git")?;
    if !out.status.success() {
        bail!(
            "git {}: {}",
            args.first().unwrap_or(&""),
            error_text(&out.stderr)
        );
    }

    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Hands each line `git <args>` prints in `dir` to `each`, as it comes,
/// so a long diff is never held whole.
fn stream_git(
    dir: &Path,
    args: &[&str],
    index: Option<&Path>,
    mut each: impl FnMut(&str),
) -> Result<()> {
    let mut command = git_command(dir, args);

    if let Some(index) = index {
        command.env("GIT_INDEX_FILE", index);
    }

    let child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();
    crate::process::launched("git", &child);
    let mut child = child.context("could not run git")?;
    let stderr = child.stderr.take().context("git's error output")?;
    // Drained apart, so a git that writes much there never blocks.
    let errors = thread::spawn(move || {
        let mut kept = Vec::new();
        // Only for the message: what was read before a failure still says
        // enough, and git's exit status tells whether it failed.
        let _ = stderr.take(64 * 1024).read_to_end(&mut kept);
        kept
    });
    let stdout = child.stdout.take().context("git's output")?;
    let mut reader = BufReader::new(stdout);
    let mut buf = Vec::new();
    let read = loop {
        buf.clear();

        match reader.read_until(b'\n', &mut buf) {
            Ok(0) => break Ok(()),
            Ok(_) => {
                let line = buf.strip_suffix(b"\n").unwrap_or(&buf);
                each(&String::from_utf8_lossy(line));
            }
            Err(e) => break Err(e),
        }
    };
    let status = child.wait().context("git did not finish")?;
    let errors = errors.join().unwrap_or_default();
    read.context("could not read git's output")?;

    if !status.success() {
        bail!("git diff: {}", error_text(&errors));
    }

    Ok(())
}

fn error_text(stderr: &[u8]) -> String {
    let text = String::from_utf8_lossy(stderr);
    let text = text.trim();
    let mut end = text.len().min(MAX_ERROR);

    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_owned()
}

#[cfg(test)]
#[path = "../../tests/agents_work_diff.rs"]
mod tests;
