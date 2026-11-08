use super::*;

fn reply(json: &str) -> Vec<SearchItem> {
    serde_json::from_str::<SearchReply>(json)
        .expect("the fixture is a search reply")
        .items
}

const AUTHORED: &str = r#"{"total_count": 2, "incomplete_results": false, "items": [
      {"html_url": "https://github.com/acme/widget/pull/142", "number": 142,
       "title": "Retry uploads when the token expires", "state": "open", "draft": false,
       "comments": 3, "updated_at": "2026-10-30T09:15:00Z",
       "labels": [{"id": 1, "name": "bug", "color": "d73a4a"}],
       "repository_url": "https://api.github.com/repos/acme/widget",
       "user": {"login": "some-user"},
       "pull_request": {"url": "https://api.github.com/repos/acme/widget/pulls/142"}},
      {"html_url": "https://github.com/acme/widget/issues/131", "number": 131,
       "title": "Crash when the camera permission is denied", "state": "open",
       "comments": 0, "updated_at": "2026-10-29T08:00:00Z",
       "repository_url": "https://api.github.com/repos/acme/widget",
       "user": {"login": "some-user"}}
    ]}"#;

const ASSIGNED: &str = r#"{"total_count": 2, "items": [
      {"html_url": "https://github.com/acme/widget/issues/131", "number": 131,
       "title": "Crash when the camera permission is denied", "comments": 0,
       "updated_at": "2026-10-29T08:00:00Z",
       "repository_url": "https://api.github.com/repos/acme/widget"},
      {"html_url": "https://github.com/tools-org/uploader/issues/48", "number": 48,
       "title": "Resume after the laptop sleeps", "comments": 1,
       "updated_at": "2026-10-20T08:00:00Z",
       "repository_url": "https://api.github.com/repos/tools-org/uploader"}
    ]}"#;

const REVIEW: &str = r#"{"total_count": 1, "items": [
      {"html_url": "https://github.com/acme/widget/pull/144", "number": 144,
       "title": "Add a dark theme", "draft": true, "comments": 0,
       "updated_at": "2026-10-31T10:00:00Z",
       "repository_url": "https://api.github.com/repos/acme/widget",
       "pull_request": {}}
    ]}"#;

const FAILING: &str = r#"{"total_count": 2, "items": [
      {"html_url": "https://github.com/acme/widget/pull/142", "number": 142, "title": "x",
       "updated_at": "2026-10-30T09:15:00Z",
       "repository_url": "https://api.github.com/repos/acme/widget", "pull_request": {}},
      {"html_url": "https://github.com/other/thing/pull/9", "number": 9, "title": "y",
       "updated_at": "2026-10-30T09:15:00Z",
       "repository_url": "https://api.github.com/repos/other/thing", "pull_request": {}}
    ]}"#;

#[test]
fn an_item_two_searches_find_is_listed_once_with_both_marks() {
    let items = merge(vec![
        (Query::AuthoredIssues, reply(AUTHORED)),
        (Query::Assigned, reply(ASSIGNED)),
        (Query::ReviewRequested, reply(REVIEW)),
    ]);
    assert_eq!(items.len(), 4);
    let pr = items
        .iter()
        .find(|i| i.item.number == 142)
        .expect("the PR is listed");
    assert_eq!(pr.labels, ["bug"]);
    let crash = &items
        .iter()
        .find(|i| i.item.number == 131)
        .expect("the issue is listed")
        .item;
    assert!(crash.authored && crash.assigned);
    let review = &items
        .iter()
        .find(|i| i.item.number == 144)
        .expect("the PR is listed")
        .item;
    assert!(review.review_requested && review.draft && !review.authored);
}

#[test]
fn a_search_hit_carries_its_created_time() {
    let hits = r#"{"items": [
          {"html_url": "https://github.com/acme/widget/issues/7", "number": 7, "title": "t",
           "updated_at": "2026-10-30T09:15:00Z", "created_at": "2026-10-01T08:00:00Z",
           "repository_url": "https://api.github.com/repos/acme/widget"},
          {"html_url": "https://github.com/acme/widget/issues/8", "number": 8, "title": "u",
           "updated_at": "2026-10-30T09:15:00Z",
           "repository_url": "https://api.github.com/repos/acme/widget"}
        ]}"#;
    let items = merge(vec![(Query::AuthoredIssues, reply(hits))]);
    let created = DateTime::parse_from_rfc3339("2026-10-01T08:00:00Z")
        .expect("a valid time")
        .with_timezone(&Utc);
    assert_eq!(items[0].opened_at, Some(SystemTime::from(created)));
    assert_eq!(items[1].opened_at, None);
}

#[test]
fn marking_searches_mark_but_never_add() {
    let items = merge(vec![
        (Query::Failing, reply(FAILING)),
        (Query::AuthoredIssues, reply(AUTHORED)),
        (Query::ChangesRequested, reply(FAILING)),
    ]);
    assert_eq!(items.len(), 2);
    assert!(items[0].item.checks_failing);
    assert_eq!(items[0].item.review, GitReview::ChangesRequested {});
    assert!(!items[1].item.checks_failing);
    assert!(items.iter().all(|i| i.item.repo != "other/thing"));
}
