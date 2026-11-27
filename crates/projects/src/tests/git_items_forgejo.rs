use super::*;

fn issues(json: &str) -> Vec<Issue> {
    serde_json::from_str(json).expect("the fixture is a list of issues")
}

const PULLS: &str = r#"[
      {"id": 901, "number": 57, "title": "WIP: Stream large files in chunks",
       "html_url": "https://git.example.com/tools-org/uploader/pulls/57",
       "state": "open", "comments": 2, "updated_at": "2026-10-28T12:00:00+01:00",
       "user": {"login": "some-user"},
       "labels": [{"id": 3, "name": "perf", "color": "ee0701"}],
       "pull_request": {"merged": false, "merged_at": null},
       "repository": {"id": 4, "name": "uploader", "owner": "tools-org",
                      "full_name": "tools-org/uploader"}}
    ]"#;

const REVIEW: &str = r#"[
      {"number": 61, "title": "Drop the old v1 endpoints",
       "html_url": "https://git.example.com/tools-org/uploader/pulls/61",
       "comments": 0, "updated_at": "2026-10-31T08:00:00Z",
       "pull_request": {"merged": false, "draft": true},
       "repository": {"full_name": "tools-org/uploader"}}
    ]"#;

const ISSUES: &str = r#"[
      {"number": 48, "title": "Resume after the laptop sleeps",
       "html_url": "https://git.example.com/tools-org/uploader/issues/48",
       "comments": 1, "updated_at": "2026-10-20T08:00:00Z", "pull_request": null,
       "repository": {"full_name": "tools-org/uploader"}}
    ]"#;

#[test]
fn an_issue_carries_its_created_time() {
    let listed = issues(
        r#"[{"number": 3, "title": "t", "html_url": "https://git.example.com/o/r/issues/3",
                 "updated_at": "2026-10-30T09:15:00Z", "created_at": "2026-10-01T10:00:00+02:00",
                 "repository": {"full_name": "o/r"}}]"#,
    );
    let items = merge("git.example.com", vec![(Query::AuthoredIssues, listed)]);
    let created = DateTime::parse_from_rfc3339("2026-10-01T08:00:00Z")
        .expect("a valid time")
        .with_timezone(&Utc);
    assert_eq!(items[0].opened_at, Some(SystemTime::from(created)));
}

#[test]
fn issues_parse_with_their_repository() {
    let items = merge(
        "git.example.com",
        vec![
            (Query::AuthoredPulls, issues(PULLS)),
            (Query::ReviewRequested, issues(REVIEW)),
            (Query::AuthoredIssues, issues(ISSUES)),
            (Query::AssignedIssues, issues(ISSUES)),
            (Query::AssignedPulls, issues(REVIEW)),
        ],
    );
    assert_eq!(items.len(), 3);
    assert_eq!(items[0].labels, ["perf"]);
    assert!(items[2].labels.is_empty());
    let pr = &items[0].item;
    assert_eq!(pr.host, "git.example.com");
    assert_eq!(pr.repo, "tools-org/uploader");
    assert_eq!(pr.kind, GitItemKind::PullRequest {});
    assert!(pr.authored && pr.draft, "a WIP title marks a draft");
    assert_eq!(pr.comments, 2);
    let review = &items[1].item;
    assert!(review.review_requested && review.draft && !review.authored && review.assigned);
    let issue = &items[2].item;
    assert_eq!(issue.kind, GitItemKind::Issue {});
    assert!(issue.authored && issue.assigned && !issue.draft);
}
