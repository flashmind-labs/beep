//! What changed in a checkout, for the apps' diff viewer (`read`): the git
//! diff from where the head left the base, plus the working tree's changes and
//! untracked files when the head is the working tree.

mod parse;

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
            None => (root.to_owned(), Some(resolve(root, branch, Prefer::Local)?)),
        },
    };
    let base = match branch_ref(&ask.base)? {
        Some(base) => Some(resolve(&dir, base, Prefer::Remote)?),
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
    let caps = if shown {
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
    };
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

/// Where a branch name is looked for first.
#[derive(Debug, Clone, Copy)]
enum Prefer {
    /// The remote's copy: a base, as the local one may be behind.
    Remote,
    /// The local branch: a head, where the work is.
    Local,
}

/// `name` as a commit git knows here, the copy `prefer` names first.
fn resolve(dir: &Path, name: &str, prefer: Prefer) -> Result<String> {
    let remote = format!("origin/{name}");
    let order = match prefer {
        Prefer::Remote => [remote.as_str(), name],
        Prefer::Local => [name, remote.as_str()],
    };
    for candidate in order {
        let rev = format!("{candidate}^{{commit}}");
        if git(dir, &["rev-parse", "--verify", "--quiet", &rev]).is_ok() {
            return Ok(candidate.to_owned());
        }
    }
    bail!("git knows no branch {name} here")
}

/// The checkout's default branch: what `origin/HEAD` points at, else the
/// first of `main` and `master` there is.
fn default_base(dir: &Path) -> Option<String> {
    if let Ok(out) = git(
        dir,
        &[
            "symbolic-ref",
            "--quiet",
            "--short",
            "refs/remotes/origin/HEAD",
        ],
    ) {
        let name = out.trim();
        if !name.is_empty() {
            return Some(name.to_owned());
        }
    }
    ["origin/main", "main", "origin/master", "master"]
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
            && let Ok(real) = fs::canonicalize(path)
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
fn git(dir: &Path, args: &[&str]) -> Result<String> {
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
mod tests {
    use super::*;
    use beep_proto::{DiffLeftOut, DiffStatus};
    use tempfile::TempDir;

    fn has_git() -> bool {
        Command::new("git").arg("--version").output().is_ok()
    }

    fn run_git(dir: &Path, args: &[&str]) {
        let ok = Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@example.com")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@example.com")
            .status()
            .expect("git runs")
            .success();
        assert!(ok, "git {args:?}");
    }

    fn ask(base: &str, head: &str) -> DiffAsk {
        DiffAsk {
            root: String::new(),
            base: base.to_owned(),
            head: head.to_owned(),
            only: Vec::new(),
            lockfiles: false,
        }
    }

    /// A checkout on `main` with two files committed, and a branch `work`
    /// off it.
    fn checkout() -> (TempDir, PathBuf) {
        let dir = TempDir::new().expect("a temp folder");
        let root = fs::canonicalize(dir.path()).expect("it resolves");
        run_git(&root, &["init", "-q", "-b", "main"]);
        fs::write(root.join("keep.txt"), "one\ntwo\nthree\n").unwrap();
        fs::write(root.join("old.txt"), "moved\nalong\nwith\nus\n").unwrap();
        fs::write(root.join("gone.txt"), "bye\n").unwrap();
        run_git(&root, &["add", "."]);
        run_git(&root, &["commit", "-q", "-m", "first"]);
        (dir, root)
    }

    fn file<'a>(diff: &'a WorkDiff, path: &str) -> &'a beep_proto::DiffFile {
        diff.files
            .iter()
            .find(|f| f.path == path)
            .unwrap_or_else(|| {
                panic!(
                    "{path} in {:?}",
                    diff.files.iter().map(|f| &f.path).collect::<Vec<_>>()
                )
            })
    }

    #[test]
    fn reads_commits_uncommitted_and_untracked_changes() {
        if !has_git() {
            return;
        }
        let (_dir, root) = checkout();
        run_git(&root, &["checkout", "-q", "-b", "work"]);
        fs::write(root.join("keep.txt"), "one\n2\nthree\n").unwrap();
        run_git(&root, &["commit", "-q", "-am", "change"]);
        // Uncommitted: a rename, a deletion, a new untracked file and a
        // binary one.
        run_git(&root, &["mv", "old.txt", "new.txt"]);
        run_git(&root, &["rm", "-q", "gone.txt"]);
        fs::write(root.join("fresh.txt"), "hello\nworld").unwrap();
        fs::write(root.join("blob.bin"), [0u8, 1, 2, 3]).unwrap();
        fs::write(root.join("Cargo.lock"), "a\nb\n").unwrap();
        let diff = read(&root, &ask("main", "")).expect("the diff reads");
        assert_eq!(diff.base, "main");
        assert!(diff.working_tree);
        assert_eq!(diff.root, root.to_string_lossy());
        let keep = file(&diff, "keep.txt");
        assert_eq!(keep.status, DiffStatus::Modified {});
        assert_eq!((keep.additions, keep.deletions), (1, 1));
        let moved = file(&diff, "new.txt");
        assert_eq!(moved.status, DiffStatus::Renamed {});
        assert_eq!(moved.old_path.as_deref(), Some("old.txt"));
        assert_eq!(file(&diff, "gone.txt").status, DiffStatus::Deleted {});
        let fresh = file(&diff, "fresh.txt");
        assert_eq!(fresh.status, DiffStatus::Added {});
        assert_eq!(fresh.additions, 2);
        assert!(fresh.hunks[0].lines[1].no_eol);
        assert_eq!(file(&diff, "blob.bin").status, DiffStatus::Binary {});
        assert_eq!(
            file(&diff, "Cargo.lock").left_out,
            Some(DiffLeftOut::Lockfile {})
        );
    }

    #[test]
    fn a_branch_with_a_worktree_inside_gives_its_working_tree() {
        if !has_git() {
            return;
        }
        let (_dir, root) = checkout();
        run_git(
            &root,
            &["worktree", "add", "-q", "-b", "wt", ".worktrees/wt"],
        );
        let wt = root.join(".worktrees/wt");
        fs::write(wt.join("note.md"), "new\n").unwrap();
        let diff = read(&root, &ask("main", "wt")).expect("the diff reads");
        assert!(diff.working_tree);
        assert_eq!(diff.root, wt.to_string_lossy());
        assert_eq!(diff.files.len(), 1);
        assert_eq!(diff.files[0].path, "note.md");
    }

    #[test]
    fn the_default_base_is_found_and_a_subfolder_stays_inside() {
        if !has_git() {
            return;
        }
        let (_dir, root) = checkout();
        fs::create_dir(root.join("sub")).unwrap();
        fs::write(root.join("sub/in.txt"), "x\n").unwrap();
        fs::write(root.join("keep.txt"), "changed\n").unwrap();
        let diff = read(&root.join("sub"), &ask("", "")).expect("the diff reads");
        assert_eq!(diff.base, "main");
        let paths: Vec<_> = diff.files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, ["in.txt"]);
    }

    #[test]
    fn refuses_bad_names_and_folders_outside_a_checkout() {
        if !has_git() {
            return;
        }
        let (_dir, root) = checkout();
        for bad in ["--output=/tmp/x", "a..b", "a b"] {
            let e = read(&root, &ask(bad, "")).unwrap_err();
            assert!(e.to_string().contains("not a branch name"), "{bad}: {e:#}");
        }
        assert!(read(&root, &ask("nope", "")).is_err());
        let plain = TempDir::new().unwrap();
        let e = read(plain.path(), &ask("", "")).unwrap_err();
        assert!(e.to_string().contains("not in a git checkout"), "{e:#}");
    }

    #[test]
    fn a_checkout_with_no_commits_shows_its_files() {
        if !has_git() {
            return;
        }
        let dir = TempDir::new().unwrap();
        let root = fs::canonicalize(dir.path()).unwrap();
        run_git(&root, &["init", "-q", "-b", "main"]);
        fs::write(root.join("a.txt"), "a\n").unwrap();
        let diff = read(&root, &ask("", "")).expect("the diff reads");
        assert_eq!(diff.base, "HEAD");
        assert_eq!(diff.merge_base, "");
        assert_eq!(diff.files.len(), 1);
        assert_eq!(diff.files[0].status, DiffStatus::Added {});
    }
}
