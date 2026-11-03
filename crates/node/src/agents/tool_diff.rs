//! An edit's diff as the lines its tool entry shows, so the apps can draw
//! it under the call: a `[diff]` header with the file and the counts, then
//! the changed lines marked `-` and `+` with a little context, as in a
//! unified diff.

use std::path::Path;

/// Unchanged lines kept around a change.
const CONTEXT: usize = 2;
/// The most lines one diff shows.
const MAX_LINES: usize = 80;
/// The longest line kept.
const MAX_LINE: usize = 300;

/// The text of a diff of `path`, shown relative to `cwd` when inside it:
///
/// ```text
/// [diff] src/login.rs +2 -1
///  fn check() {
/// -    old();
/// +    new();
/// +    more();
///  }
/// ```
///
/// The counts are every line added and removed; past `MAX_LINES` the rest
/// is one `… N more lines` line. A new file (`old` is `None`) is all
/// additions.
pub(super) fn diff_text(cwd: &Path, path: &Path, old: Option<&str>, new: &str) -> String {
    let shown = path.strip_prefix(cwd).unwrap_or(path).display().to_string();
    let old: Vec<&str> = old.map(|text| text.lines().collect()).unwrap_or_default();
    let new: Vec<&str> = new.lines().collect();
    let same_start = old.iter().zip(&new).take_while(|(a, b)| a == b).count();
    let same_end = old[same_start..]
        .iter()
        .rev()
        .zip(new[same_start..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    let removed = &old[same_start..old.len() - same_end];
    let added = &new[same_start..new.len() - same_end];
    let before = &new[same_start.saturating_sub(CONTEXT)..same_start];
    let after_start = new.len() - same_end;
    let after = &new[after_start..(after_start + CONTEXT).min(new.len())];
    let lines = before
        .iter()
        .map(|line| (' ', *line))
        .chain(removed.iter().map(|line| ('-', *line)))
        .chain(added.iter().map(|line| ('+', *line)))
        .chain(after.iter().map(|line| (' ', *line)));
    let mut text = format!("[diff] {shown} +{} -{}", added.len(), removed.len());
    let total = before.len() + removed.len() + added.len() + after.len();
    for (mark, line) in lines.take(MAX_LINES) {
        text.push('\n');
        text.push(mark);
        text.push_str(cut_line(line));
    }
    if total > MAX_LINES {
        text.push_str(&format!("\n… {} more lines", total - MAX_LINES));
    }
    text
}

fn cut_line(line: &str) -> &str {
    if line.len() <= MAX_LINE {
        return line;
    }
    let mut end = MAX_LINE;
    while !line.is_char_boundary(end) {
        end -= 1;
    }
    &line[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_edit_shows_its_changed_lines_with_context() {
        let old = "a\nb\nc\nold\nd\ne\nf";
        let new = "a\nb\nc\nnew\nmore\nd\ne\nf";
        assert_eq!(
            diff_text(
                Path::new("/srv/project"),
                Path::new("/srv/project/src/x.rs"),
                Some(old),
                new
            ),
            "[diff] src/x.rs +2 -1\n b\n c\n-old\n+new\n+more\n d\n e"
        );
    }

    #[test]
    fn an_edit_at_the_edges_keeps_what_context_there_is() {
        assert_eq!(
            diff_text(Path::new("/"), Path::new("/f"), Some("x\ny"), "z\ny"),
            "[diff] f +1 -1\n-x\n+z\n y"
        );
    }
}
