use super::*;
use std::time::SystemTime;

/// The keys `value` serializes with, sorted.
fn keys(value: impl Serialize) -> Vec<String> {
    let value = serde_json::to_value(value).expect("serializes");
    let mut keys: Vec<String> = value
        .as_object()
        .expect("an object")
        .keys()
        .cloned()
        .collect();
    keys.sort();
    keys
}

/// `IssueRow` as `threads.ts` reads it: its assignees claim issues for a team.
#[test]
fn an_issue_row_carries_its_assignees() {
    let issue = ProjectIssue {
        number: 52,
        title: "Fees are wrong".to_owned(),
        labels: Vec::new(),
        author: "dana".to_owned(),
        opened_at: SystemTime::UNIX_EPOCH,
        comments: 0,
        url: "https://example.test/org/web/issues/52".to_owned(),
        linked_pr: false,
        assignees: vec!["kim".to_owned()],
    };

    let row = serde_json::to_value(IssueRow::from(issue)).expect("serializes");

    assert_eq!(row["assignees"], serde_json::json!(["kim"]));
    assert_eq!(
        keys(row),
        [
            "assignees",
            "author",
            "comments",
            "labels",
            "linked_pr",
            "number",
            "opened_at",
            "title",
            "url"
        ]
    );
}
