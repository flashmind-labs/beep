//! Picking the machine, running the clone there and recording the project.

use super::{
    ImportError,
    checks::{self, Machine, Page},
    clone::{self, Failure, ForkSetup, Job, Outcome},
    folder::{self, FolderPath, Seen},
    fork::{self, UsedFork},
    found::{self, MachineFound},
    link::link_fork,
    place::{self, Candidate, Folders},
    preview::existing_checkout,
    repo::{Host, RepoRef},
};
use crate::{
    app::App,
    credentials::{ForgejoUrl, Opened, Slot},
    nodes::{Online, SHELL},
    protocol::{
        FoundProject, ImportFork, ImportedProject, MachineFolder, ProjectMachine,
        node::agent::SESSIONS_PROTOCOL,
    },
    store::{ImportRow, NewImport, NodeRow},
    user::User,
};
use chrono::Utc;
use secrecy::{ExposeSecret, SecretString};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::Duration,
};

/// The longest a clone may take.
const CLONE_LIMIT: Duration = Duration::from_secs(30 * 60);
/// The most output kept while cloning; the end explains a failure.
const MAX_OUTPUT: usize = 64 * 1024;
/// The longest reading a folder may take.
const LOOK_LIMIT: Duration = Duration::from_secs(20);
/// The most output kept from reading a folder; past it, folders are left out.
const MAX_LISTING: usize = 256 * 1024;

/// One import, as the sheet or the tool asks for it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Import<'a> {
    /// `org/repo` or the repository's URL.
    pub(crate) repo: &'a str,
    /// The machine's id; `None` lets `place::choose` pick.
    pub(crate) machine_id: Option<&'a str>,
    /// Where it goes, absolute or `~/...`; `None` (or blank) for the
    /// machine's projects folder.
    pub(crate) folder: Option<&'a str>,
}

/// One of the user's machines, online and able to run commands.
struct Chosen {
    row: NodeRow,
    node: Online,
}

/// The machine `machine_id` names, or the one `place::choose` picks.
async fn choose_machine(
    app: &App,
    user: &User,
    machine_id: Option<&str>,
) -> Result<Chosen, ImportError> {
    let mut rows = user.nodes().await?;
    let mut online: Vec<Option<Online>> = rows
        .iter()
        .map(|row| {
            app.nodes
                .online(&row.endpoint)
                .filter(|online| online.user == user.id())
        })
        .collect();
    let candidates: Vec<Candidate<'_>> = rows
        .iter()
        .zip(&online)
        .map(|(row, online)| Candidate {
            id: &row.endpoint,
            name: &row.name,
            online: online.is_some(),
            shell: online.as_ref().is_some_and(|online| online.offers(SHELL)),
            workspace: online
                .as_ref()
                .is_some_and(|online| online.workspace.is_some()),
        })
        .collect();
    let at = place::choose(&candidates, machine_id)?;
    let row = rows.swap_remove(at);
    let node = online
        .swap_remove(at)
        .ok_or_else(|| ImportError::Offline(row.name.clone()))?;
    Ok(Chosen { row, node })
}

/// The Forgejo servers among the user's credentials.
pub(super) fn forgejo_of(credentials: &[Opened]) -> Vec<ForgejoUrl> {
    credentials
        .iter()
        .filter_map(|opened| match &opened.slot {
            Slot::Forgejo(url) => Some(url.clone()),
            _ => None,
        })
        .collect()
}

/// Whether importing `input` would clone the user's fork of it.
pub(crate) async fn find_fork(
    app: &App,
    user: &User,
    input: &str,
) -> Result<ImportFork, ImportError> {
    let credentials = user.git_credentials(&app.credential_key).await?;
    let repo = RepoRef::parse(input, &forgejo_of(&credentials))?;
    let found = fork::find(app, &credentials, &repo).await;
    Ok(ImportFork {
        project: repo.project(),
        fork: found.as_ref().map(|fork| fork.repo.project()),
        branches: found.map(|fork| fork.found.branches).unwrap_or_default(),
    })
}

