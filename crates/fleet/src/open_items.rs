//! Home's pull requests and issues read through the machines: every imported project is asked
//! for its open lists, so a desktop with no token on the host still has them.

use crate::desk::Desk;
use beep_projects::{
    credentials::Slot,
    git_items::{
        access::{Repo, token_answers},
        board::{Login, OpenedItems},
        github,
        host::Found,
    },
    issues::MachineView,
};
use beep_proto::{
    GitHost, GitHostKind, GitItem, GitItemKind, GitItems, GitReview, LabeledGitItems,
    LabeledProjectLists, ProjectIssue, ProjectPullRequest,
};
use serde::{Deserialize, Serialize};
use std::{
    cmp::Reverse,
    collections::{HashMap, HashSet},
    sync::{Mutex, PoisonError},
    time::{Duration, Instant, UNIX_EPOCH},
};
use tokio::{sync::Semaphore, time::timeout};

/// Projects read at once.
pub(crate) const AT_ONCE: usize = 32;
/// How long one project may take, so a slow or offline machine does not hold Home.
const PER_PROJECT: Duration = Duration::from_secs(20);

/// What one project's machines said.
pub(crate) struct ProjectRead {
    pub(crate) name: String,
    pub(crate) result: Result<LabeledProjectLists, String>,
}

/// What the tokens and the project's machine said about the repositories of one project.
pub(crate) struct Learned {
    /// What the tokens said, by repository.
    answers: HashMap<Repo, bool>,
    /// The repositories no token could speak for, which the machine was asked about.
    asked: Vec<Repo>,
    /// What the project's machine said, when it was asked and answered.
    view: Option<MachineView>,
}

impl Desk {
    /// Which of `repos` the user can write to, and who they are on each host. The tokens answer
    /// first; for a repository they could not speak for, the machine that holds the project says
    /// what its own account may do. `limit` bounds the machines asked at once.
    pub(crate) async fn learn_access(
        &self,
        logins: &[Login<'_>],
        project: &str,
        repos: &HashSet<Repo>,
        limit: &Semaphore,
    ) -> Learned {
        let answers = token_answers(&self.hosts, logins, repos).await;
        let asked: Vec<Repo> = repos
            .iter()
            .filter(|repo| !answers.contains_key(*repo))
            .cloned()
            .collect();
        if asked.is_empty() {
            return Learned {
                answers,
                asked,
                view: None,
            };
        }
        let view = match kept_view(project) {
            Some(view) => Some(view),
            None => {
                // The semaphore is never closed, so waiting for it cannot fail.
                let _turn = limit.acquire().await;
                let asked = timeout(PER_PROJECT, self.machine_view(project)).await;
                let view = asked.ok().flatten();
                if let Some(view) = &view {
                    keep_view(project, view);
                }
                view
            }
        };
        Learned {
            answers,
            asked,
            view,
        }
    }

    /// The open lists of `name`, with a time limit, so a slow or offline machine does not hold
    /// Home.
    pub(crate) async fn read_project(&self, name: String) -> ProjectRead {
        let result = match timeout(PER_PROJECT, self.lists(&name)).await {
            Ok(Ok(lists)) => unreadable(&lists).map_or(Ok(lists), Err),
            Ok(Err(e)) => Err(e.to_string()),
            Err(_) => Err("no answer in time".to_owned()),
        };
        ProjectRead { name, result }
    }
}

/// Why a project's lists are of no use: neither could be read.
fn unreadable(lists: &LabeledProjectLists) -> Option<String> {
    match (&lists.lists.prs_error, &lists.lists.issues_error) {
        (Some(prs), Some(_)) => Some(prs.clone()),
        _ => None,
    }
}

/// `host` and `org/repo` out of an item's web address.
fn host_and_repo(url: &str) -> Option<(String, String)> {
    let rest = url.split_once("://")?.1;
    let mut parts = rest.split('/');
    let host = parts.next().filter(|host| !host.is_empty())?;
    let repo = format!("{}/{}", parts.next()?, parts.next()?);
    Some((host.to_owned(), repo))
}

/// What decides which of a project's open items Home lists: a repository the user can write to
/// lists them all, any other only the user's own. The tokens' searches already give what is
/// assigned to the user or asks for their review, so those need no row from the project.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub(crate) struct Access {
    /// The repositories the host says the user can write to.
    pub(crate) writable: HashSet<Repo>,
    /// The accounts the tokens sign in as, by host; empty when unknown.
    pub(crate) accounts: HashMap<String, Vec<String>>,
}

impl Access {
    /// Notes that the tokens or a machine sign in as `login` on `host`.
    pub(crate) fn note_account(&mut self, host: &str, login: &str) {
        let accounts = self.accounts.entry(host.to_owned()).or_default();
        if !accounts.iter().any(|known| known == login) {
            accounts.push(login.to_owned());
        }
    }

    /// Takes what was learned about one project. An answer replaces the one kept before; a
    /// repository nobody could speak for keeps its kept answer.
    pub(crate) fn learn(&mut self, learned: Learned) {
        let Learned {
            answers,
            asked,
            view,
        } = learned;
        for (repo, can) in answers {
            self.set(repo, can);
        }
        let Some(view) = view else {
            return;
        };
        self.note_account(&view.host, &view.login);
        for repo in asked.into_iter().filter(|repo| repo.host == view.host) {
            if let Some(can) = view.can_push {
                self.set(repo, can);
            }
        }
    }

    fn set(&mut self, repo: Repo, writable: bool) {
        if writable {
            self.writable.insert(repo);
        } else {
            self.writable.remove(&repo);
        }
    }

