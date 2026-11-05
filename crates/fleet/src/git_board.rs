//! Home's pull requests and issues, read source by source. Each token's search and each project's
//! lists are a source; one that answers replaces only its own part, and what was kept from the
//! last run stands until then, so the list can be drawn before anything is read and never shrinks
//! while it is read again.

use crate::{
    desk::Desk,
    error::Result,
    issues::project_names,
    open_items::{AT_ONCE, Access, Learned, ProjectRead, accounts_of, merge, repos_of},
};
use beep_projects::{
    credentials::{Slot, last4},
    git_items::{
        board::{Answer, Login, OpenedItems, ask, combine},
        host::Found,
    },
    imports::hosts::Hosts,
};
use beep_proto::{GitHost, LabeledProjectLists};
use futures::{FutureExt, StreamExt, future::BoxFuture, stream::FuturesUnordered};
use secrecy::ExposeSecret;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, VecDeque},
    hash::{DefaultHasher, Hash, Hasher},
    sync::{Mutex, PoisonError},
    time::{Duration, Instant},
};
use tokio::sync::{Semaphore, mpsc::Sender};

/// How long the list is kept when every host answered.
const FRESH: Duration = Duration::from_secs(180);
/// How long it is kept when a host failed, to try that one again sooner.
const FRESH_AFTER_ERROR: Duration = Duration::from_secs(60);

/// What one token's search found.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct HostRead {
    host: GitHost,
    found: Vec<Found>,
}

/// What Home's list is made of, to keep between runs. It holds no token: the key that says which
/// tokens and projects it was read with is a hash.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GitKept {
    key: String,
    /// By login, in their order; `None` until that host answers.
    hosts: Vec<Option<HostRead>>,
    /// The lists of each project that has been read, by name.
    projects: BTreeMap<String, LabeledProjectLists>,
    access: Access,
    /// Why the last read of a project failed. Not kept: a new run tries again.
    #[serde(skip)]
    failed: BTreeMap<String, String>,
}

