//! Every host a project's repository is on (primary, roles, imports, checkout remotes), since a
//! mirrored repository runs workflows on each.

use super::watch::Entry;
use crate::{
    account::credentials::{Opened, Slot},
    account::user::User,
    projects::imports::{parse_remotes, remotes_script},
    projects::{
        fleet::{Fleet, NodeFleet},
        issues::issue_host,
        model::{IssueHost, MachinePath, RepoName, remote_parts},
    },
    protocol::GitHostKind,
    store::ProjectRow,
};
use iroh::EndpointId;
use std::time::{Duration, Instant};

/// The most hosts one project's runs are read from.
pub(super) const MAX_HOSTS: usize = 4;
/// How long the remotes read from a checkout are kept.
const REMOTES_FOR: Duration = Duration::from_secs(30 * 60);
/// The longest reading a checkout's remotes may take.
const REMOTES_LIMIT: Duration = Duration::from_secs(10);

/// A repository on one host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RepoOnHost {
    pub(crate) host: IssueHost,
    pub(crate) repo: RepoName,
    /// Where the project's issues and pull requests are: the host `ListProjectRuns` reads.
    pub(crate) primary: bool,
}

impl RepoOnHost {
    /// The host's wire name: `github.com`, or the Forgejo server's host.
    pub(crate) fn name(&self) -> String {
        host_name(&self.host)
    }

    pub(crate) fn kind(&self) -> GitHostKind {
        match self.host {
            IssueHost::Github => GitHostKind::GitHub {},
            IssueHost::Forgejo(_) => GitHostKind::Forgejo {},
        }
    }
}

/// `github.com`, or a Forgejo server's host.
pub(crate) fn host_name(host: &IssueHost) -> String {
    match host {
        IssueHost::Github => "github.com".to_owned(),
        IssueHost::Forgejo(server) => server.as_str().to_owned(),
    }
}

/// Hosts the user has a token for; remotes elsewhere are left out rather than shown as errors.
pub(crate) struct Tokens<'a>(pub(crate) &'a [Opened]);

impl Tokens<'_> {
    fn has(&self, host: &IssueHost) -> bool {
        self.0.iter().any(|opened| match (&opened.slot, host) {
            (Slot::GitHub, IssueHost::Github) => true,
            (Slot::Forgejo(url), IssueHost::Forgejo(server)) => {
                url.hostname().eq_ignore_ascii_case(server.as_str())
            }
            _ => false,
        })
    }
}

/// What the hosts are found from.
pub(crate) struct Sources<'a> {
    /// The project's issues host, as `ListProjectRuns` reads it.
    pub(crate) primary: Option<IssueHost>,
    pub(crate) project: &'a ProjectRow,
    /// The web pages it was imported from.
    pub(crate) import_urls: Vec<String>,
    /// A checkout's remote URLs.
    pub(crate) remotes: &'a [String],
}

/// The repository a web or remote URL names: its host and last two path parts, without `.git`.
fn repo_of_url(url: &str) -> Option<(IssueHost, RepoName)> {
    let (host, path) = remote_parts(url.trim())?;
    let host = IssueHost::of_hostname(&host)?;
    let path = path.trim_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);
    let mut parts = path.rsplit('/');
    let repo = parts.next()?;
    let owner = parts.next()?;
    let name = RepoName::parse(&format!("{owner}/{repo}")).ok()?;
    Some((host, name))
}

