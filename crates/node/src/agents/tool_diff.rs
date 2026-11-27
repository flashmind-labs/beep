//! An edit's diff as the lines its tool entry shows (`diff_text`): a
//! `[diff]` header with the file and counts, then the changed lines marked `-`
//! and `+` with a little context, as in a unified diff.

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
        text.push_str(crate::text::cut_str(line, MAX_LINE));
    }

    if total > MAX_LINES {
        text.push_str(&format!("\n… {} more lines", total - MAX_LINES));
    }

    text
}

#[cfg(test)]
#[path = "../tests/agents_tool_diff.rs"]
mod tests;
