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
mod tests {
    use super::*;

    const CAPS: Caps = Caps {
        file_lines: 2_000,
        file_bytes: 512 * 1024,
        total_lines: 20_000,
        total_bytes: 4 * 1024 * 1024,
    };

    fn parse_with(text: &str, caps: Caps, keep: Keep) -> Parsed {
        let mut parser = DiffParser::new(caps, keep);
        for line in text.split('\n') {
            parser.line(line);
        }
        parser.finish()
    }

    fn parse(text: &str) -> Parsed {
        parse_with(text, CAPS, Keep::default())
    }

    fn texts(hunk: &DiffHunk) -> Vec<(char, &str)> {
        hunk.lines
            .iter()
            .map(|line| {
                let mark = match line.kind {
                    DiffLineKind::Added {} => '+',
                    DiffLineKind::Removed {} => '-',
                    DiffLineKind::Context {} => ' ',
                };
                (mark, line.text.as_str())
            })
            .collect()
    }

    #[test]
    fn reads_a_modified_file_with_two_hunks() {
        let diff = "diff --git a/src/lib.rs b/src/lib.rs
index 1111111..2222222 100644
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,3 +1,4 @@ mod a;
 use std::fs;
+use std::io;

 fn main() {
@@ -20,2 +21,2 @@ fn other() {
-    old();
+    new();
     done();
";
        let parsed = parse(diff);
        assert_eq!(parsed.files.len(), 1);
        let file = &parsed.files[0];
        assert_eq!(file.path, "src/lib.rs");
        assert_eq!(file.old_path, None);
        assert_eq!(file.status, DiffStatus::Modified {});
        assert_eq!((file.additions, file.deletions), (2, 1));
        assert_eq!(file.hunks.len(), 2);
        assert_eq!(file.hunks[0].section, "mod a;");
        assert_eq!(
            texts(&file.hunks[0]),
            [
                (' ', "use std::fs;"),
                ('+', "use std::io;"),
                (' ', ""),
                (' ', "fn main() {")
            ]
        );
        let second = &file.hunks[1];
        assert_eq!(
            (
                second.old_start,
                second.old_lines,
                second.new_start,
                second.new_lines
            ),
            (20, 2, 21, 2)
        );
        assert_eq!(
            texts(second),
            [
                ('-', "    old();"),
                ('+', "    new();"),
                (' ', "    done();")
            ]
        );
        assert_eq!((parsed.additions, parsed.deletions), (2, 1));
    }

    #[test]
    fn reads_renames_with_and_without_changes() {
        let diff = "diff --git a/old name.rs b/new name.rs
similarity index 90%
rename from old name.rs
rename to new name.rs
index 1111111..2222222 100644
--- a/old name.rs\t
+++ b/new name.rs\t
@@ -1 +1 @@
-a
+b
diff --git a/x.txt b/y/x.txt
similarity index 100%
rename from x.txt
rename to y/x.txt
";
        let parsed = parse(diff);
        assert_eq!(parsed.files.len(), 2);
        let changed = &parsed.files[0];
        assert_eq!(changed.path, "new name.rs");
        assert_eq!(changed.old_path.as_deref(), Some("old name.rs"));
        assert_eq!(changed.status, DiffStatus::Renamed {});
        assert_eq!((changed.additions, changed.deletions), (1, 1));
        let moved = &parsed.files[1];
        assert_eq!(moved.path, "y/x.txt");
        assert_eq!(moved.old_path.as_deref(), Some("x.txt"));
        assert_eq!(moved.status, DiffStatus::Renamed {});
        assert!(moved.hunks.is_empty());
    }

    #[test]
    fn reads_binary_added_and_deleted_files() {
        let diff = "diff --git a/logo.png b/logo.png
index 1111111..2222222 100644
Binary files a/logo.png and b/logo.png differ
diff --git a/new.txt b/new.txt
new file mode 100644
index 0000000..2222222
--- /dev/null
+++ b/new.txt
@@ -0,0 +1,2 @@
+one
+two
diff --git a/gone.txt b/gone.txt
deleted file mode 100644
index 1111111..0000000
--- a/gone.txt
+++ /dev/null
@@ -1 +0,0 @@
-bye
";
        let parsed = parse(diff);
        let status: Vec<_> = parsed
            .files
            .iter()
            .map(|f| (f.path.as_str(), f.status))
            .collect();
        assert_eq!(
            status,
            [
                ("logo.png", DiffStatus::Binary {}),
                ("new.txt", DiffStatus::Added {}),
                ("gone.txt", DiffStatus::Deleted {}),
            ]
        );
        assert!(parsed.files[0].hunks.is_empty());
        assert_eq!(parsed.files[0].left_out, None);
        assert_eq!((parsed.additions, parsed.deletions), (2, 1));
    }

    #[test]
    fn marks_a_missing_newline_at_the_end() {
        let diff = "diff --git a/a.txt b/a.txt
--- a/a.txt
+++ b/a.txt
@@ -1,2 +1,2 @@
 keep
-last
\\ No newline at end of file
+last
";
        let parsed = parse(diff);
        let lines = &parsed.files[0].hunks[0].lines;
        assert_eq!(lines.len(), 3);
        assert!(!lines[0].no_eol);
        assert!(lines[1].no_eol, "the old file ended without a newline");
        assert!(!lines[2].no_eol, "the new one has it");
        assert_eq!((parsed.additions, parsed.deletions), (1, 1));
    }

    #[test]
    fn crlf_lines_lose_their_carriage_return() {
        let diff = "diff --git a/w.bat b/w.bat\n--- a/w.bat\n+++ b/w.bat\n@@ -1,2 +1,2 @@\n echo on\r\n-del x\r\n+del y\r\n";
        let parsed = parse(diff);
        let file = &parsed.files[0];
        assert_eq!(
            texts(&file.hunks[0]),
            [(' ', "echo on"), ('-', "del x"), ('+', "del y")]
        );
        assert_eq!((file.additions, file.deletions), (1, 1));
    }

    #[test]
    fn reads_quoted_paths() {
        let diff = "diff --git \"a/caf\\303\\251 \\\"x\\\".txt\" \"b/caf\\303\\251 \\\"x\\\".txt\"
new file mode 100644
--- /dev/null
+++ \"b/caf\\303\\251 \\\"x\\\".txt\"
@@ -0,0 +1 @@
+hi
";
        let parsed = parse(diff);
        assert_eq!(parsed.files[0].path, "caf\u{e9} \"x\".txt");
        assert_eq!(parsed.files[0].status, DiffStatus::Added {});
    }

    #[test]
    fn a_line_like_a_header_inside_a_hunk_is_content() {
        let diff = "diff --git a/a.md b/a.md
--- a/a.md
+++ b/a.md
@@ -1,2 +1,2 @@
---- a/b
+++++ b/c
 rename from x
";
        let parsed = parse(diff);
        let file = &parsed.files[0];
        assert_eq!(file.path, "a.md");
        assert_eq!(
            texts(&file.hunks[0]),
            [('-', "--- a/b"), ('+', "++++ b/c"), (' ', "rename from x")]
        );
    }

    fn many_lines(path: &str, n: u32) -> String {
        let mut out = format!(
            "diff --git a/{path} b/{path}\n--- a/{path}\n+++ b/{path}\n@@ -0,0 +1,{n} @@\n"
        );
        for i in 0..n {
            out.push_str(&format!("+line {i}\n"));
        }
        out
    }

    #[test]
    fn a_file_past_its_cap_keeps_its_counts_only() {
        let caps = Caps {
            file_lines: 10,
            ..CAPS
        };
        let diff = many_lines("big.rs", 11) + &many_lines("small.rs", 10);
        let parsed = parse_with(&diff, caps, Keep::default());
        let big = &parsed.files[0];
        assert_eq!(big.left_out, Some(DiffLeftOut::TooLarge {}));
        assert!(big.hunks.is_empty());
        assert_eq!(big.additions, 11);
        let small = &parsed.files[1];
        assert_eq!(small.left_out, None);
        assert_eq!(small.hunks[0].lines.len(), 10);
        assert_eq!(parsed.additions, 21);
    }

    #[test]
    fn files_past_the_total_lose_their_lines() {
        let caps = Caps {
            total_lines: 25,
            ..CAPS
        };
        let diff = many_lines("a.rs", 10)
            + &many_lines("b.rs", 10)
            + &many_lines("c.rs", 10)
            + &many_lines("d.rs", 5);
        let parsed = parse_with(&diff, caps, Keep::default());
        let left: Vec<_> = parsed.files.iter().map(|f| f.left_out).collect();
        // A smaller file after one that did not fit still does.
        assert_eq!(left, [None, None, Some(DiffLeftOut::OverTotal {}), None]);
        assert_eq!(parsed.additions, 35);
    }

    #[test]
    fn lockfiles_are_left_out_unless_asked() {
        let diff = many_lines("Cargo.lock", 3) + &many_lines("web/package-lock.json", 3);
        let parsed = parse(&diff);
        assert!(
            parsed
                .files
                .iter()
                .all(|f| f.left_out == Some(DiffLeftOut::Lockfile {}) && f.hunks.is_empty())
        );
        let keep = Keep {
            lockfiles: true,
            ..Keep::default()
        };
        let parsed = parse_with(&diff, CAPS, keep);
        assert!(
            parsed
                .files
                .iter()
                .all(|f| f.left_out.is_none() && f.hunks[0].lines.len() == 3)
        );
    }

    #[test]
    fn only_keeps_the_named_files() {
        let diff = many_lines("a.rs", 3) + &many_lines("Cargo.lock", 3);
        let keep = Keep {
            only: HashSet::from(["Cargo.lock".to_owned()]),
            lockfiles: true,
        };
        let parsed = parse_with(&diff, CAPS, keep);
        assert_eq!(parsed.files.len(), 1);
        assert_eq!(parsed.files[0].path, "Cargo.lock");
        assert_eq!(parsed.files[0].hunks[0].lines.len(), 3);
    }

    #[test]
    fn long_lines_are_cut_on_a_character() {
        let long = "\u{e9}".repeat(MAX_LINE);
        let diff = format!("diff --git a/a b/a\n--- a/a\n+++ b/a\n@@ -0,0 +1 @@\n+{long}\n");
        let parsed = parse(&diff);
        let text = &parsed.files[0].hunks[0].lines[0].text;
        assert_eq!(text.len(), MAX_LINE);
    }
}
