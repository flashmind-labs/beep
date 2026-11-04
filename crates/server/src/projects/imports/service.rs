//! The server's side of importing: loading what the shared import code reads, and the pages that
//! list machines and found checkouts.

use super::{
    checks::{self, Machine, Page},
    context::Context,
};
use crate::{
    account::user::User,
    app::App,
    protocol::{
        FoundProject, ImportFork, ImportPreview, ImportedProject, MachineFolder, ProjectMachine,
        node::agent::SESSIONS_PROTOCOL,
    },
    store::NodeRow,
};
use beep_projects::imports::{
    ImportError,
    flow::{self, Import, choose_machine, project},
    folder::{FolderPath, look},
    found::{self, MachineFound},
    hosts::{find_fork as find_used_fork, forgejo_of},
    preview::{self as previews, checkout_for_worktree as worktree_checkout},
    records::ImportRow,
    repo::RepoRef,
};
use chrono::Utc;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

/// Whether importing `input` would clone the user's fork of it.
pub(crate) async fn find_fork(
    app: &Arc<App>,
    user: &Arc<User>,
    input: &str,
) -> Result<ImportFork, ImportError> {
    let context = Context::load(app, user).await?;
    let repo = RepoRef::parse(input, &forgejo_of(&context.credentials))?;
    let found = find_used_fork(context.hosts(), &context.credentials, &repo).await;
    Ok(ImportFork {
        project: repo.project(),
        fork: found.as_ref().map(|fork| fork.repo.project()),
        branches: found.map(|fork| fork.found.branches).unwrap_or_default(),
    })
}

/// Imports `job.repo` onto its machine, then looks through the user's other machines in the
/// background.
pub(crate) async fn import(
    app: &Arc<App>,
    user: &Arc<User>,
    job: Import<'_>,
) -> Result<ImportedProject, ImportError> {
    let context = Context::load(app, user).await?;
    flow::import(&context.env(user), job).await
}

/// Where `input` is checked out on online machines, and its branches with work in the first one.
pub(crate) async fn preview(
    app: &Arc<App>,
    user: &Arc<User>,
    input: &str,
) -> Result<ImportPreview, ImportError> {
    let context = Context::load(app, user).await?;
    let imports = user.imports().await?;
    previews::preview(&context.usable(), &imports, &context.servers(), input).await
}

/// The checkout of `input` on `endpoint`, recorded as the project's folder; `None` when offline or absent.
pub(crate) async fn checkout_for_worktree(
    app: &Arc<App>,
    user: &Arc<User>,
    input: &str,
    endpoint: &str,
) -> Result<Option<String>, ImportError> {
    let context = Context::load(app, user).await?;
    worktree_checkout(
        user.as_ref(),
        &context.usable(),
        &context.servers(),
        input,
        endpoint,
    )
    .await
}

/// One folder on `machine_id` for the folder browser: `path`, else the workspace, else home.
pub(crate) async fn browse(
    app: &Arc<App>,
    user: &Arc<User>,
    machine_id: &str,
    path: Option<&str>,
) -> Result<MachineFolder, ImportError> {
    let picked = match path {
        Some(path) => FolderPath::parse(path)?,
        None => None,
    };
    let context = Context::load(app, user).await?;
    let machine = choose_machine(&context.machines, Some(machine_id))?;
    let start = match picked {
        Some(picked) => picked,
        None => match machine.workspace.as_deref().map(FolderPath::parse) {
            Some(Ok(Some(workspace))) => workspace,
            _ => FolderPath::Home(String::new()),
        },
    };
    look(machine.shell()?, &machine.name, &start).await
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

/// Every linked machine as `project`'s page lists it, starting due looks; `None` when not imported.
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
    let now = Utc::now();
    let stale = checks::stale(&rows, &listed, now);
    let Page { machines, to_check } = checks::machines(&rows, &listed, now);
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
