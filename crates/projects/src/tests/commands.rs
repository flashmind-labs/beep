use super::{host::*, parse::*, *};
use crate::model::{IssueHost, RepoName};
use chrono::{DateTime, Utc};
use std::str::FromStr;

fn repo() -> RepoName {
    RepoName::from_str("acme/widget").expect("a repo")
}

fn github() -> IssueHost {
    IssueHost::Github
}

fn forgejo() -> IssueHost {
    IssueHost::Forgejo(crate::model::ForgejoHost::from_str("git.example.com").expect("a host"))
}

#[test]
fn can_push_reads_gh_and_teacli_output() {
    assert!(parse_can_push("true\n").expect("gh says true"));
    assert!(!parse_can_push("false").expect("gh says false"));
    let read_only = r#"{"permissions":{"admin":false,"push":false,"pull":true}}"#;
    assert!(!parse_can_push(read_only).expect("teacli prints the repository"));
    let admin = r#"{"permissions":{"admin":true,"push":false,"pull":true}}"#;
    assert!(parse_can_push(admin).expect("an admin can write"));
    assert!(parse_can_push("null").is_err());
    assert!(parse_can_push("HTTP 404").is_err());
    let gh = HostCommand::CanPush.render(&github(), &repo());
    assert!(gh.contains("gh api repos/acme/widget"), "{gh}");
    let tea = HostCommand::CanPush.render(&forgejo(), &repo());
    assert!(tea.contains("teacli api repos/acme/widget"), "{tea}");
}

#[test]
fn pr_heads_parse_from_gh_and_teacli() {
    let head = |number, branch: &str, fork| PrHead {
        number,
        branch: branch.into(),
        fork,
    };
    let gh = r#"{"number":41,"headRefName":"login-copy","isCrossRepository":false}"#;
    assert_eq!(
        parse_pr_head(gh).expect("a head"),
        head(41, "login-copy", false)
    );
    let gh = r#"{"number":43,"headRefName":"main","isCrossRepository":true}"#;
    assert_eq!(parse_pr_head(gh).expect("a head"), head(43, "main", true));
    let tea = r#"{"number":5,"head":{"ref":"faster-start","repo":{"full_name":"acme/widget"}},
        "base":{"ref":"main","repo":{"full_name":"acme/widget"}}}"#;
    assert_eq!(
        parse_pr_head(tea).expect("a head"),
        head(5, "faster-start", false)
    );
    let tea = r#"{"number":6,"head":{"ref":"fix","repo":{"full_name":"kim/widget"}},
        "base":{"ref":"main","repo":{"full_name":"acme/widget"}}}"#;
    assert_eq!(parse_pr_head(tea).expect("a head"), head(6, "fix", true));
    let gone = r#"{"number":7,"head":{"ref":"fix","repo":null},
        "base":{"ref":"main","repo":{"full_name":"acme/widget"}}}"#;
    assert!(parse_pr_head(gone).expect("a head").fork);
    assert!(parse_pr_head(r#"{"number":8}"#).is_err());
    assert!(parse_pr_head("gh: not found").is_err());
}

/// teacli prints `null` when a repository has none.
#[test]
fn an_empty_list_from_teacli_reads_as_empty() {
    assert!(parse_open_prs("null\n").unwrap().is_empty());
    assert!(parse_open_issues("null\n").unwrap().is_empty());
}

#[test]
fn open_prs_parse_from_gh_and_teacli() {
    let at = |text: &str| {
        DateTime::parse_from_rfc3339(text)
            .expect("a test time")
            .with_timezone(&Utc)
    };
    let gh = r#"[{"number":7,"title":"Bump serde","url":"https://github.com/acme/widget/pull/7",
        "author":"dependabot[bot]","isDraft":false,"createdAt":"2026-10-30T08:00:00Z",
        "updatedAt":"2026-10-31T09:00:00Z","headRefName":"bump-serde","labels":["deps"]}]"#;
    assert_eq!(
        parse_open_prs(gh).expect("prs"),
        [OpenPr {
            number: 7,
            title: "Bump serde".into(),
            url: "https://github.com/acme/widget/pull/7".into(),
            author: "dependabot[bot]".into(),
            draft: false,
            created_at: at("2026-10-30T08:00:00Z"),
            updated_at: at("2026-10-31T09:00:00Z"),
            head: "bump-serde".into(),
            labels: vec!["deps".into()],
        }]
    );
    let tea = r#"[
        {"number":5,"title":"Faster start","user":{"login":"kim"},"draft":true,
         "created_at":"2026-10-29T10:00:00+02:00","updated_at":"2026-10-29T12:00:00+02:00",
         "url":"https://git.example.com/api/v1/repos/acme/widget/pulls/5",
         "html_url":"https://git.example.com/acme/widget/pulls/5","body":"Long text",
         "labels":[{"name":"perf"}],
         "head":{"ref":"faster-start","repo":{"full_name":"acme/widget"}}},
        {"number":6,"title":"No draft field","user":{"login":"sam"},
         "created_at":"2026-10-28T10:00:00Z",
         "html_url":"https://git.example.com/acme/widget/pulls/6"}
    ]"#;
    let prs = parse_open_prs(tea).expect("prs");
    assert_eq!(prs.len(), 2, "{prs:?}");
    assert_eq!(prs[0].author, "kim");
    assert!(prs[0].draft);
    assert_eq!(prs[0].head, "faster-start");
    assert_eq!(prs[0].labels, ["perf"]);
    assert!(prs[1].labels.is_empty());
    assert_eq!(prs[1].head, "");
    assert_eq!(prs[0].url, "https://git.example.com/acme/widget/pulls/5");
    assert_eq!(prs[0].updated_at, at("2026-10-29T10:00:00Z"));
    assert!(!prs[1].draft);
    assert_eq!(prs[1].updated_at, prs[1].created_at);
    assert!(parse_open_prs("[]").expect("none").is_empty());
    assert!(parse_open_prs("gh: not logged in").is_err());
    assert!(parse_open_prs(r#"[{"number":1,"title":"x"}]"#).is_err());
}

#[test]
fn open_issues_parse_from_gh_and_teacli() {
    let gh = r#"[{"number":12,"title":"Crash on start","labels":["bug"],
        "author":"sam","createdAt":"2026-10-30T08:00:00Z","comments":2,
        "url":"https://github.com/acme/widget/issues/12"}]"#;
    let issues = parse_open_issues(gh).expect("issues");
    assert_eq!(
        issues,
        [OpenIssue {
            number: 12,
            title: "Crash on start".into(),
            labels: vec!["bug".into()],
            author: "sam".into(),
            opened_at: DateTime::parse_from_rfc3339("2026-10-30T08:00:00Z")
                .expect("a time")
                .with_timezone(&Utc),
            comments: 2,
            url: "https://github.com/acme/widget/issues/12".into(),
        }]
    );
    let tea = r#"[
        {"number":4,"title":"Slow","labels":[{"name":"perf"}],"user":{"login":"kim"},
         "created_at":"2026-10-29T10:00:00+02:00","comments":0,
         "url":"https://git.example.com/api/v1/repos/acme/widget/issues/4",
         "html_url":"https://git.example.com/acme/widget/issues/4","pull_request":null},
        {"number":5,"title":"A PR","labels":[],"user":{"login":"kim"},
         "created_at":"2026-10-29T10:00:00Z","comments":0,
         "html_url":"https://git.example.com/acme/widget/pulls/5",
         "pull_request":{"merged":false}}
    ]"#;
    let issues = parse_open_issues(tea).expect("issues");
    assert_eq!(issues.len(), 1, "{issues:?}");
    assert_eq!(issues[0].author, "kim");
    assert_eq!(issues[0].labels, ["perf"]);
    assert_eq!(
        issues[0].url,
        "https://git.example.com/acme/widget/issues/4"
    );
    assert!(parse_open_issues("gh: not logged in").is_err());
}

