use super::*;
use crate::model::ForgejoHost;
use serde_json::json;
use std::str::FromStr;

#[test]
fn a_github_pull_request_reads_with_reviews_and_checks() {
    let out = json!({
            "number": 41, "title": " Cut the cache ", "body": "Shorter **now**", "url": "https://github.com/acme/widget/pull/41",
            "author": {"login": "sam"}, "state": "OPEN", "isDraft": false,
            "labels": [{"name": "perf"}], "createdAt": "2026-10-30T09:00:00Z",
            "comments": [{"author": {"login": "alice"}, "createdAt": "2026-10-30T11:00:00Z", "body": "<img src=x onerror=alert(1)>"}],
            "reviews": [{"author": {"login": "kim"}, "submittedAt": "2026-10-30T10:00:00Z", "body": "", "state": "CHANGES_REQUESTED"}],
            "reviewDecision": "CHANGES_REQUESTED",
            "statusCheckRollup": [{"conclusion": "FAILURE"}],
        })
        .to_string();
    let d = parse(&IssueHost::Github, &out).unwrap();
    assert_eq!(d.kind, GitItemKind::PullRequest {});
    assert_eq!(d.title, "Cut the cache");
    assert_eq!(d.state, ThreadState::Open {});
    assert_eq!(d.labels, ["perf"]);
    assert_eq!(d.review, GitReview::ChangesRequested {});
    assert_eq!(d.checks, Some(RunState::Failed {}));
    // Oldest first, the review between; text kept as the host wrote it.
    let said: Vec<_> = d
        .comments
        .iter()
        .map(|c| (c.author.as_str(), c.body.as_str()))
        .collect();
    assert_eq!(
        said,
        [
            ("kim", "Requested changes"),
            ("alice", "<img src=x onerror=alert(1)>")
        ]
    );
}

#[test]
fn a_forgejo_pull_request_reads_its_issue_comments() {
    let issue = json!({
        "number": 7, "title": "Fix", "body": "", "html_url": "https://git.example.com/acme/widget/pulls/7",
        "user": {"login": "sam"}, "state": "closed", "labels": [], "created_at": "2026-10-30T09:00:00Z",
        "pull_request": {"merged": true, "draft": false},
    });
    let comments = json!([{"user": {"login": "alice"}, "created_at": "2026-10-30T10:00:00Z", "body": "Thanks"}]);
    let host = IssueHost::Forgejo(ForgejoHost::from_str("git.example.com").unwrap());
    let d = parse(&host, &format!("{issue}\n{comments}\n")).unwrap();
    assert_eq!(d.kind, GitItemKind::PullRequest {});
    assert_eq!(d.state, ThreadState::Merged {});
    assert_eq!(d.url, "https://git.example.com/acme/widget/pulls/7");
    assert_eq!(d.comments.len(), 1);
    assert_eq!(d.comments[0].author, "alice");
}
