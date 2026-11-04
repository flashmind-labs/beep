//! Each user's open items across their git hosts, kept for a few minutes.

use super::{
    forgejo, github,
    host::{Caller, Found, HostError},
};
use crate::{
    AsyncMutex,
    account::credentials::{Slot, host_client},
    protocol::{GitHost, GitHostKind, GitItem, GitItems, LabeledGitItems},
    store::GitLogin,
};
use futures::future::join_all;
use reqwest::Client;
use std::{
    cmp::Reverse,
    collections::HashMap,
    sync::{Arc, Mutex, PoisonError},
    time::{Duration, Instant},
};
use tokio::sync::Semaphore;

/// How long a list is kept when every host answered.
const FRESH: Duration = Duration::from_secs(180);
/// How long it is kept when a host failed, to try that one again sooner.
const FRESH_AFTER_ERROR: Duration = Duration::from_secs(60);
/// Calls to git hosts in flight at once, server-wide.
const MAX_CALLS: usize = 16;

/// What one user was last given.
struct Cached {
    /// The credential ids it was read with, so a token change rereads.
    logins: Vec<String>,
    at: Instant,
    items: LabeledGitItems,
}

/// Each user's last list, locked per user so concurrent asks read the hosts once.
pub struct GitBoard {
    users: Mutex<HashMap<String, Arc<AsyncMutex<Option<Cached>>>>>,
    calls: Arc<Semaphore>,
}

impl Default for GitBoard {
    fn default() -> Self {
        Self::new()
    }
}

impl GitBoard {
    pub fn new() -> Self {
        Self {
            users: Mutex::default(),
            calls: Arc::new(Semaphore::new(MAX_CALLS)),
        }
    }

    /// The server-wide limit on git host calls in flight.
    pub(crate) fn calls(&self) -> &Arc<Semaphore> {
        &self.calls
    }

    /// `user`'s open items with `logins`, from the cache while it is fresh.
    pub async fn list(&self, user: &str, logins: Vec<GitLogin>) -> anyhow::Result<LabeledGitItems> {
        if logins.is_empty() {
            return Ok(LabeledGitItems {
                found: GitItems {
                    items: Vec::new(),
                    hosts: Vec::new(),
                },
                labels: Vec::new(),
            });
        }
        let slot = self
            .users
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entry(user.to_owned())
            .or_default()
            .clone();
        // Held across the reads on purpose: a second ask waits for the
        // first and takes its list.
        let mut cached = slot.lock().await;
        let ids: Vec<String> = logins.iter().map(|login| login.id.clone()).collect();
        if let Some(last) = cached.as_ref() {
            let ttl = if last
                .items
                .found
                .hosts
                .iter()
                .any(|host| host.error.is_some())
            {
                FRESH_AFTER_ERROR
            } else {
                FRESH
            };
            if last.logins == ids && last.at.elapsed() < ttl {
                return Ok(last.items.clone());
            }
        }
        let client = host_client()?;
        let items = read(&client, &self.calls, &logins).await;
        *cached = Some(Cached {
            logins: ids,
            at: Instant::now(),
            items: items.clone(),
        });
        Ok(items)
    }
}

/// Asks every host at once and puts their answers together.
async fn read(client: &Client, calls: &Arc<Semaphore>, logins: &[GitLogin]) -> LabeledGitItems {
    let answers = join_all(logins.iter().map(|login| async move {
        let (host, kind) = match &login.slot {
            Slot::GitHub => (github::HOST.to_owned(), GitHostKind::GitHub {}),
            Slot::Forgejo(url) => (url.host(), GitHostKind::Forgejo {}),
            Slot::ApiKey(_) | Slot::AgentToken(_) => return None,
        };
        let caller = Caller {
            client,
            calls,
            host: &host,
            token: &login.secret,
        };
        let result = match &login.slot {
            Slot::Forgejo(url) => forgejo::items(&caller, url).await,
            _ => github::items(&caller).await,
        };
        Some(Answer {
            host: GitHost {
                host: host.clone(),
                kind,
                account: login.account.clone(),
                error: None,
            },
            result,
        })
    }))
    .await;
    combine(answers.into_iter().flatten().collect())
}

/// One host's answer.
struct Answer {
    host: GitHost,
    result: Result<Vec<Found>, HostError>,
}

/// Every host's error and their items deduplicated, newest activity first.
fn combine(answers: Vec<Answer>) -> LabeledGitItems {
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
    if other.review != (crate::protocol::GitReview::None {}) {
        item.review = other.review;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{GitItemKind, GitReview};
    use std::time::{SystemTime, UNIX_EPOCH};

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
