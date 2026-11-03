//! The user's fork of a repository and their open pull request branches, read from GitHub or Forgejo.

use super::host::{Caller, HostError, Page};
use crate::projects::{branches::branch_name, model::RepoName};
use reqwest::Url;
use serde::{Deserialize, de::DeserializeOwned};

/// The most forks, or open pull requests, one page reads.
const PAGE: &str = "50";
/// The most pages of one list read.
const MAX_PAGES: usize = 20;

/// Which API the calls go to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Api {
    GitHub,
    Forgejo,
}

impl Api {
    /// The query that reads one page of a list.
    fn page(self) -> String {
        match self {
            Api::GitHub => format!("per_page={PAGE}"),
            Api::Forgejo => format!("limit={PAGE}"),
        }
    }
}

/// The API base (such as `https://api.github.com`) and the repository there.
pub(crate) struct Lookup<'a> {
    pub(crate) api: Api,
    pub(crate) base: &'a str,
    pub(crate) upstream: &'a RepoName,
    /// The token's account, when it is known already; asked for otherwise.
    pub(crate) login: Option<&'a str>,
}

/// The user's fork, as the import clones it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Fork {
    /// `login/repo`.
    pub(crate) name: RepoName,
    /// The upstream's default branch, which the local one tracks.
    pub(crate) default_branch: String,
    /// The heads of the user's open pull requests from the fork.
    pub(crate) branches: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct Account {
    login: String,
}

#[derive(Debug, Deserialize)]
struct Repo {
    full_name: String,
    #[serde(default)]
    default_branch: String,
    #[serde(default)]
    fork: bool,
    #[serde(default)]
    parent: Option<Named>,
    #[serde(default)]
    owner: Option<Account>,
}

#[derive(Debug, Deserialize)]
struct Named {
    full_name: String,
}

#[derive(Debug, Deserialize)]
struct Pull {
    #[serde(default)]
    user: Option<Account>,
    head: Head,
}

#[derive(Debug, Deserialize)]
struct Head {
    #[serde(rename = "ref")]
    branch: String,
    /// `None` once the fork the branch was on is deleted.
    #[serde(default)]
    repo: Option<Named>,
}

/// The user's fork of `lookup.upstream`, `None` when they have none or own the upstream.
pub(crate) async fn find(
    caller: &Caller<'_>,
    lookup: &Lookup<'_>,
) -> Result<Option<Fork>, HostError> {
    let base = lookup.base.trim_end_matches('/');
    let login = match lookup.login {
        Some(login) => login.to_owned(),
        None => caller.get::<Account>(&format!("{base}/user")).await?.login,
    };
    if lookup.upstream.owner().eq_ignore_ascii_case(&login) {
        return Ok(None);
    }
    let upstream = lookup.upstream.to_string();
    let parent: Repo = caller.get(&format!("{base}/repos/{upstream}")).await?;
    let Some(name) = fork_of(caller, lookup, base, &login, &parent.full_name).await? else {
        return Ok(None);
    };
    let pulls: Vec<Pull> = all_pages(
        caller,
        format!(
            "{base}/repos/{upstream}/pulls?state=open&{}",
            lookup.api.page()
        ),
        |_| false,
    )
    .await?;
    Ok(Some(Fork {
        branches: branches_from(&pulls, &login, &name),
        name,
        default_branch: parent.default_branch,
    }))
}

/// The fork of `upstream` owned by `login`, by name first, else from the upstream's fork list.
async fn fork_of(
    caller: &Caller<'_>,
    lookup: &Lookup<'_>,
    base: &str,
    login: &str,
    upstream: &str,
) -> Result<Option<RepoName>, HostError> {
    let same_name = format!("{base}/repos/{login}/{}", lookup.upstream.repo());
    match caller.get::<Repo>(&same_name).await {
        Ok(repo)
            if repo.fork
                && repo
                    .parent
                    .as_ref()
                    .is_some_and(|parent| parent.full_name.eq_ignore_ascii_case(upstream)) =>
        {
            return Ok(RepoName::parse(&repo.full_name).ok());
        }
        Ok(_) | Err(HostError::Status { status: 404, .. }) => {}
        Err(e) => return Err(e),
    }
    let owned = |repo: &Repo| {
        repo.owner
            .as_ref()
            .is_some_and(|owner| owner.login.eq_ignore_ascii_case(login))
    };
    let forks: Vec<Repo> = all_pages(
        caller,
        format!("{base}/repos/{upstream}/forks?{}", lookup.api.page()),
        |page| page.iter().any(owned),
    )
    .await?;
    Ok(forks
        .iter()
        .find(|repo| owned(repo))
        .and_then(|repo| RepoName::parse(&repo.full_name).ok()))
}