    /// Whether Home lists the item at `url` that `author` opened; `mine` when the machine's own
    /// account is known to be the author.
    fn lists(&self, url: &str, author: &str, mine: bool) -> bool {
        let Some((host, name)) = host_and_repo(url) else {
            return false;
        };
        let own = !author.is_empty()
            && self.accounts.get(&host).is_some_and(|accounts| {
                accounts
                    .iter()
                    .any(|account| account.eq_ignore_ascii_case(author))
            });
        mine || own || self.writable.contains(&Repo { host, name })
    }
}

/// How long a machine's answer about a project is kept.
const KEEP_VIEW: Duration = Duration::from_secs(3600);

/// What machines said, by project. Only answers that included whether the account can push.
static VIEWS: Mutex<Option<HashMap<String, (Instant, MachineView)>>> = Mutex::new(None);

fn kept_view(project: &str) -> Option<MachineView> {
    let views = VIEWS.lock().unwrap_or_else(PoisonError::into_inner);
    let (at, view) = views.as_ref()?.get(project)?;
    (at.elapsed() < KEEP_VIEW).then(|| view.clone())
}

fn keep_view(project: &str, view: &MachineView) {
    if view.can_push.is_none() {
        return;
    }
    VIEWS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get_or_insert_with(HashMap::new)
        .insert(project.to_owned(), (Instant::now(), view.clone()));
}

/// The GitHub accounts the tokens sign in as, by host.
pub(crate) fn accounts_of(logins: &[Login<'_>]) -> HashMap<String, Vec<String>> {
    let github = logins
        .iter()
        .filter(|login| matches!(login.slot, Slot::GitHub))
        .filter_map(|login| login.account.map(ToOwned::to_owned))
        .collect();
    HashMap::from([(github::HOST.to_owned(), github)])
}

/// The repositories the projects' items are in.
pub(crate) fn repos_of(projects: &[ProjectRead]) -> HashSet<Repo> {
    let urls = |lists: &LabeledProjectLists| {
        let issues = lists.lists.issues.iter().map(|issue| issue.url.clone());
        let prs = lists.lists.pull_requests.iter().map(|pr| pr.url.clone());
        issues.chain(prs).collect::<Vec<_>>()
    };
    projects
        .iter()
        .filter_map(|read| read.result.as_ref().ok())
        .flat_map(urls)
        .filter_map(|url| host_and_repo(&url))
        .map(|(host, name)| Repo { host, name })
        .collect()
}

/// A row with what a project list knows; the rest is false.
fn row(url: &str, number: u64, kind: GitItemKind) -> Option<GitItem> {
    let (host, repo) = host_and_repo(url)?;
    Some(GitItem {
        host,
        repo,
        number,
        kind,
        title: String::new(),
        url: url.to_owned(),
        updated_at: UNIX_EPOCH,
        authored: false,
        assigned: false,
        review_requested: false,
        draft: false,
        review: GitReview::None {},
        checks_failing: false,
        comments: 0,
    })
}

fn from_issue(issue: &ProjectIssue) -> Option<Found> {
    let mut item = row(&issue.url, issue.number, GitItemKind::Issue {})?;
    item.title = issue.title.clone();
    item.updated_at = issue.opened_at;
    item.comments = issue.comments;
    Some(Found {
        item,
        labels: issue.labels.clone(),
        opened_at: Some(issue.opened_at),
    })
}

fn from_pull_request(pr: &ProjectPullRequest, labels: &[String]) -> Option<Found> {
    let mut item = row(&pr.url, pr.number, GitItemKind::PullRequest {})?;
    item.title = pr.title.clone();
    item.updated_at = pr.updated_at;
    item.authored = pr.mine;
    item.draft = pr.draft;
    Some(Found {
        item,
        labels: labels.to_vec(),
        opened_at: Some(pr.created_at),
    })
}

/// What the projects add to what the tokens gave. A row the tokens also gave stays the token's,
/// which knows reviews and who waits, with the earlier opening time. A project that could not be
/// read is a host row that says why.
pub(crate) fn merge(
    tokens: OpenedItems,
    projects: Vec<ProjectRead>,
    access: &Access,
) -> OpenedItems {
    let OpenedItems { items, opened_at } = tokens;
    let mut hosts = items.found.hosts;
    let mut found: Vec<Found> = items
        .found
        .items
        .into_iter()
        .zip(items.labels)
        .zip(opened_at)
        .map(|((item, labels), opened_at)| Found {
            item,
            labels,
            opened_at,
        })
        .collect();
    let mut at: HashMap<String, usize> = found
        .iter()
        .enumerate()
        .map(|(i, one)| (one.item.url.clone(), i))
        .collect();
    for ProjectRead { name, result } in projects {
        let lists = match result {
            Ok(lists) => lists,
            Err(e) => {
                hosts.push(GitHost {
                    host: name.clone(),
                    kind: GitHostKind::GitHub {},
                    account: None,
                    error: Some(format!("{name}: {e}")),
                });
                continue;
            }
        };
        let issues = lists
            .lists
            .issues
            .iter()
            .filter(|issue| access.lists(&issue.url, &issue.author, false))
            .filter_map(from_issue);
        let prs = lists
            .lists
            .pull_requests
            .iter()
            .zip(&lists.pr_labels)
            .filter(|(pr, _)| access.lists(&pr.url, &pr.author, pr.mine))
            .filter_map(|(pr, labels)| from_pull_request(pr, labels));
        for one in issues.chain(prs) {
            match at.get(&one.item.url) {
                Some(&i) => {
                    let kept = &mut found[i];
                    kept.opened_at = match (kept.opened_at, one.opened_at) {
                        (Some(a), Some(b)) => Some(a.min(b)),
                        (a, b) => a.or(b),
                    };
                }
                None => {
                    at.insert(one.item.url.clone(), found.len());
                    found.push(one);
                }
            }
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use beep_proto::ProjectLists;

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
}