/// Imports `job.repo` onto its machine: clones it there, or finds it
/// already there, and records it. When the user has a fork of it, the fork
/// is cloned as `origin` with the repository as `upstream`, and their pull
/// request branches made local; the project is still the repository. With
/// no folder given, or one not there yet, a checkout of the repository or
/// the fork already on the machine is linked instead of cloning again.
/// The user's other machines are then looked through for checkouts of it,
/// in the background.
pub(crate) async fn import(
    app: &Arc<App>,
    user: &Arc<User>,
    job: Import<'_>,
) -> Result<ImportedProject, ImportError> {
    let credentials = user.git_credentials(&app.credential_key).await?;
    let repo = RepoRef::parse(job.repo, &forgejo_of(&credentials))?;
    let picked = match job.folder {
        Some(folder) => FolderPath::parse(folder)?,
        None => None,
    };
    let Chosen { row, node } = choose_machine(app, user, job.machine_id).await?;
    let fork = fork::find(app, &credentials, &repo).await;
    let seen = match &picked {
        Some(picked) => Some(look(&node, &row, picked).await?),
        None => None,
    };
    let existing = if seen.as_ref().is_none_or(|seen| !seen.exists) {
        existing_checkout(&row, &node, &repo, fork.as_ref().map(|fork| &fork.repo)).await
    } else {
        None
    };
    let token = token_for(&credentials, &repo);
    let (folder, cloned) = match existing {
        Some(found) => (found.folder, false),
        None => {
            let folder = match &seen {
                Some(seen) => folder::destination(seen, repo.name()),
                None => {
                    let env = row.env.as_ref();
                    let folders = Folders {
                        os: env.map(|env| env.os.as_str()),
                        home: env.map(|env| env.home.as_str()),
                        workspace: node.workspace.as_deref(),
                    };
                    place::checkout_folder(folders, &repo)
                        .ok_or_else(|| ImportError::NoHome(row.name.clone()))?
                }
            };
            let cloned = run_clone(&node, &row, &repo, fork.as_ref(), &folder, token).await?;
            (folder, cloned)
        }
    };
    if !cloned && let Some(fork) = &fork {
        link_fork(&node, &row, &repo, fork, &folder, token).await;
    }
    let saved = user
        .save_import(NewImport {
            project: &repo.project(),
            endpoint: &row.endpoint,
            folder: &folder,
            url: &repo.web_url(),
        })
        .await?;
    let fork_name = fork.as_ref().map(|fork| &fork.found.name);
    user.set_import_fork(&saved.project, &row.endpoint, fork_name)
        .await?;
    log::info!(
        "project imported; project={} node={} cloned={cloned} fork={}",
        saved.project,
        row.endpoint,
        fork.is_some()
    );
    let others: Vec<String> = user
        .nodes()
        .await?
        .into_iter()
        .map(|node| node.endpoint)
        .filter(|endpoint| *endpoint != row.endpoint)
        .collect();
    app.checkouts.check_all(app, user, &others);
    Ok(project(saved, row.name, cloned))
}

/// One folder on the machine `machine_id`, for the sheet's folder browser:
/// `path`, or the machine's workspace, else its home.
pub(crate) async fn browse(
    app: &App,
    user: &User,
    machine_id: &str,
    path: Option<&str>,
) -> Result<MachineFolder, ImportError> {
    let picked = match path {
        Some(path) => FolderPath::parse(path)?,
        None => None,
    };
    let Chosen { row, node } = choose_machine(app, user, Some(machine_id)).await?;
    let start = match picked {
        Some(picked) => picked,
        None => match node.workspace.as_deref().map(FolderPath::parse) {
            Some(Ok(Some(workspace))) => workspace,
            _ => FolderPath::Home(String::new()),
        },
    };
    look(&node, &row, &start).await
}

/// What `folder` on the machine holds.
async fn look(
    node: &Online,
    row: &NodeRow,
    folder: &FolderPath,
) -> Result<MachineFolder, ImportError> {
    let mut output = String::new();
    let mut on_output = |text: &str| {
        if output.len() < MAX_LISTING {
            output.push_str(text);
        }
    };
    node.exec(&folder::script(folder), LOOK_LIMIT, &mut on_output)
        .await
        .map_err(|e| ImportError::Link {
            machine: row.name.clone(),
            detail: format!("{e:#}"),
        })?;
    match folder::read(&output) {
        Some(Seen::Folder(seen)) => Ok(seen),
        Some(Seen::File(path)) => Err(ImportError::NotAFolder {
            folder: path,
            machine: row.name.clone(),
        }),
        Some(Seen::Denied(path)) => Err(ImportError::Unreadable {
            folder: path,
            machine: row.name.clone(),
        }),
        None => Err(ImportError::NoListing {
            machine: row.name.clone(),
        }),
    }
}

/// The user's token for `repo`'s host, when they saved one: on GitHub, the
/// account for the repository's owner.
fn token_for<'a>(credentials: &'a [Opened], repo: &RepoRef) -> Option<&'a SecretString> {
    repo.credential(credentials).map(|opened| &opened.secret)
}