/// Every item of the list at `first`, following `Link` up to `MAX_PAGES` and only on the same host.
async fn all_pages<T: DeserializeOwned>(
    caller: &Caller<'_>,
    first: String,
    found: impl Fn(&[T]) -> bool,
) -> Result<Vec<T>, HostError> {
    let origin = Url::parse(&first).ok().map(|url| url.origin());
    let mut items = Vec::new();
    let mut url = Some(first);
    for _ in 0..MAX_PAGES {
        let Some(at) = url.take() else {
            break;
        };
        let page: Page<Vec<T>> = caller.get_page(&at).await?;
        let done = found(&page.items);
        items.extend(page.items);
        if done {
            break;
        }
        url = page.next.filter(|next| {
            let next = Url::parse(next).ok().map(|url| url.origin());
            next.is_some() && next == origin
        });
    }
    Ok(items)
}

/// Head branches of `login`'s pull requests from `fork`, once each, skipping option-like names.
fn branches_from(pulls: &[Pull], login: &str, fork: &RepoName) -> Vec<String> {
    let fork = fork.to_string();
    let mut branches: Vec<String> = Vec::new();
    for pull in pulls {
        let mine = pull
            .user
            .as_ref()
            .is_some_and(|user| user.login.eq_ignore_ascii_case(login));
        let from_fork = pull
            .head
            .repo
            .as_ref()
            .is_some_and(|repo| repo.full_name.eq_ignore_ascii_case(&fork));
        let Some(branch) = branch_name(&pull.head.branch) else {
            continue;
        };
        if mine && from_fork && !branches.iter().any(|known| known == branch) {
            branches.push(branch.to_owned());
        }
    }
    branches
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::Client;
    use secrecy::SecretString;
    use std::{collections::HashMap, sync::Arc};
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
        sync::Semaphore,
    };

    /// A host that answers `routes`, 404s otherwise, and checks the token.
    async fn host(routes: HashMap<&'static str, &'static str>) -> String {
        paged_host(routes, HashMap::new()).await
    }

    /// `host`, whose reply to each path in `next` links to the page there.
    async fn paged_host(
        routes: HashMap<&'static str, &'static str>,
        next: HashMap<&'static str, &'static str>,
    ) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("a free port");
        let base = format!("http://{}", listener.local_addr().expect("an address"));
        let here = base.clone();
        tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                let mut data = Vec::new();
                let mut buf = [0u8; 4096];
                while !data.windows(4).any(|w| w == b"\r\n\r\n") {
                    let n = stream.read(&mut buf).await.expect("the request reads");
                    if n == 0 {
                        break;
                    }
                    data.extend_from_slice(&buf[..n]);
                }
                let request = String::from_utf8_lossy(&data).into_owned();
                let path = request.split(' ').nth(1).unwrap_or_default().to_owned();
                let authorized = request
                    .to_ascii_lowercase()
                    .contains("authorization: token tok");
                let (status, body) = match routes.get(path.as_str()) {
                    Some(body) if authorized => ("200 OK", *body),
                    Some(_) => ("401 Unauthorized", "{}"),
                    None => ("404 Not Found", r#"{"message":"Not Found"}"#),
                };
                let link = next
                    .get(path.as_str())
                    .map(|next| format!("Link: <{here}{next}>; rel=\"next\"\r\n"))
                    .unwrap_or_default();
                let reply = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\n{link}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                stream
                    .write_all(reply.as_bytes())
                    .await
                    .expect("the reply writes");
            }
        });
        base
    }

    async fn look(
        base: &str,
        api: Api,
        upstream: &str,
        login: Option<&str>,
    ) -> Result<Option<Fork>, HostError> {
        let client = Client::new();
        let calls = Arc::new(Semaphore::new(4));
        let token = SecretString::from("tok");
        let caller = Caller {
            client: &client,
            calls: &calls,
            host: "github.com",
            token: &token,
        };
        let upstream = RepoName::parse(upstream).expect("a repository name");
        find(
            &caller,
            &Lookup {
                api,
                base,
                upstream: &upstream,
                login,
            },
        )
        .await
    }

    const UPSTREAM: &str = r#"{"full_name": "acme/widget", "default_branch": "main", "fork": false,
        "owner": {"login": "acme"}}"#;
    const PULLS: &str = r#"[
      {"number": 7, "user": {"login": "octo"},
       "head": {"ref": "fix-retry", "repo": {"full_name": "octo/widget"}}},
      {"number": 8, "user": {"login": "octo"},
       "head": {"ref": "on-upstream", "repo": {"full_name": "acme/widget"}}},
      {"number": 9, "user": {"login": "someone"},
       "head": {"ref": "theirs", "repo": {"full_name": "someone/widget"}}},
      {"number": 10, "user": {"login": "octo"},
       "head": {"ref": "--upload-pack=x", "repo": {"full_name": "octo/widget"}}},
      {"number": 11, "user": {"login": "Octo"},
       "head": {"ref": "dark-theme", "repo": {"full_name": "Octo/Widget"}}},
      {"number": 12, "user": {"login": "octo"}, "head": {"ref": "gone", "repo": null}}
    ]"#;

    #[tokio::test]
    async fn a_fork_under_the_login_with_its_pull_request_branches() {
        let base = host(HashMap::from([
            ("/user", r#"{"login": "octo"}"#),
            ("/repos/acme/widget", UPSTREAM),
            (
                "/repos/octo/widget",
                r#"{"full_name": "octo/widget", "default_branch": "main", "fork": true,
                    "parent": {"full_name": "acme/widget", "default_branch": "main"}}"#,
            ),
            ("/repos/acme/widget/pulls?state=open&per_page=50", PULLS),
        ]))
        .await;
        let fork = look(&base, Api::GitHub, "acme/widget", None)
            .await
            .expect("the host answers")
            .expect("a fork");
        assert_eq!(fork.name.to_string(), "octo/widget");
        assert_eq!(fork.default_branch, "main");
        assert_eq!(fork.branches, ["fix-retry", "dark-theme"]);
    }

    #[tokio::test]
    async fn the_fork_and_branches_on_later_pages_are_found() {
        let base = paged_host(
            HashMap::from([
                ("/repos/acme/widget", UPSTREAM),
                (
                    "/repos/octo/widget",
                    r#"{"full_name": "octo/widget", "fork": false}"#,
                ),
                (
                    "/repos/acme/widget/forks?per_page=50",
                    r#"[{"full_name": "someone/widget", "owner": {"login": "someone"}}]"#,
                ),
                (
                    "/repos/acme/widget/forks?per_page=50&page=2",
                    r#"[{"full_name": "octo/widget-fork", "owner": {"login": "octo"}}]"#,
                ),
                (
                    "/repos/acme/widget/pulls?state=open&per_page=50",
                    r#"[{"user": {"login": "octo"},
                         "head": {"ref": "first", "repo": {"full_name": "octo/widget-fork"}}}]"#,
                ),
                (
                    "/repos/acme/widget/pulls?state=open&per_page=50&page=2",
                    r#"[{"user": {"login": "octo"},
                         "head": {"ref": "second", "repo": {"full_name": "octo/widget-fork"}}}]"#,
                ),
            ]),
            HashMap::from([
                (
                    "/repos/acme/widget/forks?per_page=50",
                    "/repos/acme/widget/forks?per_page=50&page=2",
                ),
                (
                    "/repos/acme/widget/pulls?state=open&per_page=50",
                    "/repos/acme/widget/pulls?state=open&per_page=50&page=2",
                ),
            ]),
        )
        .await;
        let fork = look(&base, Api::GitHub, "acme/widget", Some("octo"))
            .await
            .expect("the host answers")
            .expect("a fork");
        assert_eq!(fork.name.to_string(), "octo/widget-fork");
        assert_eq!(fork.branches, ["first", "second"]);
    }
}
