use super::{host::*, parse::*, *};
use crate::{
    leak::PublicText,
    model::{IssueHost, RepoName, Visibility},
};
use chrono::{DateTime, Utc};

fn repo() -> RepoName {
    RepoName::parse("acme/widget").expect("a repo")
}

fn github() -> IssueHost {
    IssueHost::Github
}

fn forgejo() -> IssueHost {
    IssueHost::Forgejo(crate::model::ForgejoHost::parse("git.example.com").expect("a host"))
}

fn setup_out(remote: &str, issues: &str, prs: &str, head: &str) -> String {
    format!(
        "== remotes\nremote.origin.url {remote}\n== config\nissues={issues}\nprs={prs}\n\
         == head\n{head}\n"
    )
}

#[test]
fn setup_reads_roles_from_config_then_remotes() {
    let out = setup_out(
        "https://git.example.com/Acme/Widget",
        "forgejo",
        "",
        "origin/trunk",
    );
    let setup = parse_setup(&out, &repo()).expect("a setup");
    assert_eq!(setup.roles.issues, Some(forgejo()));
    assert_eq!(setup.roles.prs, Some(forgejo()));
    assert_eq!(setup.default_branch, "trunk");

    let out = setup_out("git@github.com:acme/widget.git", "", "none", "");
    let setup = parse_setup(&out, &repo()).expect("a setup");
    assert_eq!(setup.roles.issues, Some(github()));
    assert_eq!(setup.roles.prs, None);
    assert_eq!(setup.default_branch, "main");

    let out = setup_out(
        "ssh://git@git.example.com:2222/acme/widget.git",
        "github",
        "",
        "",
    );
    let setup = parse_setup(&out, &repo()).expect("a setup");
    assert_eq!(setup.roles.issues, Some(github()));
    assert_eq!(setup.roles.prs, Some(forgejo()));
}

#[test]
fn setup_reads_a_forgejo_remote_under_a_sub_path() {
    let out = setup_out("https://git.example.com/forge/acme/widget.git", "", "", "");
    let setup = parse_setup(&out, &repo()).expect("a setup");
    assert_eq!(setup.roles.issues, Some(forgejo()));
}

#[test]
fn setup_refuses_another_repo() {
    let out = setup_out("git@github.com:acme/other.git", "", "", "origin/main");
    assert_eq!(
        parse_setup(&out, &repo()),
        Err(SetupError::OtherRepo("acme/widget".into()))
    );
    let out = setup_out("git@github.com:acme/widget.git", "svn", "", "");
    assert_eq!(
        parse_setup(&out, &repo()),
        Err(SetupError::BadConfig("svn".into()))
    );
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
fn a_viewed_issue_parses_and_a_pr_is_refused() {
    let gh = r#"{"number":12,"title":"Crash","body":"Steps","state":"OPEN",
        "author":{"login":"sam"},"url":"https://github.com/acme/widget/issues/12"}"#;
    let issue = parse_viewed_issue(gh).expect("an issue");
    assert!(issue.open);
    assert_eq!(issue.author, "sam");
    assert_eq!(issue.body, "Steps");
    let pr = r#"{"number":5,"title":"PR","body":"","state":"open","pull_request":{}}"#;
    assert!(parse_viewed_issue(pr).is_err());
}

#[test]
fn gh_runs_as_the_account_for_the_owner() {
    let repo = RepoName::parse("octo-two/tool").unwrap();
    assert_eq!(
        HostCommand::MergePr { number: 1 }.render(&github(), &repo),
        "GH_TOKEN=\"${BEEP_GH_TOKEN_OCTO_TWO:-$GH_TOKEN}\" gh pr merge 1 -R octo-two/tool --squash"
    );
    // A name GitHub would not give an owner gets no account var.
    let odd = RepoName::parse("my.org/tool").unwrap();
    assert_eq!(
        HostCommand::MergePr { number: 1 }.render(&github(), &odd),
        "gh pr merge 1 -R my.org/tool --squash"
    );
    // Forgejo is untouched.
    assert!(
        HostCommand::MergePr { number: 1 }
            .render(&forgejo(), &repo)
            .starts_with("teacli ")
    );
}

#[test]
fn text_for_a_host_is_checked() {
    let public_text = PublicText::new(&["build-box".to_owned()], &[]);
    let create = HostCommand::CreateIssue {
        title: "Fix",
        body: "Failed on build-box",
    };
    assert_eq!(create.leaks(&public_text), ["build-box"]);
    assert!(
        HostCommand::ReopenIssue { number: 1 }
            .leaks(&public_text)
            .is_empty()
    );
}

