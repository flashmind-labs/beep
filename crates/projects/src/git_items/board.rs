//! Asking every git host a user has a token for at once, and putting the answers together.

use super::{Caller, HostError, forgejo, github, host::Found};
use crate::{credentials::Slot, imports::hosts::Hosts};
use beep_proto::{GitHost, GitHostKind, GitItem, GitItems, GitReview, LabeledGitItems};
use futures::future::join_all;
use reqwest::Client;
use secrecy::SecretString;
use std::{cmp::Reverse, collections::HashMap, time::SystemTime};

/// One token to read a host with.
pub struct Login<'a> {
    pub slot: &'a Slot,
    pub secret: &'a SecretString,
    /// Who the token signs in as, when it is known.
    pub account: Option<&'a str>,
}

/// The items and, beside them, when each was opened. The times are not on the wire: `GitItem` is
/// read by position by released apps, so a time travels next to the reply instead of in it.
#[derive(Debug, Clone, PartialEq)]
pub struct OpenedItems {
    pub items: LabeledGitItems,
    /// When each of `items.found.items` was opened, in its order; `None` when the host did not say.
    pub opened_at: Vec<Option<SystemTime>>,
}

/// Asks every host at once and puts their answers together.
pub async fn read(hosts: &dyn Hosts, logins: &[Login<'_>]) -> anyhow::Result<OpenedItems> {
    let client = hosts.client()?;
    let client = &client;
    let answers = join_all(logins.iter().map(|login| ask(hosts, client, login))).await;
    Ok(combine(answers.into_iter().flatten().collect()))
}

/// Asks the host `login` is for; `None` for a credential that is no git host.
pub async fn ask(hosts: &dyn Hosts, client: &Client, login: &Login<'_>) -> Option<Answer> {
    let (host, kind) = match login.slot {
        Slot::GitHub => (github::HOST.to_owned(), GitHostKind::GitHub {}),
        Slot::Forgejo(url) => (url.host(), GitHostKind::Forgejo {}),
        Slot::ApiKey(_) | Slot::AgentToken(_) => return None,
    };
    let caller = Caller {
        client,
        calls: hosts.calls(),
        host: &host,
        token: login.secret,
    };
    let result = match login.slot {
        Slot::Forgejo(url) if !hosts.is_public(url).await => Err(HostError::Private(host.clone())),
        Slot::Forgejo(url) => forgejo::items(&caller, url.as_str()).await,
        _ => github::items(&caller).await,
    };
    Some(Answer {
        host: GitHost {
            host: host.clone(),
            kind,
            account: login.account.map(ToOwned::to_owned),
            error: None,
        },
        result,
    })
}

/// What one host said.
pub struct Answer {
    pub host: GitHost,
    pub result: Result<Vec<Found>, HostError>,
}

/// Every host's error and their items deduplicated, newest activity first.
pub fn combine(answers: Vec<Answer>) -> OpenedItems {
    let mut hosts = Vec::new();
    let mut found: Vec<Found> = Vec::new();
    let mut at: HashMap<String, usize> = HashMap::new();
    for Answer { mut host, result } in answers {
        match result {
            Ok(listed) => {
                for one in listed {
                    match at.get(&one.item.url) {
                        Some(&i) => {
                            let kept = &mut found[i];
                            kept.opened_at = match (kept.opened_at, one.opened_at) {
                                (Some(a), Some(b)) => Some(a.min(b)),
                                (a, b) => a.or(b),
                            };
                            absorb(&mut kept.item, one.item);
                        }
                        None => {
                            at.insert(one.item.url.clone(), found.len());
                            found.push(one);
                        }
                    }
                }
            }
            Err(e) => host.error = Some(e.to_string()),
        }
        hosts.push(host);
    }
    found.sort_by_key(|one| Reverse(one.item.updated_at));
    let mut items = Vec::new();
    let mut labels = Vec::new();
    let mut opened_at = Vec::new();
    for one in found {
        items.push(one.item);
        labels.push(one.labels);
        opened_at.push(one.opened_at);
    }
    OpenedItems {
        items: LabeledGitItems {
            found: GitItems { items, hosts },
            labels,
        },
        opened_at,
    }
}

/// Adds what `other` says about the same item to `item`.
fn absorb(item: &mut GitItem, other: GitItem) {
    item.authored |= other.authored;
    item.assigned |= other.assigned;
    item.review_requested |= other.review_requested;
    item.checks_failing |= other.checks_failing;
    if other.review != (GitReview::None {}) {
        item.review = other.review;
    }
}

#[cfg(test)]
mod tests {
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
}
