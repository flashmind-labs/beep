//! Reads `git diff` output a line at a time into `DiffFile`s, keeping at
//! most what the caps let through: a file past its cap, a lockfile, or
//! one past the total keeps its counts and loses its lines.

use beep_proto::{
    DiffFile, DiffHunk, DiffLeftOut, DiffLine, DiffLineKind, DiffStatus, MAX_DIFF_FILES,
};
use std::collections::HashSet;

/// The longest line kept, in bytes; the rest of it is cut.
const MAX_LINE: usize = 2_000;

/// How much one diff keeps.
#[derive(Debug, Clone, Copy)]
pub(super) struct Caps {
    /// Lines one file keeps.
    pub(super) file_lines: u32,
    /// Bytes of text one file keeps.
    pub(super) file_bytes: usize,
    /// Lines all the files keep.
    pub(super) total_lines: u32,
    /// Bytes of text all the files keep.
    pub(super) total_bytes: usize,
}

/// What the parser was asked to keep apart from the caps.
#[derive(Debug, Default)]
pub(super) struct Keep {
    /// Only these paths, when not empty.
    pub(super) only: HashSet<String>,
    /// Lockfiles keep their lines.
    pub(super) lockfiles: bool,
}

/// The files read so far, with the totals over all of them.
#[derive(Debug, Default)]
pub(super) struct Parsed {
    pub(super) files: Vec<DiffFile>,
    pub(super) additions: u32,
    pub(super) deletions: u32,
    pub(super) more: u32,
}

/// The file being read.
#[derive(Debug, Default)]
struct Building {
    /// Paths from `diff --git`, used when no other header names them.
    git_old: Option<String>,
    git_new: Option<String>,
    /// From `---` and `+++`; `None` inside for `/dev/null`.
    minus: Option<Option<String>>,
    plus: Option<Option<String>>,
    rename_from: Option<String>,
    rename_to: Option<String>,
    new_file: bool,
    deleted: bool,
    binary: bool,
    additions: u32,
    deletions: u32,
    hunks: Vec<DiffHunk>,
    /// Lines and bytes kept in `hunks`.
    lines: u32,
    bytes: usize,
    left_out: Option<DiffLeftOut>,
    /// Old and new lines the hunk being read still has to come.
    old_left: u32,
    new_left: u32,
}

impl Building {
    fn path(&self) -> String {
        self.rename_to
            .clone()
            .or_else(|| self.plus.clone().flatten())
            .or_else(|| self.minus.clone().flatten())
            .or_else(|| self.git_new.clone())
            .or_else(|| self.git_old.clone())
            .unwrap_or_default()
    }

    fn old_path(&self) -> Option<String> {
        self.rename_from
            .clone()
            .or_else(|| self.minus.clone().flatten())
            .or_else(|| self.git_old.clone())
    }

    fn status(&self, renamed: bool) -> DiffStatus {
        if self.binary {
            DiffStatus::Binary {}
        } else if renamed {
            DiffStatus::Renamed {}
        } else if self.new_file || self.minus == Some(None) {
            DiffStatus::Added {}
        } else if self.deleted || self.plus == Some(None) {
            DiffStatus::Deleted {}
        } else {
            DiffStatus::Modified {}
        }
    }

    fn in_hunk(&self) -> bool {
        self.old_left > 0 || self.new_left > 0
    }
}

/// Reads one diff's output, a line at a time, into kept files.
pub(super) struct DiffParser {
    caps: Caps,
    keep: Keep,
    out: Parsed,
    lines: u32,
    bytes: usize,
    file: Option<Building>,
}

impl DiffParser {
    pub(super) fn new(caps: Caps, keep: Keep) -> Self {
        Self {
            caps,
            keep,
            out: Parsed::default(),
            lines: 0,
            bytes: 0,
            file: None,
        }
    }

    /// The bytes of text one file keeps.
    #[cfg(feature = "host")]
    pub(super) fn file_bytes(&self) -> usize {
        self.caps.file_bytes
    }