/// Every host of the project's repository, primary first, at most [`MAX_HOSTS`], one repo per host.
/// A remote counts only with a token for its host and a matching full name (or name, on another host).
pub(crate) fn collect(sources: &Sources<'_>, tokens: &Tokens<'_>) -> Vec<RepoOnHost> {
    let name = &sources.project.name;
    let mut found: Vec<RepoOnHost> = Vec::new();
    let mut add = |host: IssueHost, repo: RepoName, primary: bool| {
        if found.len() < MAX_HOSTS && !found.iter().any(|at| at.host == host) {
            found.push(RepoOnHost {
                host,
                repo,
                primary,
            });
        }
    };
    if let Some(host) = &sources.primary {
        add(host.clone(), name.clone(), true);
    }
    let roles = &sources.project.roles;
    for host in [&roles.issues, &roles.prs].into_iter().flatten() {
        add(host.clone(), name.clone(), false);
    }
    for (host, repo) in sources
        .import_urls
        .iter()
        .filter_map(|url| repo_of_url(url))
    {
        if repo.to_string().eq_ignore_ascii_case(&name.to_string()) {
            add(host, repo, false);
        }
    }
    let remotes: Vec<(IssueHost, RepoName)> = sources
        .remotes
        .iter()
        .filter_map(|url| repo_of_url(url))
        .filter(|(host, _)| tokens.has(host))
        .collect();
    let same = |repo: &RepoName| repo.to_string().eq_ignore_ascii_case(&name.to_string());
    for (host, repo) in remotes.iter().filter(|(_, repo)| same(repo)) {
        add(host.clone(), repo.clone(), false);
    }
    for (host, repo) in remotes
        .iter()
        .filter(|(_, repo)| repo.repo().eq_ignore_ascii_case(name.repo()))
    {
        add(host.clone(), repo.clone(), false);
    }
    found
}

/// The remotes read from one of the project's checkouts, and when.
#[derive(Debug, Clone)]
pub(super) struct RemotesRead {
    at: Instant,
    urls: Vec<String>,
}

/// Where a checkout of the project is.
struct Place {
    machine: EndpointId,
    folder: String,
}

/// The project's checkouts, home first, then its import folders.
async fn places(user: &User, project: &ProjectRow) -> anyhow::Result<Vec<Place>> {
    let mut checkouts = user.checkouts(project.id).await?;
    checkouts.sort_by_key(|checkout| !checkout.home);
    let mut places: Vec<Place> = checkouts
        .into_iter()
        .map(|checkout| Place {
            machine: checkout.machine,
            folder: checkout.path.as_str().to_owned(),
        })
        .collect();
    let name = project.name.to_string();
    for import in user.imports().await? {
        if !import.project.eq_ignore_ascii_case(&name) {
            continue;
        }
        if let (Ok(machine), Ok(path)) = (
            import.endpoint.parse::<EndpointId>(),
            MachinePath::parse(&import.folder),
        ) {
            places.push(Place {
                machine,
                folder: path.as_str().to_owned(),
            });
        }
    }
    Ok(places)
}

/// The remote URLs of the first checkout whose machine is online; `None` when none is.
async fn read_remotes(
    fleet: &dyn Fleet,
    user: &User,
    project: &ProjectRow,
) -> anyhow::Result<Option<Vec<String>>> {
    for place in places(user, project).await? {
        let Some(shell) = fleet.shell(&place.machine) else {
            continue;
        };
        let ran = shell
            .run(&remotes_script(&[place.folder]), REMOTES_LIMIT)
            .await?;
        let urls = parse_remotes(&ran.out)
            .into_iter()
            .flat_map(|folder| folder.remotes.into_iter().map(|(_, url)| url))
            .collect();
        return Ok(Some(urls));
    }
    Ok(None)
}

/// The project's remotes: read in the last half hour, else reread from an online checkout, else the last read.
async fn remotes(fleet: &NodeFleet, project: &ProjectRow, entry: &Entry) -> Vec<String> {
    let kept = entry.remotes();
    if let Some(kept) = &kept
        && kept.at.elapsed() < REMOTES_FOR
    {
        return kept.urls.clone();
    }
    match read_remotes(fleet, &fleet.user, project).await {
        Ok(Some(urls)) => {
            entry.set_remotes(RemotesRead {
                at: Instant::now(),
                urls: urls.clone(),
            });
            urls
        }
        Ok(None) => kept.map(|kept| kept.urls).unwrap_or_default(),
        Err(e) => {
            log::info!("repo watch: a checkout's remotes did not read: {e:#}");
            kept.map(|kept| kept.urls).unwrap_or_default()
        }
    }
}

