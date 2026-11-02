//! What reviewer and verifier sessions are asked, and reading what they
//! answer.

use super::git::{Changed, Shard};
use crate::{protocol::ReviewSeverity, store::ReviewFindingRow};
use serde::Deserialize;
use serde_json::Value;
use std::fmt::Write;

/// The most findings kept from one reviewer.
pub(crate) const MAX_FINDINGS: usize = 10;
/// The most files the stat in a prompt lists.
const MAX_STAT: usize = 300;
/// The most paths a diff command in a prompt names; past it, the shard's
/// folder.
const MAX_PATHS: usize = 60;
const MAX_TITLE: usize = 200;
const MAX_WHY: usize = 400;
const MAX_FILE: usize = 400;

const FINDINGS_START: &str = "BEEP-FINDINGS";
const FINDINGS_END: &str = "END-BEEP-FINDINGS";
const VERDICTS_START: &str = "BEEP-VERDICTS";
const VERDICTS_END: &str = "END-BEEP-VERDICTS";

/// What every session of a review is told first.
const READ_ONLY: &str = "This is a read-only review. Do not edit, create or delete files, and \
     do not commit, check out, switch, stash, reset, rebase, merge or push: nothing may change \
     the checkout, its branches or any remote, and nothing may post anywhere. Reading files, \
     `git log`, `git show` and `git diff`, and running tests or builds that only write build \
     output, are fine.";

/// The branch a review reads.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Subject<'a> {
    pub(crate) project: &'a str,
    pub(crate) branch: &'a str,
    pub(crate) base: &'a str,
    pub(crate) head: &'a str,
    pub(crate) merge_base: &'a str,
}

fn short(sha: &str) -> &str {
    sha.get(..10).unwrap_or(sha)
}

/// `text` cut to `max` bytes on a character boundary, on one line.
fn clip(text: &str, max: usize) -> String {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut end = text.len().min(max);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_owned()
}

fn intro(subject: &Subject<'_>) -> String {
    format!(
        "You are reviewing the branch `{branch}` of {project} before it merges into `{base}`. \
         {READ_ONLY}\n\nThe branch head is {head}; it forked from `{base}` at {mb}. The working \
         tree may not be at the head: read the branch's version of a file with \
         `git show {head}:<path>`.\n",
        branch = subject.branch,
        project = subject.project,
        base = subject.base,
        head = short(subject.head),
        mb = short(subject.merge_base),
    )
}

/// What one shard's reviewer is asked: the whole change's stat, the
/// command that shows its own shard's diff, and the block to end with.
pub(crate) fn reviewer_prompt(subject: &Subject<'_>, changed: &[Changed], shard: &Shard) -> String {
    let mut out = intro(subject);
    out.push_str("\nThe whole change, lines changed per file:\n");
    for file in changed.iter().take(MAX_STAT) {
        let _ = writeln!(out, "  {} ({})", file.path, file.lines);
    }
    if changed.len() > MAX_STAT {
        let _ = writeln!(out, "  and {} more files", changed.len() - MAX_STAT);
    }
    let paths: Vec<String> = if shard.paths.len() <= MAX_PATHS {
        shard.paths.iter().map(|path| quote_path(path)).collect()
    } else {
        vec![quote_path(&format!("{}/", shard.area))]
    };
    let _ = write!(
        out,
        "\nOther reviewers read the rest. You review one shard, {area}: {files} files, {lines} \
         changed lines. Read its diff with:\n\n    git diff {mb} {head} -- {paths}\n\n\
         Look for real problems in it: bugs, regressions, security holes, data loss, broken \
         compatibility, and tests that miss the change or test the wrong thing. Read the code \
         around the change when that helps. Skip style and taste.\n\n\
         End your reply with what you found, most serious first, at most {MAX_FINDINGS}, one \
         JSON object per line between these two marker lines:\n\n\
         {FINDINGS_START}\n\
         {{\"severity\": \"high\", \"title\": \"What is wrong, in a few words\", \"file\": \"path/to/file\", \"line\": 18, \"why\": \"One line on why it matters\"}}\n\
         {FINDINGS_END}\n\n\
         severity is high, medium or low; line is null when there is none. With nothing to \
         report, put nothing between the markers.",
        area = shard.area,
        files = shard.paths.len(),
        lines = shard.lines,
        mb = short(subject.merge_base),
        head = short(subject.head),
        paths = paths.join(" "),
    );
    out
}