    /// One line of `git diff` output, its `\n` taken off. A `\r` before
    /// it is part of a changed line in a file with CRLF endings, and goes.
    pub(super) fn line(&mut self, raw: &str) {
        let line = raw.strip_suffix('\r').unwrap_or(raw);

        if let Some(file) = self.file.as_mut()
            && file.in_hunk()
        {
            let (kind, text) = match line.split_at_checked(1) {
                Some(("+", text)) => (DiffLineKind::Added {}, text),
                Some(("-", text)) => (DiffLineKind::Removed {}, text),
                Some((" ", text)) => (DiffLineKind::Context {}, text),
                Some(("\\", _)) => return self.no_eol(),
                // An empty context line some tools write without its space.
                None if line.is_empty() => (DiffLineKind::Context {}, ""),
                _ => {
                    // Not a hunk line after all: the hunk was cut short.
                    file.old_left = 0;
                    file.new_left = 0;
                    return self.header(line);
                }
            };

            match kind {
                DiffLineKind::Added {} => {
                    file.new_left = file.new_left.saturating_sub(1);
                    file.additions = file.additions.saturating_add(1);
                }
                DiffLineKind::Removed {} => {
                    file.old_left = file.old_left.saturating_sub(1);
                    file.deletions = file.deletions.saturating_add(1);
                }
                DiffLineKind::Context {} => {
                    file.old_left = file.old_left.saturating_sub(1);
                    file.new_left = file.new_left.saturating_sub(1);
                }
            }

            return self.keep_line(kind, text);
        }

        self.header(line);
    }

    fn header(&mut self, line: &str) {
        if let Some(rest) = line.strip_prefix("diff --git ") {
            self.finish_file();
            let (old, new) = git_paths(rest);
            self.file = Some(Building {
                git_old: old,
                git_new: new,
                ..Building::default()
            });
            return;
        }

        let Some(file) = self.file.as_mut() else {
            return;
        };
        if let Some(rest) = line.strip_prefix("@@ ") {
            if let Some(hunk) = hunk_header(rest) {
                file.old_left = hunk.old_lines;
                file.new_left = hunk.new_lines;
                if file.left_out.is_none() {
                    file.hunks.push(hunk);
                }
            }
        } else if line.starts_with('\\') {
            self.no_eol();
        } else if let Some(rest) = line.strip_prefix("--- ") {
            file.minus = Some(side_path(rest, "a/"));
        } else if let Some(rest) = line.strip_prefix("+++ ") {
            file.plus = Some(side_path(rest, "b/"));
        } else if let Some(rest) = line.strip_prefix("rename from ") {
            file.rename_from = Some(unquote(rest));
        } else if let Some(rest) = line.strip_prefix("rename to ") {
            file.rename_to = Some(unquote(rest));
        } else if line.starts_with("new file mode ") {
            file.new_file = true;
        } else if line.starts_with("deleted file mode ") {
            file.deleted = true;
        } else if line.starts_with("Binary files ") && line.ends_with(" differ") {
            file.binary = true;
        }
    }

    fn no_eol(&mut self) {
        let last = self
            .file
            .as_mut()
            .and_then(|file| file.hunks.last_mut())
            .and_then(|hunk| hunk.lines.last_mut());
        if let Some(line) = last {
            line.no_eol = true;
        }
    }

    fn keep_line(&mut self, kind: DiffLineKind, text: &str) {
        let caps = self.caps;
        let Some(file) = self.file.as_mut() else {
            return;
        };

        if file.left_out.is_some() {
            return;
        }

        let text = cut(text, MAX_LINE);

        if file.lines >= caps.file_lines || file.bytes + text.len() > caps.file_bytes {
            file.left_out = Some(DiffLeftOut::TooLarge {});
            file.hunks = Vec::new();
            return;
        }

        let Some(hunk) = file.hunks.last_mut() else {
            return;
        };
        file.lines += 1;
        file.bytes += text.len();
        hunk.lines.push(DiffLine {
            kind,
            text: text.to_owned(),
            no_eol: false,
        });
    }