#[test]
fn gh_runs_as_the_account_for_the_owner() {
    let repo = RepoName::from_str("octo-two/tool").unwrap();
    assert_eq!(
        HostCommand::CommentPr {
            number: 1,
            body: "hi"
        }
        .render(&github(), &repo),
        "GH_TOKEN=\"${BEEP_GH_TOKEN_OCTO_TWO:-$GH_TOKEN}\" gh pr comment 1 -R octo-two/tool --body hi"
    );
    // A name GitHub would not give an owner gets no account var.
    let odd = RepoName::from_str("my.org/tool").unwrap();
    assert_eq!(
        HostCommand::CommentPr {
            number: 1,
            body: "hi"
        }
        .render(&github(), &odd),
        "gh pr comment 1 -R my.org/tool --body hi"
    );
    // Forgejo is untouched.
    assert!(
        HostCommand::CommentPr {
            number: 1,
            body: "hi"
        }
        .render(&forgejo(), &repo)
        .starts_with("teacli ")
    );
}

#[test]
fn checks_are_failing_pending_passing_or_none() {
    let checks = |rollup: &str| checks_of(Some(&json(rollup).expect("json")));
    let failing = checks(
        r#"[{"conclusion":"SUCCESS","status":"COMPLETED"},
            {"conclusion":"FAILURE","status":"COMPLETED"},
            {"conclusion":"","status":"IN_PROGRESS"}]"#,
    );
    assert_eq!(failing, ChecksState::Failing);
    let passing = checks(
        r#"[{"conclusion":"SUCCESS","status":"COMPLETED"},
            {"conclusion":"SKIPPED","status":"COMPLETED"}]"#,
    );
    assert_eq!(passing, ChecksState::Passing);
    assert_eq!(
        checks(r#"[{"conclusion":"SUCCESS"},{"conclusion":"","status":"QUEUED"}]"#),
        ChecksState::Pending
    );
    assert_eq!(checks("[]"), ChecksState::None);
    assert_eq!(checks_of(None), ChecksState::None);
}
