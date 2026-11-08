use super::*;
use beep_proto::{ReviewChecks, ReviewSeverity};
use std::time::UNIX_EPOCH;

#[test]
fn a_review_reads_plainly() {
    let view = ReviewView::from(BranchReview {
        id: Uuid::nil(),
        project: "acme/widget".into(),
        branch: "login-copy".into(),
        base: "main".into(),
        head: "3f2c1a0".into(),
        head_at: UNIX_EPOCH,
        started_at: UNIX_EPOCH,
        machine_name: "build-box".into(),
        changed_lines: 412,
        files: 9,
        merge: ReviewMerge::Conflicts {
            files: vec!["Cargo.toml".into()],
        },
        pr: Some(ReviewPr {
            number: 41,
            url: "https://example.com/pull/41".into(),
            checks: ReviewChecks::Failing {},
        }),
        shards: vec![ReviewShard {
            area: "apps/web".into(),
            files: 6,
            lines: 268,
            agent: "claude".into(),
            state: ReviewShardState::Reading {
                doing: "Read Login.tsx".into(),
            },
            found: 0,
        }],
        findings: vec![ReviewFinding {
            id: 3,
            area: "apps/web".into(),
            severity: ReviewSeverity::Medium {},
            title: "Unused import".into(),
            file: "src/limit.rs".into(),
            line: None,
            why: "Dead".into(),
            verdict: ReviewVerdict::Dropped {
                why: "A macro uses it".into(),
            },
            dismissed: false,
        }],
    });
    assert_eq!(view.merge, "conflicts");
    assert_eq!(view.conflicts, ["Cargo.toml"]);
    assert_eq!(view.pr.as_ref().map(|pr| pr.checks), Some("failing"));
    assert_eq!(view.shards[0].state, "reading");
    assert_eq!(view.shards[0].note, "Read Login.tsx");
    assert_eq!(view.findings[0].severity, "medium");
    assert_eq!(view.findings[0].verdict, "dropped");
    assert_eq!(view.findings[0].dropped_why, "A macro uses it");
}