fn gh_pr(rollup: &str, review: &str, merged_at: &str) -> String {
    format!(
        r#"{{"number":9,"state":"OPEN","mergedAt":{merged_at},"reviewDecision":"{review}",
        "statusCheckRollup":{rollup},"url":"https://github.com/acme/widget/pull/9",
        "title":"Fix","body":"Body"}}"#
    )
}

#[test]
fn pr_state_parses_checks() {
    let pr = |rollup: &str| {
        parse_pr(&github(), &gh_pr(rollup, "APPROVED", "null"))
            .expect("json")
            .expect("a pr")
    };
    let failing = pr(r#"[{"conclusion":"SUCCESS","status":"COMPLETED"},
                        {"conclusion":"FAILURE","status":"COMPLETED"},
                        {"conclusion":"","status":"IN_PROGRESS"}]"#);
    assert_eq!(failing.checks, ChecksState::Failing);
    let passing = pr(r#"[{"conclusion":"SUCCESS","status":"COMPLETED"},
                        {"conclusion":"SKIPPED","status":"COMPLETED"}]"#);
    assert_eq!(passing.checks, ChecksState::Passing);
    assert_eq!(
        pr(r#"[{"conclusion":"SUCCESS"},{"conclusion":"","status":"QUEUED"}]"#).checks,
        ChecksState::Pending
    );
    assert_eq!(pr("[]").checks, ChecksState::None);
    assert_eq!(passing.review.as_deref(), Some("APPROVED"));
    assert!(passing.open && !passing.merged);
    assert_eq!(passing.number, 9);
    assert_eq!(passing.title, "Fix");
}

#[test]
fn pr_state_reads_merges_lists_and_forgejo() {
    let merged = parse_pr(&github(), &gh_pr("[]", "", r#""2026-01-02T03:04:05Z""#))
        .expect("json")
        .expect("a pr");
    assert!(merged.merged);
    assert_eq!(merged.review, None);
    assert_eq!(parse_pr(&github(), "[]").expect("json"), None);
    assert_eq!(parse_pr(&github(), "").expect("empty"), None);
    let listed = format!("[{}]", gh_pr("[]", "", "null"));
    assert_eq!(
        parse_pr(&github(), &listed)
            .expect("json")
            .map(|pr| pr.number),
        Some(9)
    );
    let tea = r#"{"number":2,"state":"closed","merged":true,
        "html_url":"https://git.example.com/acme/widget/pulls/2","title":"T","body":"B"}"#;
    let pr = parse_pr(&forgejo(), tea).expect("json").expect("a pr");
    assert!(pr.merged && !pr.open);
    assert_eq!(pr.checks, ChecksState::None);
    assert_eq!(pr.url, "https://git.example.com/acme/widget/pulls/2");
    assert!(parse_pr(&forgejo(), r#"{"error":true,"message":"not found"}"#).is_err());
}

#[test]
fn unreadable_visibility_is_public() {
    assert_eq!(
        parse_visibility(&github(), r#"{"visibility":"PRIVATE"}"#),
        Visibility::Private
    );
    assert_eq!(
        parse_visibility(&github(), r#"{"visibility":"PUBLIC"}"#),
        Visibility::Public
    );
    assert_eq!(
        parse_visibility(&forgejo(), r#"{"private":true}"#),
        Visibility::Private
    );
    assert_eq!(
        parse_visibility(&forgejo(), r#"{"private":false}"#),
        Visibility::Public
    );
    for out in ["", "not json", "{}", "[]"] {
        assert_eq!(
            parse_visibility(&github(), out),
            Visibility::Public,
            "{out}"
        );
    }
}

#[test]
fn found_issues_must_hold_the_marker() {
    let out = r#"[{"number": 4, "html_url": "https://git.example.com/acme/widget/issues/4", "body": "x beep-task:2"},
                  {"number": 5, "html_url": "https://git.example.com/acme/widget/issues/5", "body": "y <!-- beep-task:1 -->"}]"#;
    let found = parse_found_issue(out, "beep-task:1").unwrap();
    assert_eq!(found.map(|issue| issue.number), Some(5));
    assert_eq!(parse_found_issue("", "beep-task:1").unwrap(), None);
    assert_eq!(parse_found_issue("[]", "beep-task:1").unwrap(), None);
}
