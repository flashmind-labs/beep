//! Asking every git host a user has a token for at once, and putting the answers together.

use super::{Caller, HostError, forgejo, github, host::Found};
use crate::{credentials::Slot, imports::hosts::Hosts};
use beep_proto::{GitHost, GitHostKind, GitItem, GitItems, GitReview, LabeledGitItems};
use futures::future::join_all;
use secrecy::SecretString;
use std::{cmp::Reverse, collections::HashMap};

/// One token to read a host with.
pub struct Login<'a> {
    pub slot: &'a Slot,
    pub secret: &'a SecretString,
    /// Who the token signs in as, when it is known.
    pub account: Option<&'a str>,
}

/// Asks every host at once and puts their answers together.
pub async fn read(hosts: &dyn Hosts, logins: &[Login<'_>]) -> anyhow::Result<LabeledGitItems> {
    let client = hosts.client()?;
    let calls = hosts.calls();
    let client = &client;
    let answers = join_all(logins.iter().map(|login| async move {
        let (host, kind) = match login.slot {
            Slot::GitHub => (github::HOST.to_owned(), GitHostKind::GitHub {}),
            Slot::Forgejo(url) => (url.host(), GitHostKind::Forgejo {}),
            Slot::ApiKey(_) | Slot::AgentToken(_) => return None,
        };
        let caller = Caller {
            client,
            calls,
            host: &host,
            token: login.secret,
        };
        let result = match login.slot {
            Slot::Forgejo(url) if !hosts.is_public(url).await => {
                Err(HostError::Private(host.clone()))
            }
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
    }))
    .await;
    Ok(combine(answers.into_iter().flatten().collect()))
}

/// What one host said.
pub struct Answer {
    pub host: GitHost,
    pub result: Result<Vec<Found>, HostError>,
}

/// Every host's error and their items deduplicated, newest activity first.
pub fn combine(answers: Vec<Answer>) -> LabeledGitItems {
    let mut hosts = Vec::new();
    let mut found: Vec<Found> = Vec::new();
    let mut at: HashMap<String, usize> = HashMap::new();
    for Answer { mut host, result } in answers {
        match result {
            Ok(listed) => {
                for one in listed {
                    match at.get(&one.item.url) {
                        Some(&i) => absorb(&mut found[i].item, one.item),
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
    let (items, labels) = found.into_iter().map(|one| (one.item, one.labels)).unzip();
    LabeledGitItems {
        found: GitItems { items, hosts },
        labels,
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
                }]),
            },
            Answer {
                host: host("git.example.com", "some-user"),
                result: Err(HostError::Timeout("git.example.com".into())),
            },
        ]);
        assert_eq!(got.found.items, vec![ok]);
        assert_eq!(got.labels, [["bug"]]);
        assert_eq!(got.found.hosts[0].error, None);
        assert_eq!(
            got.found.hosts[1].error.as_deref(),
            Some("git.example.com did not answer in time.")
        );
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
        let got = got.found;
        assert_eq!(got.items.len(), 2);
        assert_eq!(got.items[0], newer);
        let merged = &got.items[1];
        assert!(merged.authored && merged.review_requested);
        assert_eq!(merged.review, GitReview::Approved {});
        assert!(got.items[0].updated_at > SystemTime::UNIX_EPOCH);
    }
}
