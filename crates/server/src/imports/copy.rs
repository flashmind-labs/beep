//! Copying a project onto a machine without it: beep-sync brings it from a machine that has it over
//! iroh-blobs, `.git` included, and keeps the two in step from then on.

use super::{
    ImportError,
    clone::quote,
    folder::FolderPath,
    place::{self, Folders},
    preview::existing_checkout,
    repo::RepoRef,
    service::{Chosen, choose_machine, forgejo_of, look},
};
use crate::{
    app::App,
    nodes::push_sync,
    projects::branches::{READ_LIMIT, run_script},
    protocol::{
        ProjectCopy,
        node::files::{Reply, Request},
    },
    store::{ImportRow, NewImport, SyncRow},
    user::User,
};
use std::{path::Path, sync::Arc};

/// Copies `project` onto `machine_id`, unless the machine has it already.
pub(crate) async fn copy_project(
    app: &Arc<App>,
    user: &Arc<User>,
    project: &str,
    machine_id: &str,
) -> Result<ProjectCopy, ImportError> {
    let rows: Vec<ImportRow> = user
        .imports()
        .await?
        .into_iter()
        .filter(|row| row.project.eq_ignore_ascii_case(project))
        .collect();
    let Some(first) = rows.first() else {
        return Err(ImportError::NotImported(project.to_owned()));
    };
    let Chosen { row, node } = choose_machine(app, user, Some(machine_id)).await?;
    let record = async |folder: &str| {
        user.link_import(NewImport {
            project: &first.project,
            endpoint: &row.endpoint,
            folder,
            url: &first.url,
        })
        .await
    };
    let already = async |folder: &str| -> Result<ProjectCopy, ImportError> {
        record(folder).await?;
        Ok(ProjectCopy {
            folder: folder.to_owned(),
            from: None,
        })
    };
    if let Some(here) = rows.iter().find(|one| one.endpoint == row.endpoint) {
        return already(&here.folder).await;
    }
    let syncs = user.project_syncs().await?;
    let synced_on = |endpoint: &str| {
        syncs
            .iter()
            .find(|one| one.project == first.project && one.endpoint == endpoint)
    };
    if let Some(here) = synced_on(&row.endpoint) {
        return already(&here.folder).await;
    }
    let credentials = user.git_credentials(&app.credential_key).await?;
    let repo = RepoRef::parse(&first.url, &forgejo_of(&credentials))?;
    if let Some(found) = existing_checkout(&row, &node, &repo, None).await {
        return already(&found.folder).await;
    }
    if !node.syncs() {
        return Err(ImportError::NoSharedFolders(row.name.clone()));
    }
    if user.pinned_projects().await?.contains(&first.project) {
        return Err(ImportError::Pinned(first.project.clone()));
    }
    if user.paused_syncs().await?.contains(&first.project) {
        return Err(ImportError::SyncPaused(first.project.clone()));
    }
    let link = |e: anyhow::Error| ImportError::Link {
        machine: row.name.clone(),
        detail: format!("{e:#}"),
    };
    let Reply::Roots { roots } = node.files(&Request::Roots {}).await.map_err(link)? else {
        return Err(ImportError::NoSharedFolders(row.name.clone()));
    };
    let env = row.env.as_ref();
    let usual = place::checkout_folder(
        Folders {
            os: env.map(|env| env.os.as_str()),
            home: env.map(|env| env.home.as_str()),
            workspace: node.workspace.as_deref(),
        },
        &repo,
    );
    let Some(folder) = shared_folder(usual, &roots, &repo) else {
        return Err(ImportError::NoSharedFolders(row.name.clone()));
    };
    let Some(picked) = FolderPath::parse(&folder)? else {
        return Err(ImportError::BadFolder);
    };
    let seen = look(&node, &row, &picked).await?;
    if seen.exists && !seen.empty {
        return match seen.origin.as_deref() {
            Some(origin) if repo.matches_remote(origin) => already(&seen.path).await,
            Some(origin) if !origin.is_empty() => Err(ImportError::OtherRepo {
                folder: seen.path.clone(),
                machine: row.name.clone(),
                remote: origin.to_owned(),
            }),
            _ => Err(ImportError::Occupied {
                folder: seen.path.clone(),
                machine: row.name.clone(),
            }),
        };
    }
    if !seen.exists {
        // Sync refuses a folder that is not there.
        let make = format!("mkdir -p {}", quote(&seen.path));
        run_script(&node, &make, READ_LIMIT).await.map_err(link)?;
    }
    let to = SyncRow {
        project: first.project.clone(),
        endpoint: row.endpoint.clone(),
        folder: seen.path,
    };
    let mut sources: Vec<&ImportRow> = rows.iter().collect();
    // One that syncs it already need not start.
    sources.sort_by_key(|source| synced_on(&source.endpoint).is_none());
    let dest = node.connection.remote_id();
    let mut refused = None;
    for source in sources {
        let Some(giver) = app
            .nodes
            .online(&source.endpoint)
            .filter(|giver| giver.user == user.id() && giver.syncs())
        else {
            continue;
        };
        let starts = synced_on(&source.endpoint).is_none();
        if starts {
            user.set_project_sync(&SyncRow {
                project: first.project.clone(),
                endpoint: source.endpoint.clone(),
                folder: source.folder.clone(),
            })
            .await?;
        }
        user.set_project_sync(&to).await?;
        let refusals = push_sync(&app.nodes, user, Some(dest)).await?;
        let Some(refusal) = refusals.into_iter().find(|refusal| {
            refusal.refused.project == first.project
                && (refusal.node == dest || refusal.node == giver.connection.remote_id())
        }) else {
            record(&to.folder).await?;
            log::info!(
                "project copying; project={} node={} from={}",
                first.project,
                row.endpoint,
                source.endpoint
            );
            return Ok(ProjectCopy {
                folder: to.folder,
                from: Some(giver.name.clone()),
            });
        };
        user.remove_project_sync(&first.project, &row.endpoint)
            .await?;
        if starts {
            user.remove_project_sync(&first.project, &source.endpoint)
                .await?;
        }
        push_sync(&app.nodes, user, None).await?;
        let on_dest = refusal.node == dest;
        let failed = ImportError::CannotSync {
            machine: if on_dest {
                row.name.clone()
            } else {
                giver.name.clone()
            },
            why: refusal.refused.message,
        };
        if on_dest {
            return Err(failed);
        }
        refused = Some(failed);
    }
    Err(refused.unwrap_or_else(|| ImportError::NoCopyOnline(first.project.clone())))
}

/// `usual` when the machine shares it, else `<org>/<repo>` under the first folder it shares.
fn shared_folder(usual: Option<String>, roots: &[String], repo: &RepoRef) -> Option<String> {
    usual
        .filter(|usual| roots.iter().any(|root| Path::new(usual).starts_with(root)))
        .or_else(|| {
            let root = roots.first()?.trim_end_matches('/');
            Some(format!("{root}/{}/{}", repo.org(), repo.name()))
        })
}