/// What a fresh verifier is asked about one shard's findings, numbered
/// from 1 in the order given.
pub(crate) fn verifier_prompt(subject: &Subject<'_>, findings: &[ReviewFindingRow]) -> String {
    let mut out = intro(subject);
    out.push_str(
        "\nAnother reviewer reported the findings below. Check each one against the code at the \
         branch head and the diff: keep it only when the problem is there and matters as \
         described. Be strict: a finding that is wrong, already handled, or not a problem is \
         not real.\n\n",
    );
    for (n, finding) in findings.iter().enumerate() {
        let severity: &'static str = finding.severity.into();
        let place = match finding.line {
            Some(line) => format!("{}:{line}", finding.file),
            None => finding.file.clone(),
        };
        let _ = writeln!(
            out,
            "{}. [{severity}] {} at {place}\n   {}",
            n + 1,
            finding.title,
            finding.why
        );
    }
    let _ = write!(
        out,
        "\nEnd your reply with one JSON object per finding, one per line, between these two \
         marker lines:\n\n\
         {VERDICTS_START}\n\
         {{\"n\": 1, \"real\": true, \"why\": \"One line on what you found\"}}\n\
         {VERDICTS_END}"
    );
    out
}

fn quote_path(path: &str) -> String {
    super::super::commands::quote(path)
}

/// The lines between the last `start` marker line and its `end`; `None`
/// when there is no start marker.
fn block<'a>(text: &'a str, start: &str, end: &str) -> Option<Vec<&'a str>> {
    let lines: Vec<&str> = text.lines().collect();
    let from = lines.iter().rposition(|line| line.trim() == start)?;
    Some(
        lines[from + 1..]
            .iter()
            .take_while(|line| line.trim() != end)
            .map(|line| line.trim())
            .collect(),
    )
}

/// One finding as a reviewer reported it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Reported {
    pub(crate) severity: ReviewSeverity,
    pub(crate) title: String,
    pub(crate) file: String,
    pub(crate) line: Option<u32>,
    pub(crate) why: String,
}

#[derive(Deserialize)]
struct RawFinding {
    #[serde(default)]
    severity: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    file: String,
    #[serde(default)]
    line: Value,
    #[serde(default)]
    why: String,
}

fn severity(text: &str) -> ReviewSeverity {
    match text.trim().to_ascii_lowercase().as_str() {
        "high" | "critical" | "major" | "blocker" => ReviewSeverity::High {},
        "medium" | "moderate" => ReviewSeverity::Medium {},
        _ => ReviewSeverity::Low {},
    }
}

fn line_of(value: &Value) -> Option<u32> {
    match value {
        Value::Number(n) => n.as_u64().and_then(|n| u32::try_from(n).ok()),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
    .filter(|line| *line > 0)
}

/// The findings in a reviewer's reply; `None` when it has no findings
/// block. Lines that are not a finding are skipped.
pub(crate) fn parse_findings(text: &str) -> Option<Vec<Reported>> {
    let lines = block(text, FINDINGS_START, FINDINGS_END)?;
    Some(
        lines
            .into_iter()
            .filter(|line| line.starts_with('{'))
            .filter_map(|line| serde_json::from_str::<RawFinding>(line).ok())
            .filter(|raw| !raw.title.trim().is_empty())
            .map(|raw| {
                let mut file = raw.file.trim().to_owned();
                let mut line = line_of(&raw.line);
                // `path:18` in the file.
                if line.is_none()
                    && let Some((path, n)) = file.rsplit_once(':')
                    && let Ok(n) = n.parse::<u32>()
                {
                    line = Some(n);
                    file = path.to_owned();
                }
                Reported {
                    severity: severity(&raw.severity),
                    title: clip(&raw.title, MAX_TITLE),
                    file: clip(&file, MAX_FILE),
                    line,
                    why: clip(&raw.why, MAX_WHY),
                }
            })
            .take(MAX_FINDINGS)
            .collect(),
    )
}

/// What a verifier said of the finding numbered `n`, from 1.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Said {
    pub(crate) n: usize,
    pub(crate) real: bool,
    pub(crate) why: String,
}

#[derive(Deserialize)]
struct RawVerdict {
    n: Value,
    real: bool,
    #[serde(default)]
    why: String,
}

