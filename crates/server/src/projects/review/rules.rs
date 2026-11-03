//! What a review allows: open findings and whether a merge may ask.

use super::drive::running;
use crate::{
    protocol::ReviewChecks,
    store::{ReviewFindingRow, ReviewRow, ShardRow, Verdict},
};

/// Why merging is off (running review, open findings, conflicts, failing checks); `None` if not.
pub(crate) fn merge_blocked(
    review: &ReviewRow,
    shards: &[ShardRow],
    findings: &[ReviewFindingRow],
) -> Option<String> {
    if running(shards) {
        return Some("The review is still running.".to_owned());
    }
    let open = open(findings).len();
    if open > 0 {
        return Some(format!(
            "{open} {} open: fix or dismiss {} first.",
            if open == 1 {
                "finding is"
            } else {
                "findings are"
            },
            if open == 1 { "it" } else { "them" },
        ));
    }
    if let Some(files) = review.conflicts.as_ref().filter(|files| !files.is_empty()) {
        return Some(format!(
            "The branch conflicts with {} in {} {}.",
            review.base,
            files.len(),
            if files.len() == 1 { "file" } else { "files" }
        ));
    }
    if review
        .pr
        .as_ref()
        .is_some_and(|pr| pr.checks == ReviewChecks::Failing {})
    {
        return Some("Checks fail on the pull request.".to_owned());
    }
    None
}

/// Findings neither dismissed nor dropped by the verifier, most severe first.
pub(crate) fn open(findings: &[ReviewFindingRow]) -> Vec<&ReviewFindingRow> {
    let mut open: Vec<&ReviewFindingRow> = findings
        .iter()
        .filter(|finding| !finding.dismissed && finding.verdict != Some(Verdict::Dropped))
        .collect();
    open.sort_by_key(|finding| (finding.severity, finding.id));
    open
}