    /// Starts an added file at `path` that git does not track, as if git
    /// had diffed it: its lines follow as `+` lines through [`Self::line`],
    /// `lines` of them, or none for a binary file.
    #[cfg(feature = "host")]
    pub(super) fn untracked(&mut self, path: &str, lines: u32, binary: bool) {
        self.finish_file();
        let mut file = Building {
            git_new: Some(path.to_owned()),
            new_file: true,
            binary,
            ..Building::default()
        };

        if !binary && lines > 0 {
            file.hunks.push(DiffHunk {
                old_start: 0,
                old_lines: 0,
                new_start: 1,
                new_lines: lines,
                section: String::new(),
                lines: Vec::new(),
            });
            file.new_left = lines;
        }

        self.file = Some(file);
    }

    fn finish_file(&mut self) {
        let Some(mut file) = self.file.take() else {
            return;
        };
        let path = file.path();
        let old = file.old_path();
        let renamed = file.rename_to.is_some() || old.as_ref().is_some_and(|old| *old != path);
        let wanted = self.keep.only.is_empty()
            || self.keep.only.contains(&path)
            || old.as_ref().is_some_and(|old| self.keep.only.contains(old));
        if !wanted {
            return;
        }

        self.out.additions = self.out.additions.saturating_add(file.additions);
        self.out.deletions = self.out.deletions.saturating_add(file.deletions);

        if self.out.files.len() >= MAX_DIFF_FILES {
            self.out.more += 1;
            return;
        }

        if file.left_out.is_none() && !self.keep.lockfiles && is_lockfile(&path) {
            file.left_out = Some(DiffLeftOut::Lockfile {});
        }
        if file.left_out.is_none()
            && (self.lines + file.lines > self.caps.total_lines
                || self.bytes + file.bytes > self.caps.total_bytes)
        {
            file.left_out = Some(DiffLeftOut::OverTotal {});
        }

        if file.left_out.is_some() {
            file.hunks = Vec::new();
        } else {
            self.lines += file.lines;
            self.bytes += file.bytes;
        }

        let status = file.status(renamed);
        self.out.files.push(DiffFile {
            path,
            old_path: old.filter(|_| renamed),
            status,
            additions: file.additions,
            deletions: file.deletions,
            hunks: file.hunks,
            left_out: file.left_out,
        });
    }

    pub(super) fn finish(mut self) -> Parsed {
        self.finish_file();
        self.out
    }
}