/// The verdicts in a verifier's reply; `None` when it has no verdicts
/// block.
pub(crate) fn parse_verdicts(text: &str) -> Option<Vec<Said>> {
    let lines = block(text, VERDICTS_START, VERDICTS_END)?;
    Some(
        lines
            .into_iter()
            .filter(|line| line.starts_with('{'))
            .filter_map(|line| serde_json::from_str::<RawVerdict>(line).ok())
            .filter_map(|raw| {
                let n = line_of(&raw.n)?;
                Some(Said {
                    n: usize::try_from(n).ok()?,
                    real: raw.real,
                    why: clip(&raw.why, MAX_WHY),
                })
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn subject() -> Subject<'static> {
        Subject {
            project: "acme/widget",
            branch: "login-copy",
            base: "main",
            head: "3f2c1a0aaaaaaaaa",
            merge_base: "1a2b3c4bbbbbbbbb",
        }
    }

    #[test]
    fn a_reviewer_sees_the_whole_stat_and_its_own_diff() {
        let changed = [
            Changed {
                path: "apps/web/copy.ts".into(),
                lines: 12,
            },
            Changed {
                path: "crates/core/lib.rs".into(),
                lines: 30,
            },
        ];
        let shard = Shard {
            area: "apps/web".into(),
            paths: vec!["apps/web/copy.ts".into()],
            lines: 12,
        };
        let prompt = reviewer_prompt(&subject(), &changed, &shard);
        assert!(prompt.contains("read-only review"), "{prompt}");
        assert!(prompt.contains("crates/core/lib.rs (30)"), "{prompt}");
        assert!(
            prompt.contains("git diff 1a2b3c4bbb 3f2c1a0aaa -- apps/web/copy.ts"),
            "{prompt}"
        );
        assert!(prompt.contains(FINDINGS_START) && prompt.contains(FINDINGS_END));
        assert!(!prompt.contains('\u{2014}'));
    }

    #[test]
    fn findings_are_read_from_the_last_block() {
        let reply = "I read the diff.\n\
            BEEP-FINDINGS\nan example\nEND-BEEP-FINDINGS\n\
            Here they are:\n```\nBEEP-FINDINGS\n\
            {\"severity\": \"HIGH\", \"title\": \"Says which field is wrong\", \"file\": \"apps/web/reset.ts\", \"line\": 18, \"why\": \"Tells someone the email exists\"}\n\
            {\"severity\": \"moderate\", \"title\": \"Counter resets\", \"file\": \"crates/core/limit.rs:72\", \"line\": null, \"why\": \"On every attempt\"}\n\
            {\"severity\": \"low\", \"title\": \"\", \"file\": \"x\"}\n\
            not json\n\
            END-BEEP-FINDINGS\n```\n";
        let found = parse_findings(reply).unwrap();
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].severity, ReviewSeverity::High {});
        assert_eq!(found[0].line, Some(18));
        assert_eq!(found[1].severity, ReviewSeverity::Medium {});
        assert_eq!(found[1].file, "crates/core/limit.rs");
        assert_eq!(found[1].line, Some(72));
        assert_eq!(
            parse_findings("BEEP-FINDINGS\nEND-BEEP-FINDINGS"),
            Some(Vec::new())
        );
        assert_eq!(parse_findings("Nothing to say."), None);
    }

    #[test]
    fn verdicts_name_findings_by_number() {
        let finding = ReviewFindingRow {
            id: 4,
            shard: 0,
            severity: ReviewSeverity::Low {},
            title: "Unused import".into(),
            file: "src/limit.rs".into(),
            line: Some(3),
            why: "Dead code".into(),
            verdict: None,
            verdict_why: None,
            dismissed: false,
        };
        let prompt = verifier_prompt(&subject(), &[finding]);
        assert!(
            prompt.contains("1. [low] Unused import at src/limit.rs:3"),
            "{prompt}"
        );
        let said = parse_verdicts(
            "BEEP-VERDICTS\n{\"n\": 1, \"real\": false, \"why\": \"A macro uses it\"}\n{\"n\": \"2\", \"real\": true}\nEND-BEEP-VERDICTS",
        )
        .unwrap();
        assert_eq!(
            said,
            [
                Said {
                    n: 1,
                    real: false,
                    why: "A macro uses it".into()
                },
                Said {
                    n: 2,
                    real: true,
                    why: String::new()
                }
            ]
        );
        assert_eq!(parse_verdicts("no block"), None);
    }
}