/// Clones on `node`, the user's `fork` when they have one; `true` when it
/// cloned, `false` when a checkout of the repository or the fork was there
/// already.
async fn run_clone(
    node: &Online,
    row: &NodeRow,
    repo: &RepoRef,
    fork: Option<&UsedFork>,
    folder: &str,
    token: Option<&SecretString>,
) -> Result<bool, ImportError> {
    let cloning = fork.map_or(repo, |fork| &fork.repo);
    let url = cloning.clone_url();
    let upstream_url = repo.clone_url();
    let script = clone::script(&Job {
        url: &url,
        folder,
        token,
        github: repo.host == Host::GitHub,
        fork: fork.map(|fork| ForkSetup {
            upstream_url: &upstream_url,
            default_branch: &fork.found.default_branch,
            branches: &fork.found.branches,
        }),
    });
    let mut output = String::new();
    let mut on_output = |text: &str| {
        output.push_str(text);
        if output.len() > MAX_OUTPUT * 2 {
            output = clone::tail(&output);
        }
    };
    let exited = node
        .exec(script.expose_secret(), CLONE_LIMIT, &mut on_output)
        .await
        .map_err(|e| ImportError::Link {
            machine: row.name.clone(),
            detail: format!("{e:#}"),
        })?;
    match clone::outcome(&output, exited.exit_code, exited.timed_out) {
        Outcome::Cloned => Ok(true),
        Outcome::Existing(remote)
            if repo.matches_remote(&remote)
                || fork.is_some_and(|fork| fork.repo.matches_remote(&remote)) =>
        {
            Ok(false)
        }
        Outcome::Existing(remote) => Err(ImportError::OtherRepo {
            folder: folder.to_owned(),
            machine: row.name.clone(),
            remote: if remote.is_empty() {
                "no origin remote".to_owned()
            } else {
                remote
            },
        }),
        Outcome::Occupied => Err(ImportError::Occupied {
            folder: folder.to_owned(),
            machine: row.name.clone(),
        }),
        Outcome::NoGit => Err(ImportError::NoGit(row.name.clone())),
        Outcome::TimedOut => Err(ImportError::TimedOut(cloning.project())),
        Outcome::Failed(said) => Err(clone::explain(Failure {
            output: &said,
            project: cloning.project(),
            host: repo.host_name(),
            machine: &row.name,
            had_token: token.is_some(),
        })),
    }
}

/// What the machines found, one row per repository not imported yet.
pub(crate) async fn found_projects(
    app: &App,
    user: &User,
    found: Vec<MachineFound>,
) -> anyhow::Result<Vec<FoundProject>> {
    let servers = checks::forgejo_servers(app, user).await?;
    let imported = user.imports().await?;
    Ok(found::group(found, &imported, &servers))
}

/// Every linked machine of the user's as `project`'s page lists it, and
/// looks started on those online not looked through since it was
/// imported. `None` when the user has not imported `project`.
pub(crate) async fn project_machines(
    app: &Arc<App>,
    user: &Arc<User>,
    project: &str,
) -> anyhow::Result<Option<Vec<ProjectMachine>>> {
    let rows: Vec<ImportRow> = user
        .imports()
        .await?
        .into_iter()
        .filter(|row| row.project.eq_ignore_ascii_case(project))
        .collect();
    if rows.is_empty() {
        return Ok(None);
    }
    let linked: HashSet<String> = user
        .account_keys()
        .await?
        .into_iter()
        .map(|key| key.endpoint_id)
        .collect();
    let nodes: Vec<NodeRow> = user
        .nodes()
        .await?
        .into_iter()
        .filter(|node| linked.contains(&node.endpoint))
        .collect();
    let checked = user.checkout_checks().await?;
    let listed: Vec<Machine<'_>> = nodes
        .iter()
        .map(|row| {
            let online = app
                .nodes
                .online(&row.endpoint)
                .filter(|online| online.user == user.id());
            Machine {
                row,
                online: online.is_some(),
                lookable: online.is_some_and(|online| online.offers(SESSIONS_PROTOCOL)),
                checking: app.checkouts.is_checking(user.id(), &row.endpoint),
                checked_at: checked.get(&row.endpoint).copied(),
            }
        })
        .collect();
    let stale = checks::stale(&rows, &listed, Utc::now());
    let Page { machines, to_check } = checks::machines(&rows, &listed);
    app.checkouts.check_all(app, user, &to_check);
    app.checkouts.check_all(app, user, &stale);
    Ok(Some(machines))
}

/// The user's imported projects on machines still theirs.
pub(crate) async fn imported(user: &User) -> anyhow::Result<Vec<ImportedProject>> {
    let names: HashMap<String, String> = user
        .nodes()
        .await?
        .into_iter()
        .map(|node| (node.endpoint, node.name))
        .collect();
    Ok(user
        .imports()
        .await?
        .into_iter()
        .filter_map(|row| {
            let name = names.get(&row.endpoint)?.clone();
            Some(project(row, name, false))
        })
        .collect())
}

fn project(row: ImportRow, machine_name: String, cloned: bool) -> ImportedProject {
    ImportedProject {
        project: row.project,
        url: row.url,
        machine_id: row.endpoint,
        machine_name,
        folder: row.folder,
        cloned,
        imported_at: row.imported_at.into(),
    }
}
