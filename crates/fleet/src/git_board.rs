//! Home's pull requests and issues, read source by source. Each token's search and each project's
//! lists are a source; one that answers replaces only its own part, and what was kept from the
//! last run stands until then, so the list can be drawn before anything is read and never shrinks
//! while it is read again.

use crate::{
    desk::Desk,
    error::Result,
    open_items::{AT_ONCE, Access, Learned, ProjectRead, accounts_of, merge, repos_of},
};
use beep_projects::{
    credentials::last4,
    git_items::{
        access::Repo,
        board::{Answer, Login, OpenedItems, ask, combine},
        host::{Found, HostError},
    },
    issues::host_of_url,
};
use beep_proto::{GitHost, GitHostKind, LabeledProjectLists, Service};
use futures::{FutureExt, StreamExt, future::BoxFuture, stream::FuturesUnordered};
use secrecy::ExposeSecret;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, VecDeque},
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
    /// The search got no connection to the host from this Mac.
    #[serde(skip)]
    unreached: bool,
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
    /// The hosts a machine read a project's lists from in this run. A host this Mac cannot reach
    /// is no error while a machine reaches it.
    #[serde(skip)]
    reached: BTreeSet<String>,
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
            reached: BTreeSet::new(),
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
            .map(|read| {
                let mut host = read.host.clone();

                if read.unreached && self.reached.contains(&host.host) {
                    host.error = None;
                }

                Answer {
                    host,
                    result: Ok(read.found.clone()),
                }
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
        let unreached = matches!(result, Err(HostError::Unreachable(_)));
        let found = match result {
            Ok(found) => found,
            Err(e) => {
                host.error = Some(e.to_string());
                before.unwrap_or_default()
            }
        };
        self.hosts[index] = Some(HostRead {
            host,
            found,
            unreached,
        });
    }

    /// Takes a project's lists, read from `host`. One that failed keeps the lists it had before,
    /// beside the error.
    fn take_project(&mut self, read: ProjectRead, host: Option<&str>) {
        match read.result {
            Ok(lists) => {
                self.reached.extend(host.map(str::to_owned));
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
/// minutes.
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
                service: &opened.service,
                secret: &opened.secret,
                account: opened.github.as_ref().map(|github| github.login.as_str()),
            })
            .filter(|login| matches!(login.service, Service::GitHub | Service::Forgejo { .. }))
            .filter(|login| !login.secret.expose_secret().is_empty())
            .collect();
        // The host each project is on, as the searches name it.
        let hosts: HashMap<String, String> = crate::records::projects(&self.mesh)
            .into_iter()
            .map(|stored| {
                let host = Repo::try_from(stored.record.url.as_str())
                    .map(|repo| repo.host)
                    .unwrap_or_default();
                (stored.name, host)
            })
            .collect();
        let mut names: Vec<String> = hosts.keys().cloned().collect();
        names.sort();
        let key: Vec<String> = logins
            .iter()
            .map(|login| {
                let tail = last4(login.secret.expose_secret());
                format!("{:?}/{tail}/{:?}", login.service, login.account)
            })
            .chain(names.iter().map(|name| format!("project/{name}")))
            .collect();
        if let Some(update) = fresh(&key) {
            return Ok(update);
        }

        // Runs at once would read every project again each: one reads, the others take its list.
        let _run = self.hosts.board_run.lock().await;

        if let Some(update) = fresh(&key) {
            return Ok(update);
        }

        let mut sources = GitKept::resumed(kept, digest(&key), &logins, &names);
        let mut shown = None;

        if !sources.is_empty() {
            show(progress, &mut shown, sources.items()).await;
        }

        let client = &self.hosts.client;
        let logins = &logins;
        let turns = Semaphore::new(AT_ONCE);
        let turns = &turns;
        let mut queue: VecDeque<String> = names.into();
        let mut reading = 0;
        let mut steps: FuturesUnordered<BoxFuture<'_, Step>> = FuturesUnordered::new();

        for (index, login) in logins.iter().enumerate() {
            steps.push(
                async move {
                    let answer = match self.unreached(login) {
                        Some(answer) => Some(answer),
                        None => ask(&self.hosts, client, login).await,
                    };
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
                    sources.take_project(read, hosts.get(&name).map(String::as_str));

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

    /// The answer for a Forgejo `login` whose host this Mac's own CLI cannot reach now, without
    /// asking: its address is the same here, and the host is tried again when the pause ends.
    fn unreached(&self, login: &Login<'_>) -> Option<Answer> {
        let Service::Forgejo { url } = login.service else {
            return None;
        };
        let host = host_of_url(url.as_ref())?;

        if !self.hosts.gates.is_paused(self.local, &host) {
            return None;
        }

        Some(Answer {
            host: GitHost {
                host: url.host(),
                kind: GitHostKind::Forgejo {},
                account: login.account.map(ToOwned::to_owned),
                error: None,
            },
            result: Err(HostError::Unreachable(url.host())),
        })
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
#[path = "tests/git_board.rs"]
mod tests;