/// Every host `project`'s runs are read from, or why its host is unknown.
pub(super) async fn find(
    fleet: &NodeFleet,
    project: &ProjectRow,
    entry: &Entry,
    tokens: &Tokens<'_>,
) -> Result<Vec<RepoOnHost>, String> {
    let user = &fleet.user;
    let unknown = |e: &dyn std::fmt::Display| {
        log::warn!("runs: the project's host is unknown: {e:#}");
        "Beep could not read where this project lives.".to_owned()
    };
    let primary = issue_host(user, project).await.map_err(|e| unknown(&e))?;
    let name = project.name.to_string();
    let import_urls = user
        .imports()
        .await
        .map_err(|e| unknown(&e))?
        .into_iter()
        .filter(|import| import.project.eq_ignore_ascii_case(&name))
        .map(|import| import.url)
        .collect();
    let remotes = remotes(fleet, project, entry).await;
    Ok(collect(
        &Sources {
            primary,
            project,
            import_urls,
            remotes: &remotes,
        },
        tokens,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        account::credentials::ForgejoUrl,
        projects::model::{ForgejoHost, Roles, Visibility},
    };
    use secrecy::SecretString;

    fn project(roles: Roles) -> ProjectRow {
        ProjectRow {
            id: crate::projects::model::ProjectId(uuid::Uuid::from_u128(1)),
            name: RepoName::parse("acme/widget").expect("a name"),
            roles,
            visibility: Visibility::Public,
            default_branch: "main".to_owned(),
            pin: None,
        }
    }

    fn opened(slot: Slot) -> Opened {
        Opened {
            slot,
            secret: SecretString::from("tok"),
            github: None,
        }
    }

    fn forge() -> IssueHost {
        IssueHost::Forgejo(ForgejoHost::parse("forge.example.com").expect("a host"))
    }

    #[test]
    fn a_mirror_on_github_and_forgejo_gives_both_primary_first() {
        let project = project(Roles {
            issues: Some(IssueHost::Github),
            prs: Some(IssueHost::Github),
        });
        let tokens = [
            opened(Slot::GitHub),
            opened(Slot::Forgejo(
                "https://forge.example.com"
                    .parse::<ForgejoUrl>()
                    .expect("a url"),
            )),
        ];
        let remotes = [
            "git@github.com:acme/widget.git".to_owned(),
            "ssh://git@forge.example.com:2222/mirrors/widget.git".to_owned(),
            "https://gitlab.example.org/acme/widget.git".to_owned(),
        ];
        let found = collect(
            &Sources {
                primary: Some(IssueHost::Github),
                project: &project,
                import_urls: Vec::new(),
                remotes: &remotes,
            },
            &Tokens(&tokens),
        );
        let got: Vec<(String, String, bool)> = found
            .iter()
            .map(|at| (at.name(), at.repo.to_string(), at.primary))
            .collect();
        assert_eq!(
            got,
            [
                ("github.com".to_owned(), "acme/widget".to_owned(), true),
                (
                    "forge.example.com".to_owned(),
                    "mirrors/widget".to_owned(),
                    false
                ),
            ]
        );
    }

    #[test]
    fn a_remote_without_a_token_or_of_another_repository_is_left_out() {
        let project = project(Roles::default());
        let tokens = [opened(Slot::GitHub)];
        let remotes = [
            "https://forge.example.com/acme/widget".to_owned(),
            "https://github.com/someone/other.git".to_owned(),
        ];
        let found = collect(
            &Sources {
                primary: Some(forge()),
                project: &project,
                import_urls: Vec::new(),
                remotes: &remotes,
            },
            &Tokens(&tokens),
        );
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].host, forge());
        assert!(found[0].primary);
    }
}
