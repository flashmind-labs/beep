//! Asking every git host a user has a token for at once, and putting the answers together.

use super::Hosts;
use super::{Caller, HostError, forgejo, github, host::Found};
use beep_proto::Service;
use beep_proto::{GitHost, GitHostKind, GitItem, GitItems, GitReview, LabeledGitItems};
use reqwest::Client;
use secrecy::SecretString;
use std::{cmp::Reverse, collections::HashMap, time::SystemTime};

/// One token to read a host with.
pub struct Login<'a> {
    pub service: &'a Service,
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

/// Asks the host `login` is for; `None` for a credential that is no git host.
pub async fn ask(hosts: &dyn Hosts, client: &Client, login: &Login<'_>) -> Option<Answer> {
    let (host, kind) = match login.service {
        Service::GitHub => (github::HOST.to_owned(), GitHostKind::GitHub {}),
        Service::Forgejo { url } => (url.host(), GitHostKind::Forgejo {}),
        Service::ApiKey { provider: _ } | Service::Agent { agent: _ } => return None,
    };
    let caller = Caller {
        client,
        calls: hosts.calls(),
        host: &host,
        token: login.secret,
    };
    let result = match login.service {
        Service::Forgejo { url } => forgejo::items(&caller, url.as_ref()).await,
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
#[path = "../tests/git_items_board.rs"]
mod tests;
