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
    let mut out =
        format!("diff --git a/{path} b/{path}\n--- a/{path}\n+++ b/{path}\n@@ -0,0 +1,{n} @@\n");
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
