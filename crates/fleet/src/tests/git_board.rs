use super::*;
use beep_projects::git_items::{access::Repo, host::HostError};
use beep_proto::{GitHostKind, GitItem, GitItemKind, GitReview, ProjectLists};
use std::time::UNIX_EPOCH;

fn host() -> GitHost {
    GitHost {
        host: "github.com".into(),
        kind: GitHostKind::GitHub {},
        account: Some("sam".into()),
        error: None,
    }
}

fn found() -> Found {
    Found {
        item: GitItem {
            host: "github.com".into(),
            repo: "acme/widget".into(),
            number: 9,
            kind: GitItemKind::PullRequest {},
            title: "Fix".into(),
            url: "https://github.com/acme/widget/pull/9".into(),
            updated_at: UNIX_EPOCH + Duration::from_secs(30),
            authored: true,
            assigned: false,
            review_requested: false,
            draft: false,
            review: GitReview::None {},
            checks_failing: false,
            comments: 0,
        },
        labels: vec!["ready".into()],
        opened_at: Some(UNIX_EPOCH + Duration::from_secs(20)),
    }
}

fn one_host_kept(key: &str) -> GitKept {
    let mut kept = GitKept {
        key: key.into(),
        hosts: vec![None],
        ..GitKept::default()
    };
    let answer = Answer {
        host: host(),
        result: Ok(vec![found()]),
    };
    kept.take_host(0, Some(answer));
    kept.access.writable.insert(Repo {
        host: "github.com".into(),
        name: "acme/widget".into(),
    });
    kept
}

#[test]
fn a_failed_search_keeps_what_it_found_and_says_why() {
    let mut kept = one_host_kept("a");
    let failed = Answer {
        host: host(),
        result: Err(HostError::Timeout("github.com".into())),
    };
    kept.take_host(0, Some(failed));
    let items = kept.items();
    assert_eq!(items.items.found.items.len(), 1);
    assert!(items.items.found.hosts[0].error.is_some());
    assert!(
        kept.saved().items().items.found.hosts[0].error.is_none(),
        "an error is not kept"
    );
}

#[test]
fn a_failed_project_read_keeps_the_lists_it_had() {
    let mut kept = GitKept::default();
    let lists = LabeledProjectLists {
        lists: ProjectLists {
            pull_requests: Vec::new(),
            prs_error: None,
            issues: Vec::new(),
            issues_error: None,
            synced_at: None,
        },
        pr_labels: Vec::new(),
    };
    kept.take_project(ProjectRead {
        name: "acme/widget".into(),
        result: Ok(lists),
    });
    kept.take_project(ProjectRead {
        name: "acme/widget".into(),
        result: Err("no answer in time".into()),
    });
    assert!(kept.projects.contains_key("acme/widget"));
    assert!(kept.saved().failed.is_empty());
    assert_eq!(kept.items().items.found.hosts.len(), 1);
}

#[test]
fn what_was_kept_comes_back_only_for_the_same_key() {
    let kept = one_host_kept("a");
    // No logins now, so the hosts do not line up and nothing is taken.
    assert!(GitKept::resumed(Some(kept.clone()), "a".into(), &[], &[]).is_empty());
    let mut none = kept;
    none.hosts.clear();
    assert!(GitKept::resumed(Some(none.clone()), "a".into(), &[], &[]).is_empty());
    assert!(GitKept::resumed(Some(none), "b".into(), &[], &[]).is_empty());
}

#[test]
fn what_is_kept_survives_json() {
    let kept = one_host_kept("0123456789abcdef");
    let text = serde_json::to_string(&kept).expect("it serializes");
    let back: GitKept = serde_json::from_str(&text).expect("it reads back");
    assert_eq!(back.items(), kept.items());
    assert!(back.access.writable.contains(&Repo {
        host: "github.com".into(),
        name: "acme/widget".into(),
    }));
}
