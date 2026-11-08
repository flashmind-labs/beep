use super::*;
use beep_proto::GitItemKind;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn found(item: GitItem) -> Found {
    Found {
        item,
        labels: Vec::new(),
        opened_at: None,
    }
}

fn item(url: &str, secs: u64) -> GitItem {
    GitItem {
        host: "github.com".into(),
        repo: "acme/widget".into(),
        number: 1,
        kind: GitItemKind::PullRequest {},
        title: "t".into(),
        url: url.into(),
        updated_at: UNIX_EPOCH + Duration::from_secs(secs),
        authored: false,
        assigned: false,
        review_requested: false,
        draft: false,
        review: GitReview::None {},
        checks_failing: false,
        comments: 0,
    }
}

fn host(name: &str, account: &str) -> GitHost {
    GitHost {
        host: name.into(),
        kind: GitHostKind::GitHub {},
        account: Some(account.into()),
        error: None,
    }
}

#[test]
fn a_failing_host_keeps_its_error_and_the_others_list() {
    let ok = item("https://github.com/acme/widget/pull/1", 10);
    let got = combine(vec![
        Answer {
            host: host("github.com", "some-user"),
            result: Ok(vec![Found {
                item: ok.clone(),
                labels: vec!["bug".into()],
                opened_at: None,
            }]),
        },
        Answer {
            host: host("git.example.com", "some-user"),
            result: Err(HostError::Timeout("git.example.com".into())),
        },
    ]);
    let got = got.items;
    assert_eq!(got.found.items, vec![ok]);
    assert_eq!(got.labels, [["bug"]]);
    assert_eq!(got.found.hosts[0].error, None);
    assert_eq!(
        got.found.hosts[1].error.as_deref(),
        Some("git.example.com did not answer in time.")
    );
}

#[test]
fn opened_times_follow_their_items_and_the_earliest_wins() {
    let at = |secs| Some(UNIX_EPOCH + Duration::from_secs(secs));
    let url = "https://github.com/acme/widget/pull/1";
    let other = "https://github.com/acme/widget/pull/2";
    let with = |url, updated, opened| Found {
        opened_at: opened,
        ..found(item(url, updated))
    };
    let got = combine(vec![
        Answer {
            host: host("github.com", "first-login"),
            result: Ok(vec![with(url, 10, at(5)), with(other, 20, None)]),
        },
        Answer {
            host: host("github.com", "second-login"),
            result: Ok(vec![with(url, 10, at(3))]),
        },
    ]);
    assert_eq!(got.items.found.items[0].url, other);
    assert_eq!(got.opened_at, vec![None, at(3)]);
}

#[test]
fn two_accounts_seeing_one_item_list_it_once_newest_first() {
    let url = "https://github.com/acme/widget/pull/1";
    let mine = GitItem {
        authored: true,
        ..item(url, 10)
    };
    let theirs = GitItem {
        review_requested: true,
        review: GitReview::Approved {},
        ..item(url, 10)
    };
    let newer = item("https://github.com/acme/widget/pull/2", 20);
    let got = combine(vec![
        Answer {
            host: host("github.com", "first-login"),
            result: Ok(vec![found(mine)]),
        },
        Answer {
            host: host("github.com", "second-login"),
            result: Ok(vec![found(theirs), found(newer.clone())]),
        },
    ]);
    let got = got.items.found;
    assert_eq!(got.items.len(), 2);
    assert_eq!(got.items[0], newer);
    let merged = &got.items[1];
    assert!(merged.authored && merged.review_requested);
    assert_eq!(merged.review, GitReview::Approved {});
    assert!(got.items[0].updated_at > SystemTime::UNIX_EPOCH);
}
