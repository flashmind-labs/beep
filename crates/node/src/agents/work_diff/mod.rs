//! What changed in a checkout, for the apps' diff viewer (`read`): the git
//! diff from where the head left the base, plus the working tree's changes and
//! untracked files when the head is the working tree.

mod parse;

use crate::files::paths;
use anyhow::{Context, Result, bail};
use beep_proto::{
    MAX_DIFF_FILE_LINES, MAX_DIFF_FILES, MAX_DIFF_LINES, MAX_DIFF_SHOWN_LINES, WorkDiff,
    node::agent::DiffAsk,
};
use parse::{Caps, DiffParser, Keep};
use std::{
    fs,
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
};

/// Bytes of text one file keeps, and all of them.
const FILE_BYTES: usize = 512 * 1024;
const TOTAL_BYTES: usize = 4 * 1024 * 1024;
/// Bytes of text a file named in `only` keeps.
const SHOWN_BYTES: usize = 4 * 1024 * 1024;
/// The most of git's error kept for the message.
const MAX_ERROR: usize = 2_000;
/// The longest branch name taken.
const MAX_REF: usize = 250;
/// What git calls the tree with nothing in it, the base before the first
/// commit.
const EMPTY_TREE: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";

fn patch_caps(shown: bool) -> Caps {
    if shown {
        Caps {
            file_lines: MAX_DIFF_SHOWN_LINES,
            file_bytes: SHOWN_BYTES,
            total_lines: MAX_DIFF_SHOWN_LINES,
            total_bytes: SHOWN_BYTES,
        }
    } else {
        Caps {
            file_lines: MAX_DIFF_FILE_LINES,
            file_bytes: FILE_BYTES,
            total_lines: MAX_DIFF_LINES,
            total_bytes: TOTAL_BYTES,
        }
    }
}

/// Parses a complete Git patch with the same limits as a live worktree diff.
pub fn parse_git_patch(ask: &DiffAsk, text: &str) -> WorkDiff {
    let mut parser = DiffParser::new(
        patch_caps(!ask.only.is_empty()),
        Keep {
            only: ask.only.iter().cloned().collect(),
            lockfiles: ask.lockfiles || !ask.only.is_empty(),
        },
    );
    for line in text.lines() {
        parser.line(line);
    }
    let parsed = parser.finish();
    WorkDiff {
        root: ask.root.clone(),
        base: ask.base.clone(),
        merge_base: ask.base.clone(),
        working_tree: false,
        files: parsed.files,
        additions: parsed.additions,
        deletions: parsed.deletions,
        more: parsed.more,
    }
}

/// The diff `ask` names, in `root`, already resolved and in reach. Git
/// runs with `--relative`, so nothing outside `root` shows. Blocks on git
/// and the file system.
pub(super) fn read(root: &Path, ask: &DiffAsk) -> Result<WorkDiff> {
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
    let has_head = git(&dir, &["rev-parse", "--verify", "--quiet", "HEAD^{commit}"]).is_ok();
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
    let shown = !ask.only.is_empty();
    let caps = patch_caps(shown);
    let keep = Keep {
        only: ask.only.iter().cloned().collect(),
        lockfiles: ask.lockfiles || shown,
    };
    let mut parser = DiffParser::new(caps, keep);
    let mut args = vec![
        "-c",
        "core.quotepath=off",
        "diff",
        "--no-color",
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
    stream_git(&dir, &args, |line| parser.line(line))?;
    let working_tree = head.is_none();
    if working_tree {
        add_untracked(&dir, &mut parser, caps)?;
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

/// `name` as a commit git knows here, preferring the local branch.
fn resolve(dir: &Path, name: &str) -> Result<String> {
    let remote = format!("origin/{name}");
    for candidate in [name, remote.as_str()] {
        let rev = format!("{candidate}^{{commit}}");
        if git(dir, &["rev-parse", "--verify", "--quiet", &rev]).is_ok() {
            return Ok(candidate.to_owned());
        }
    }
    bail!("git knows no branch {name} here")
}

/// The checkout's local default branch: the first of `main` and `master` there is.
fn default_base(dir: &Path) -> Option<String> {
    ["main", "master"]
        .into_iter()
        .find(|name| {
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
        })
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

/// Adds the files git does not track and does not ignore, as added files.
fn add_untracked(dir: &Path, parser: &mut DiffParser, caps: Caps) -> Result<()> {
    let list = git(dir, &["ls-files", "--others", "--exclude-standard", "-z"])?;
    for rel in list
        .split('\0')
        .filter(|rel| !rel.is_empty())
        .take(MAX_DIFF_FILES + 1)
    {
        let Some(text) = untracked_text(dir, rel, caps.file_bytes)? else {
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
    let mut command = Command::new("git");
    command
        .args(args)
        .current_dir(dir)
        .env("GIT_TERMINAL_PROMPT", "0")
        // Reading takes no locks, so a running agent's own git is never in
        // its way.
        .env("GIT_OPTIONAL_LOCKS", "0")
        .stdin(Stdio::null());
    command
}

/// What `git <args>` printed in `dir`; its error when it failed.
pub(super) fn git(dir: &Path, args: &[&str]) -> Result<String> {
    let out = git_command(dir, args)
        .output()
        .context("could not run git")?;
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
fn stream_git(dir: &Path, args: &[&str], mut each: impl FnMut(&str)) -> Result<()> {
    let mut child = git_command(dir, args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("could not run git")?;
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
