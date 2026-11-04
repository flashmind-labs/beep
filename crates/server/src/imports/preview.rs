//! What importing a repository would find: its checkouts on online machines and branches with work.

use super::{
    ImportError,
    checks::forgejo_servers,
    place::{self, Folders},
    repo::{Host, RepoRef},
};
use crate::{
    app::App,
    nodes::{Online, SHELL},
    projects::branches::{
        READ_LIMIT, branches_with_work, my_prs, quote, read_branches, run_script,
    },
    protocol::{ImportCheckout, ImportPreview, node::agent::SESSIONS_PROTOCOL},
    store::{NewImport, NodeRow},
    user::User,
};
use futures::future::join_all;
use std::{collections::HashSet, time::Duration};
use tokio::time::timeout;

/// The longest asking one machine for its checkouts may take.
const FOUND_LIMIT: Duration = Duration::from_secs(20);
/// The most checkouts one machine names.
const FOUND_FOLDERS: u32 = 800;
/// The most folders one machine's remotes are read in.
const MAX_CANDIDATES: usize = 20;

/// A folder and its remotes, as the remotes script prints them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FolderRemotes {
    pub(crate) folder: String,
    /// Name and URL, in the order git lists them.
    pub(crate) remotes: Vec<(String, String)>,
}

/// Prints `@@dir <folder>` then `remote.<name>.url <url>` lines for each folder.
pub(crate) fn remotes_script(folders: &[String]) -> String {
    let quoted: Vec<String> = folders.iter().map(|folder| quote(folder)).collect();
    format!(
        "for f in {}; do echo \"@@dir $f\"; git -C \"$f\" config --get-regexp '^remote\\..*\\.url$' 2>/dev/null; done; true",
        quoted.join(" ")
    )
}

pub(crate) fn parse_remotes(out: &str) -> Vec<FolderRemotes> {
    let mut found: Vec<FolderRemotes> = Vec::new();
    for line in out.lines() {
        if let Some(folder) = line.strip_prefix("@@dir ") {
            found.push(FolderRemotes {
                folder: folder.to_owned(),
                remotes: Vec::new(),
            });
        } else if let Some(last) = found.last_mut()
            && let Some((key, url)) = line.split_once(' ')
            && let Some(name) = key
                .strip_prefix("remote.")
                .and_then(|key| key.strip_suffix(".url"))
        {
            last.remotes.push((name.to_owned(), url.trim().to_owned()));
        }
    }
    found
}

/// The remote of `folder` that is `repo`, `origin` first; `None` when none is.
pub(crate) fn repo_remote(folder: &FolderRemotes, repo: &RepoRef) -> Option<String> {
    let mut remotes: Vec<&(String, String)> = folder.remotes.iter().collect();
    remotes.sort_by_key(|(name, _)| name != "origin");
    remotes
        .into_iter()
        .find(|(_, url)| repo.matches_remote(url))
        .map(|(name, _)| name.clone())
}

/// The last part of a remote URL or path, without `.git`, lowercased.
fn repo_name_of(remote: &str) -> String {
    let last = remote
        .trim()
        .trim_end_matches('/')
        .rsplit(['/', ':', '\\'])
        .next()
        .unwrap_or_default();
    last.strip_suffix(".git")
        .unwrap_or(last)
        .to_ascii_lowercase()
}

/// One online machine of the user's that runs commands.
struct Machine {
    row: NodeRow,
    node: Online,
}

async fn machines(app: &App, user: &User) -> Result<Vec<Machine>, ImportError> {
    Ok(user
        .nodes()
        .await?
        .into_iter()
        .filter_map(|row| {
            let node = app
                .nodes
                .online(&row.endpoint)
                .filter(|node| node.user == user.id() && node.offers(SHELL))?;
            Some(Machine { row, node })
        })
        .collect())
}