/// `text` cut to at most `max` bytes, on a character boundary.
fn cut(text: &str, max: usize) -> &str {
    if text.len() <= max {
        return text;
    }

    let mut end = max;

    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

/// The paths of `a/<old> b/<new>` after `diff --git `. Without quotes and
/// with spaces in the names the split is only certain when both are the
/// same, as they are unless the file was renamed, when `rename from` and
/// `rename to` name them anyway.
fn git_paths(rest: &str) -> (Option<String>, Option<String>) {
    if rest.starts_with('"') {
        let (old, after) = quoted(rest);
        let after = after.trim_start();
        let new = if after.starts_with('"') {
            quoted(after).0
        } else {
            after.to_owned()
        };
        return (
            old.strip_prefix("a/").map(str::to_owned),
            new.strip_prefix("b/").map(str::to_owned),
        );
    }
    // `a/X b/X`: the halves are the same length.
    if rest.len() >= 5 && (rest.len() - 1).is_multiple_of(2) {
        let half = (rest.len() - 1) / 2;

        if let (Some(old), Some(new)) = (rest.get(..half), rest.get(half + 1..))
            && let (Some(old), Some(new)) = (old.strip_prefix("a/"), new.strip_prefix("b/"))
            && old == new
        {
            return (Some(old.to_owned()), Some(new.to_owned()));
        }
    }

    match rest.split_once(" b/") {
        Some((old, new)) => (
            old.strip_prefix("a/").map(str::to_owned),
            Some(new.to_owned()),
        ),
        None => (None, None),
    }
}

/// The path after `--- ` or `+++ `: `None` for `/dev/null`, else without
/// its `prefix` and the tab git puts after a name with spaces.
fn side_path(rest: &str, prefix: &str) -> Option<String> {
    let rest = rest.strip_suffix('\t').unwrap_or(rest);

    if rest == "/dev/null" {
        return None;
    }

    let path = unquote(rest);
    Some(path.strip_prefix(prefix).map(str::to_owned).unwrap_or(path))
}

/// A path as git writes it: in double quotes with C escapes when it has
/// characters that need them, else as it is.
fn unquote(text: &str) -> String {
    if text.starts_with('"') {
        quoted(text).0
    } else {
        text.to_owned()
    }
}

/// The C-quoted string at the start of `text` and what follows it. Octal
/// escapes are bytes, so a UTF-8 name comes back whole.
fn quoted(text: &str) -> (String, &str) {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 1;

    while i < bytes.len() {
        match bytes[i] {
            b'"' => {
                return (String::from_utf8_lossy(&out).into_owned(), &text[i + 1..]);
            }
            b'\\' if i + 1 < bytes.len() => {
                i += 1;

                match bytes[i] {
                    b'n' => out.push(b'\n'),
                    b't' => out.push(b'\t'),
                    b'r' => out.push(b'\r'),
                    b'a' => out.push(7),
                    b'b' => out.push(8),
                    b'f' => out.push(12),
                    b'v' => out.push(11),
                    d @ b'0'..=b'7' => {
                        let mut value = u32::from(d - b'0');
                        let mut taken = 1;

                        while taken < 3
                            && i + 1 < bytes.len()
                            && (b'0'..=b'7').contains(&bytes[i + 1])
                        {
                            i += 1;
                            taken += 1;
                            value = value * 8 + u32::from(bytes[i] - b'0');
                        }

                        out.push(u8::try_from(value).unwrap_or(b'?'));
                    }
                    other => out.push(other),
                }
            }
            other => out.push(other),
        }
        i += 1;
    }
    (String::from_utf8_lossy(&out).into_owned(), "")
}

/// `-a,b +c,d @@ section` after the first `@@ `.
fn hunk_header(rest: &str) -> Option<DiffHunk> {
    let (ranges, section) = rest.split_once("@@")?;
    let mut parts = ranges.split_whitespace();
    let (old_start, old_lines) = range(parts.next()?.strip_prefix('-')?)?;
    let (new_start, new_lines) = range(parts.next()?.strip_prefix('+')?)?;
    Some(DiffHunk {
        old_start,
        old_lines,
        new_start,
        new_lines,
        section: section.trim().to_owned(),
        lines: Vec::new(),
    })
}

/// `start,count`, or `start` alone for one line.
fn range(text: &str) -> Option<(u32, u32)> {
    match text.split_once(',') {
        Some((start, count)) => Some((start.parse().ok()?, count.parse().ok()?)),
        None => Some((text.parse().ok()?, 1)),
    }
}

/// Lockfiles: their diffs are long and seldom read.
fn is_lockfile(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    matches!(
        name,
        "Cargo.lock"
            | "package-lock.json"
            | "npm-shrinkwrap.json"
            | "yarn.lock"
            | "pnpm-lock.yaml"
            | "bun.lock"
            | "bun.lockb"
            | "Gemfile.lock"
            | "poetry.lock"
            | "uv.lock"
            | "Pipfile.lock"
            | "composer.lock"
            | "go.sum"
            | "Podfile.lock"
            | "Package.resolved"
            | "flake.lock"
            | "mix.lock"
            | "pubspec.lock"
            | "gradle.lockfile"
    )
}

#[cfg(test)]
#[path = "../../tests/agents_work_diff_parse.rs"]
mod tests;
