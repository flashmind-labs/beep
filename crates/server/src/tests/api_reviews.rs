use super::*;
use crate::{projects::review::rules::merge_blocked, store::PrRow};
use chrono::Utc;

fn review() -> ReviewRow {
    ReviewRow {
        id: Uuid::nil(),
        project: "acme/widget".into(),
        branch: "login-copy".into(),
        base: "main".into(),
        endpoint: "node".into(),
        folder: "/srv/project".into(),
        head: "3f2c1a0aaaa".into(),
        merge_base: "1a2b3c4".into(),
        head_at: Utc::now(),
        changed_lines: 412,
        files: 9,
        conflicts: Some(Vec::new()),
        pr: Some(PrRow {
            number: 41,
            url: "https://example.com/acme/widget/pull/41".into(),
            checks: ReviewChecks::Passing {},
        }),
        started_at: Utc::now(),
        worktree: None,
    }
}

fn shard(n: u32, state: ShardState) -> ShardRow {
    ShardRow {
        shard: n,
        area: format!("area{n}"),
        paths: Vec::new(),
        files: 1,
        lines: 10,
        agent: "claude".into(),
        session: format!("s{n}"),
        verifier: None,
        state,
        error: None,
    }
}

fn finding(id: u32, severity: ReviewSeverity, verdict: Option<Verdict>) -> ReviewFindingRow {
    ReviewFindingRow {
        id,
        shard: id / 100,
        severity,
        title: format!("finding {id}"),
        file: "src/lib.rs".into(),
        line: Some(id + 1),
        why: "It breaks".into(),
        verdict,
        verdict_why: None,
        dismissed: false,
    }
}

#[test]
fn merge_refuses_on_failing_ci_conflicts_and_open_findings() {
    let done = [shard(0, ShardState::Done)];
    let open = [finding(0, ReviewSeverity::Low {}, Some(Verdict::Unchecked))];
    let dropped = [finding(0, ReviewSeverity::Low {}, Some(Verdict::Dropped))];
    assert_eq!(merge_blocked(&review(), &done, &dropped), None);
    assert_eq!(
        merge_blocked(&review(), &[shard(0, ShardState::Reading)], &[]).as_deref(),
        Some("The review is still running.")
    );
    assert_eq!(
        merge_blocked(&review(), &done, &open).as_deref(),
        Some("1 finding is open: fix or dismiss it first.")
    );
    let mut conflicts = review();
    conflicts.conflicts = Some(vec!["a".into(), "b".into()]);
    assert_eq!(
        merge_blocked(&conflicts, &done, &[]).as_deref(),
        Some("The branch conflicts with main in 2 files.")
    );
    let mut failing = review();
    if let Some(pr) = failing.pr.as_mut() {
        pr.checks = ReviewChecks::Failing {};
    }
    assert!(merge_blocked(&failing, &done, &[]).is_some());
}