/// Folders on one machine that may be checkouts of `repo` or `fork`, each with its remotes.
async fn candidates_on(
    row: &NodeRow,
    node: &Online,
    repo: &RepoRef,
    fork: Option<&RepoRef>,
    imported: &[String],
) -> Vec<FolderRemotes> {
    let mut candidates: Vec<String> = imported.to_vec();
    let env = row.env.as_ref();
    let folders = Folders {
        os: env.map(|env| env.os.as_str()),
        home: env.map(|env| env.home.as_str()),
        workspace: node.workspace.as_deref(),
    };
    candidates.extend(place::checkout_folder(folders, repo));
    if node.offers(SESSIONS_PROTOCOL) {
        match timeout(FOUND_LIMIT, node.found_folders(FOUND_FOLDERS)).await {
            Ok(Ok(found)) => {
                let names: Vec<String> = [Some(repo), fork]
                    .into_iter()
                    .flatten()
                    .map(|repo| repo.name().to_ascii_lowercase())
                    .collect();
                candidates.extend(
                    found
                        .into_iter()
                        .filter(|folder| {
                            names.contains(&repo_name_of(&folder.origin))
                                || names.contains(&repo_name_of(&folder.path))
                        })
                        .map(|folder| folder.path),
                );
            }
            Ok(Err(e)) => log::info!("preview: no found folders on {}: {e:#}", row.name),
            Err(_) => log::info!("preview: found folders not in time on {}", row.name),
        }
    }
    let mut seen = HashSet::new();
    candidates.retain(|folder| seen.insert(folder.clone()));
    candidates.truncate(MAX_CANDIDATES);
    match run_script(node, &remotes_script(&candidates), READ_LIMIT).await {
        Ok(out) => parse_remotes(&out),
        Err(e) => {
            log::info!("preview: remotes not read on {}: {e:#}", row.name);
            Vec::new()
        }
    }
}

/// The checkouts of `repo` on one machine (see `candidates_on`).
async fn checkouts_on(
    machine: &Machine,
    repo: &RepoRef,
    imported: &[String],
) -> Vec<ImportCheckout> {
    candidates_on(&machine.row, &machine.node, repo, None, imported)
        .await
        .iter()
        .filter_map(|folder| {
            Some(ImportCheckout {
                machine_id: machine.row.endpoint.clone(),
                machine_name: machine.row.name.clone(),
                remote: repo_remote(folder, repo)?,
                folder: folder.folder.clone(),
            })
        })
        .collect()
}

/// A checkout of `repo` or `fork` on machine `row` to link rather than clone, `origin` matches first.
pub(super) async fn existing_checkout(
    row: &NodeRow,
    node: &Online,
    repo: &RepoRef,
    fork: Option<&RepoRef>,
) -> Option<FolderRemotes> {
    let found = candidates_on(row, node, repo, fork, &[]).await;
    pick_existing(found, repo, fork)
}

/// The first of `found` that is `repo` or `fork` by any remote, `origin` matches first.
fn pick_existing(
    found: Vec<FolderRemotes>,
    repo: &RepoRef,
    fork: Option<&RepoRef>,
) -> Option<FolderRemotes> {
    let remote_of = |folder: &FolderRemotes| {
        repo_remote(folder, repo).or_else(|| fork.and_then(|fork| repo_remote(folder, fork)))
    };
    let mut matching: Vec<(bool, FolderRemotes)> = found
        .into_iter()
        .filter_map(|folder| {
            let remote = remote_of(&folder)?;
            Some((remote != "origin", folder))
        })
        .collect();
    matching.sort_by_key(|(not_origin, _)| *not_origin);
    matching.into_iter().next().map(|(_, folder)| folder)
}

/// Where `repo` is checked out on online machines, and its branches with work in the first one.
pub(crate) async fn preview(
    app: &App,
    user: &User,
    input: &str,
) -> Result<ImportPreview, ImportError> {
    let servers = forgejo_servers(app, user).await?;
    let repo = RepoRef::parse(input, &servers)?;
    let project = repo.project();
    let machines = machines(app, user).await?;
    if machines.is_empty() {
        return Err(ImportError::NoneOnline);
    }
    let imports = user.imports().await?;
    let looks = machines.iter().map(|machine| {
        let imported: Vec<String> = imports
            .iter()
            .filter(|row| {
                row.endpoint == machine.row.endpoint && row.project.eq_ignore_ascii_case(&project)
            })
            .map(|row| row.folder.clone())
            .collect();
        let repo = &repo;
        async move { checkouts_on(machine, repo, &imported).await }
    });
    let mut checkouts: Vec<ImportCheckout> = join_all(looks).await.into_iter().flatten().collect();
    // One whose origin is the repository reads its branches best.
    checkouts.sort_by_key(|checkout| checkout.remote != "origin");
    let mut notes = Vec::new();
    let mut default_branch = String::new();
    let mut branches = Vec::new();
    if let Some(first) = checkouts.first()
        && let Some(machine) = machines
            .iter()
            .find(|machine| machine.row.endpoint == first.machine_id)
    {
        match read_branches(&machine.node, &first.folder, "main").await {
            Ok((default, read)) => {
                default_branch = default;
                branches = read;
            }
            Err(e) => notes.push(format!(
                "The branches on {} were not read: {e:#}.",
                machine.row.name
            )),
        }
    }
    let prs = if repo.host == Host::GitHub {
        let at = checkouts
            .first()
            .and_then(|first| {
                machines
                    .iter()
                    .find(|machine| machine.row.endpoint == first.machine_id)
            })
            .unwrap_or(&machines[0]);
        match my_prs(&at.node, &project).await {
            Ok(prs) => prs,
            Err(e) => {
                notes.push(format!(
                    "Your pull requests were not read on {}: {e:#}.",
                    at.row.name
                ));
                Vec::new()
            }
        }
    } else {
        Vec::new()
    };
    Ok(ImportPreview {
        branches: branches_with_work(&branches, &prs, &default_branch),
        project,
        checkouts,
        default_branch,
        note: notes.join(" "),
    })
}