impl GitKept {
    fn new(key: String, logins: &[Login<'_>]) -> Self {
        Self {
            key,
            hosts: vec![None; logins.len()],
            projects: BTreeMap::new(),
            access: Access {
                accounts: accounts_of(logins),
                ..Access::default()
            },
            failed: BTreeMap::new(),
        }
    }

    /// What was kept, when it was read with the same tokens and projects as now.
    fn resumed(kept: Option<Self>, key: String, logins: &[Login<'_>], names: &[String]) -> Self {
        let mut now = Self::new(key.clone(), logins);
        let Some(kept) = kept.filter(|kept| kept.key == key && kept.hosts.len() == logins.len())
        else {
            return now;
        };
        now.hosts = kept.hosts;
        now.projects = kept.projects;
        now.projects.retain(|name, _| names.contains(name));
        now.access.writable = kept.access.writable;
        for (host, accounts) in kept.access.accounts {
            for login in accounts {
                now.access.note_account(&host, &login);
            }
        }
        now
    }

    fn is_empty(&self) -> bool {
        self.hosts.iter().all(Option::is_none) && self.projects.is_empty()
    }

    fn items(&self) -> OpenedItems {
        let answers = self
            .hosts
            .iter()
            .flatten()
            .map(|read| Answer {
                host: read.host.clone(),
                result: Ok(read.found.clone()),
            })
            .collect();
        let lists = self.projects.iter().map(|(name, lists)| ProjectRead {
            name: name.clone(),
            result: Ok(lists.clone()),
        });
        let failed = self.failed.iter().map(|(name, why)| ProjectRead {
            name: name.clone(),
            result: Err(why.clone()),
        });
        merge(
            combine(answers),
            lists.chain(failed).collect(),
            &self.access,
        )
    }

    /// Takes a token's search. One that failed keeps what it found before, beside the error.
    fn take_host(&mut self, index: usize, answer: Option<Answer>) {
        let Some(Answer { mut host, result }) = answer else {
            return;
        };
        let before = self.hosts[index].take().map(|read| read.found);
        let found = match result {
            Ok(found) => found,
            Err(e) => {
                host.error = Some(e.to_string());
                before.unwrap_or_default()
            }
        };
        self.hosts[index] = Some(HostRead { host, found });
    }

    /// Takes a project's lists. One that failed keeps the lists it had before, beside the error.
    fn take_project(&mut self, read: ProjectRead) {
        match read.result {
            Ok(lists) => {
                self.failed.remove(&read.name);
                self.projects.insert(read.name, lists);
            }
            Err(why) => {
                self.failed.insert(read.name, why);
            }
        }
    }

    /// The part to keep: what was read, not what failed.
    fn saved(&self) -> Self {
        let mut saved = self.clone();
        saved.failed.clear();
        for read in saved.hosts.iter_mut().flatten() {
            read.host.error = None;
        }
        saved
    }
}

/// The list and what to keep of it.
#[derive(Debug, Clone)]
pub struct GitUpdate {
    pub items: OpenedItems,
    pub kept: GitKept,
}

/// The last run, and the tokens and projects it was read with, so a change of either reads again.
struct Last {
    key: Vec<String>,
    at: Instant,
    update: GitUpdate,
}

/// A host's rate limit is not a desktop's to spend on every call, so Home's list is kept for a few
/// minutes, as the server keeps it.
static BOARD: Mutex<Option<Last>> = Mutex::new(None);

fn fresh(key: &[String]) -> Option<GitUpdate> {
    let last = BOARD.lock().unwrap_or_else(PoisonError::into_inner);
    let last = last.as_ref()?;
    let failed = last
        .update
        .items
        .items
        .found
        .hosts
        .iter()
        .any(|host| host.error.is_some());
    let ttl = if failed { FRESH_AFTER_ERROR } else { FRESH };
    (last.key == key && last.at.elapsed() < ttl).then(|| last.update.clone())
}

/// One step of a run: a source that answered.
enum Step {
    Host {
        index: usize,
        answer: Option<Answer>,
    },
    Project(ProjectRead),
    Access(Learned),
}

/// A short stand-in for the tokens and projects a list was read with.
fn digest(key: &[String]) -> String {
    let mut hasher = DefaultHasher::new();
    key.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

impl Desk {
    /// The open pull requests and issues of every imported project, read through the machines,
    /// with the user's own on every git host they have a token for.
    pub async fn git_items(&self) -> Result<OpenedItems> {
        Ok(self.git_items_with(None, None).await?.items)
    }

    /// As `git_items`, starting from what `kept` holds when it was read with the same tokens and
    /// projects, and sending the list to `progress` each time a source changes it: first what was
    /// kept, then each token's search and each project's lists as they arrive. Gives the final
    /// list and what to keep of it.
    pub async fn git_items_with(
        &self,
        kept: Option<GitKept>,
        progress: Option<&Sender<OpenedItems>>,
    ) -> Result<GitUpdate> {
        let opened = self.opened();
        let logins: Vec<Login<'_>> = opened
            .iter()
            .map(|opened| Login {
                slot: &opened.slot,
                secret: &opened.secret,
                account: opened.github.as_ref().map(|github| github.login.as_str()),
            })
            .filter(|login| matches!(login.slot, Slot::GitHub | Slot::Forgejo(_)))
            .filter(|login| !login.secret.expose_secret().is_empty())
            .collect();
        let mut names = project_names(&self.mesh);
        names.sort();
        let key: Vec<String> = logins
            .iter()
            .map(|login| {
                let tail = last4(login.secret.expose_secret());
                format!("{:?}/{tail}/{:?}", login.slot, login.account)
            })
            .chain(names.iter().map(|name| format!("project/{name}")))
            .collect();
        if let Some(update) = fresh(&key) {
            return Ok(update);
        }
        let mut sources = GitKept::resumed(kept, digest(&key), &logins, &names);
        let mut shown = None;
        if !sources.is_empty() {
            show(progress, &mut shown, sources.items()).await;
        }

        let client = self.hosts.client()?;
        let client = &client;
        let logins = &logins;
        let turns = Semaphore::new(AT_ONCE);
        let turns = &turns;
        let mut queue: VecDeque<String> = names.into();
        let mut reading = 0;
        let mut steps: FuturesUnordered<BoxFuture<'_, Step>> = FuturesUnordered::new();
        for (index, login) in logins.iter().enumerate() {
            steps.push(
                async move {
                    let answer = ask(&self.hosts, client, login).await;
                    Step::Host { index, answer }
                }
                .boxed(),
            );
        }
        loop {
            while reading < AT_ONCE
                && let Some(name) = queue.pop_front()
            {
                reading += 1;
                steps.push(self.read_project(name).map(Step::Project).boxed());
            }
            let Some(step) = steps.next().await else {
                break;
            };
            match step {
                Step::Host { index, answer } => sources.take_host(index, answer),
                Step::Project(read) => {
                    reading -= 1;
                    let repos = repos_of(std::slice::from_ref(&read));
                    let name = read.name.clone();
                    sources.take_project(read);
                    if !repos.is_empty() {
                        steps.push(
                            async move {
                                let learned = self.learn_access(logins, &name, &repos, turns).await;
                                Step::Access(learned)
                            }
                            .boxed(),
                        );
                    }
                }
                Step::Access(learned) => sources.access.learn(learned),
            }
            show(progress, &mut shown, sources.items()).await;
        }

        let update = GitUpdate {
            items: sources.items(),
            kept: sources.saved(),
        };
        *BOARD.lock().unwrap_or_else(PoisonError::into_inner) = Some(Last {
            key,
            at: Instant::now(),
            update: update.clone(),
        });
        Ok(update)
    }
}

/// Sends `items` when they differ from what was sent last. A page that has gone has no use for
/// them, and the run does not depend on it.
async fn show(
    progress: Option<&Sender<OpenedItems>>,
    shown: &mut Option<OpenedItems>,
    items: OpenedItems,
) {
    if shown.as_ref() == Some(&items) {
        return;
    }
    if let Some(progress) = progress {
        let _ = progress.send(items.clone()).await;
    }
    *shown = Some(items);
}

#[cfg(test)]
mod tests {
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
}
