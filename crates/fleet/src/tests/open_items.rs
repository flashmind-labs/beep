use super::*;
use beep_proto::{ProjectIssue, ProjectLists};

/// Every repository writable, as for the user's own projects.
fn open_access() -> Access {
    Access {
        writable: HashSet::from([Repo {
            host: "forge.example.com".into(),
            name: "acme/widget".into(),
        }]),
        accounts: HashMap::new(),
    }
}
use std::time::SystemTime;

fn at(secs: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(secs)
}

fn issue(number: u64, opened: u64) -> ProjectIssue {
    ProjectIssue {
        number,
        title: format!("issue {number}"),
        labels: vec!["bug".into()],
        author: "someone".into(),
        opened_at: at(opened),
        comments: 3,
        url: format!("https://forge.example.com/acme/widget/issues/{number}"),
        linked_pr: false,
    }
}

fn pr(number: u64, created: u64, updated: u64, mine: bool) -> ProjectPullRequest {
    ProjectPullRequest {
        number,
        title: format!("pr {number}"),
        url: format!("https://forge.example.com/acme/widget/pull/{number}"),
        author: "someone".into(),
        draft: false,
        mine,
        created_at: at(created),
        updated_at: at(updated),
    }
}

fn read(name: &str, issues: Vec<ProjectIssue>, prs: Vec<ProjectPullRequest>) -> ProjectRead {
    ProjectRead {
        name: name.into(),
        result: Ok(LabeledProjectLists {
            pr_labels: prs.iter().map(|_| vec!["ready".to_owned()]).collect(),
            lists: ProjectLists {
                pull_requests: prs,
                prs_error: None,
                issues,
                issues_error: None,
                synced_at: None,
            },
        }),
    }
}

fn empty() -> OpenedItems {
    OpenedItems {
        items: LabeledGitItems {
            found: GitItems {
                items: Vec::new(),
                hosts: Vec::new(),
            },
            labels: Vec::new(),
        },
        opened_at: Vec::new(),
    }
}

fn token_pr() -> OpenedItems {
    let mut item = row(
        "https://forge.example.com/acme/widget/pull/9",
        9,
        GitItemKind::PullRequest {},
    )
    .expect("a web address");
    item.title = "pr 9".into();
    item.updated_at = at(50);
    item.review_requested = true;
    item.review = GitReview::Approved {};
    let mut tokens = empty();
    tokens.items.found.items.push(item);
    tokens.items.labels.push(Vec::new());
    tokens.opened_at.push(Some(at(40)));
    tokens
}

#[test]
fn projects_fill_in_what_no_token_gave() {
    let got = merge(
        empty(),
        vec![read(
            "acme/widget",
            vec![issue(7, 10)],
            vec![pr(9, 20, 30, true)],
        )],
        &open_access(),
    );
    let items = &got.items.found.items;
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].number, 9, "newest activity first");
    assert!(items[0].authored && !items[0].review_requested);
    assert_eq!(items[0].host, "forge.example.com");
    assert_eq!(items[0].repo, "acme/widget");
    assert_eq!(got.items.labels[0], ["ready"]);
    assert_eq!(items[1].kind, GitItemKind::Issue {});
    assert_eq!(items[1].comments, 3);
    assert_eq!(got.opened_at, [Some(at(20)), Some(at(10))]);
    assert!(got.items.found.hosts.is_empty());
}

#[test]
fn a_row_both_gave_stays_the_tokens_with_the_earliest_opening() {
    let got = merge(
        token_pr(),
        vec![read("acme/widget", Vec::new(), vec![pr(9, 20, 30, false)])],
        &open_access(),
    );
    let items = &got.items.found.items;
    assert_eq!(items.len(), 1);
    assert!(items[0].review_requested);
    assert_eq!(items[0].review, GitReview::Approved {});
    assert_eq!(items[0].updated_at, at(50));
    assert_eq!(got.opened_at, [Some(at(20))]);
}

#[test]
fn a_project_that_fails_says_why_and_the_others_still_list() {
    let got = merge(
        token_pr(),
        vec![
            ProjectRead {
                name: "acme/gone".into(),
                result: Err("no answer in time".into()),
            },
            read("acme/widget", vec![issue(7, 10)], Vec::new()),
        ],
        &open_access(),
    );
    assert_eq!(got.items.found.items.len(), 2);
    let hosts = &got.items.found.hosts;
    assert_eq!(hosts.len(), 1);
    assert_eq!(
        hosts[0].error.as_deref(),
        Some("acme/gone: no answer in time")
    );
}

#[test]
fn both_lists_failing_makes_the_project_unreadable() {
    let lists = LabeledProjectLists {
        pr_labels: Vec::new(),
        lists: ProjectLists {
            pull_requests: Vec::new(),
            prs_error: Some("not signed in".into()),
            issues: Vec::new(),
            issues_error: Some("not signed in".into()),
            synced_at: None,
        },
    };
    assert_eq!(unreadable(&lists).as_deref(), Some("not signed in"));
}

/// An upstream the user only forked: someone else's PR and issue, and the user's own PR.
fn upstream() -> ProjectRead {
    read(
        "acme/widget",
        vec![issue(7, 10)],
        vec![pr(4, 5, 6, false), pr(9, 20, 30, true)],
    )
}

fn numbers(items: &OpenedItems) -> Vec<u64> {
    let mut numbers: Vec<u64> = items.items.found.items.iter().map(|i| i.number).collect();
    numbers.sort_unstable();
    numbers
}

#[test]
fn a_repository_the_user_cannot_write_to_adds_only_their_own() {
    let none = Access::default();
    let got = merge(empty(), vec![upstream()], &none);
    assert_eq!(numbers(&got), [9]);
    assert_eq!(got.opened_at.len(), 1);
}

#[test]
fn a_writable_repository_keeps_all_its_open_items() {
    let got = merge(empty(), vec![upstream()], &open_access());
    assert_eq!(numbers(&got), [4, 7, 9]);
}

#[test]
fn the_token_account_marks_an_issue_as_the_users() {
    let mut access = Access::default();
    access
        .accounts
        .insert("forge.example.com".into(), vec!["Someone".into()]);
    let got = merge(empty(), vec![upstream()], &access);
    assert_eq!(numbers(&got), [4, 7, 9], "the fixtures are all by someone");
    access.accounts.clear();
    let got = merge(empty(), vec![upstream()], &access);
    assert_eq!(
        numbers(&got),
        [9],
        "no known account keeps only the machine's mine"
    );
}

#[test]
fn what_the_tokens_gave_stays_when_the_repository_is_filtered() {
    let got = merge(token_pr(), vec![upstream()], &Access::default());
    assert_eq!(numbers(&got), [9]);
    assert!(got.items.found.items[0].review_requested);
}

#[test]
fn the_repositories_to_check_are_those_of_the_items() {
    let repos = repos_of(&[upstream()]);
    assert_eq!(repos.len(), 1);
    assert!(repos.contains(&Repo {
        host: "forge.example.com".into(),
        name: "acme/widget".into(),
    }));
}