/// The checkout of `input` on `endpoint`, recorded as the project's folder; `None` when offline or absent.
pub(crate) async fn checkout_for_worktree(
    app: &App,
    user: &User,
    input: &str,
    endpoint: &str,
) -> Result<Option<String>, ImportError> {
    let servers = forgejo_servers(app, user).await?;
    let repo = RepoRef::parse(input, &servers)?;
    let project = repo.project();
    let imports = user.imports().await?;
    if let Some(row) = imports
        .iter()
        .find(|row| row.endpoint == endpoint && row.project.eq_ignore_ascii_case(&project))
    {
        return Ok(Some(row.folder.clone()));
    }
    let Some(machine) = machines(app, user)
        .await?
        .into_iter()
        .find(|machine| machine.row.endpoint == endpoint)
    else {
        return Ok(None);
    };
    let Some(found) = checkouts_on(&machine, &repo, &[]).await.into_iter().next() else {
        return Ok(None);
    };
    user.link_import(NewImport {
        project: &project,
        endpoint,
        folder: &found.folder,
        url: &repo.web_url(),
    })
    .await?;
    Ok(Some(found.folder))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn import_finds_checkout_by_fork_url() {
        let repo = RepoRef::parse("acme/widget", &[]).expect("a repo");
        let out = "@@dir /srv/fork/widget\n\
            remote.origin.url git@github.com:someone/widget.git\n\
            remote.upstream.url https://github.com/acme/widget\n\
            @@dir /srv/other\n\
            remote.origin.url https://github.com/acme/gadget.git\n\
            @@dir /srv/not-a-checkout\n\
            @@dir /srv/widget\n\
            remote.mine.url https://github.com/someone/widget\n\
            remote.origin.url https://github.com/Acme/Widget.git\n";
        let found = parse_remotes(out);
        assert_eq!(found.len(), 4);
        let matched: Vec<(&str, Option<String>)> = found
            .iter()
            .map(|folder| (folder.folder.as_str(), repo_remote(folder, &repo)))
            .collect();
        assert_eq!(
            matched,
            [
                ("/srv/fork/widget", Some("upstream".to_owned())),
                ("/srv/other", None),
                ("/srv/not-a-checkout", None),
                ("/srv/widget", Some("origin".to_owned())),
            ]
        );
    }

    #[test]
    fn an_import_links_a_checkout_of_the_fork_or_the_repository() {
        let repo = RepoRef::parse("acme/widget", &[]).expect("a repo");
        let name = crate::projects::model::RepoName::parse("octo/widget").expect("a name");
        let fork = repo.sibling(&name).expect("a fork");
        let found = parse_remotes(
            "@@dir /srv/other\n\
             remote.origin.url https://github.com/acme/gadget.git\n\
             @@dir /srv/mirror\n\
             remote.mine.url https://github.com/acme/widget\n\
             remote.origin.url /srv/elsewhere\n\
             @@dir /srv/fork/widget\n\
             remote.origin.url git@github.com:octo/widget.git\n",
        );
        let picked = pick_existing(found.clone(), &repo, Some(&fork)).expect("a checkout");
        assert_eq!(picked.folder, "/srv/fork/widget");
        let without_fork = pick_existing(found.clone(), &repo, None).expect("a checkout");
        assert_eq!(without_fork.folder, "/srv/mirror");
        assert_eq!(pick_existing(found[..1].to_vec(), &repo, Some(&fork)), None);
    }
}
