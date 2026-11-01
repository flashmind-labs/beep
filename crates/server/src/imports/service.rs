//! Picking the machine, running the clone there and recording the project.

use super::{
    ImportError,
    clone::{self, Failure, Job, Outcome},
    place::{self, Candidate, Folders},
    repo::{Host, RepoRef},
};
use crate::{
    app::App,
    credentials::{ForgejoUrl, Opened, Slot},
    nodes::{Online, SHELL},
    protocol::ImportedProject,
    store::{ImportRow, NewImport, NodeRow},
    user::User,
};
use secrecy::{ExposeSecret, SecretString};
use std::{collections::HashMap, time::Duration};

/// The longest a clone may take.
const CLONE_LIMIT: Duration = Duration::from_secs(30 * 60);
/// The most output kept while cloning; the end explains a failure.
const MAX_OUTPUT: usize = 64 * 1024;

/// Imports `repo` onto the machine `machine_id`, or the one `place::choose`
/// picks: clones it there, or finds it already there, and records it.
pub(crate) async fn import(
    app: &App,
    user: &User,
    repo: &str,
    machine_id: Option<&str>,
) -> Result<ImportedProject, ImportError> {
    let credentials = user.git_credentials(&app.credential_key).await?;
    let servers: Vec<ForgejoUrl> = credentials
        .iter()
        .filter_map(|opened| match &opened.slot {
            Slot::Forgejo(url) => Some(url.clone()),
            _ => None,
        })
        .collect();
    let repo = RepoRef::parse(repo, &servers)?;
    let rows = user.nodes().await?;
    let online: Vec<Option<Online>> = rows
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
    let row = &rows[at];
    let node = online[at]
        .as_ref()
        .ok_or_else(|| ImportError::Offline(row.name.clone()))?;
    let env = row.env.as_ref();
    let folders = Folders {
        os: env.map(|env| env.os.as_str()),
        home: env.map(|env| env.home.as_str()),
        workspace: node.workspace.as_deref(),
    };
    let folder = place::checkout_folder(folders, &repo)
        .ok_or_else(|| ImportError::NoHome(row.name.clone()))?;
    let token = token_for(&credentials, &repo.host);
    let cloned = run_clone(node, row, &repo, &folder, token).await?;
    let saved = user
        .save_import(NewImport {
            project: &repo.project(),
            endpoint: &row.endpoint,
            folder: &folder,
            url: &repo.web_url(),
        })
        .await?;
    log::info!(
        "project imported; project={} node={} cloned={cloned}",
        saved.project,
        row.endpoint
    );
    Ok(project(saved, row.name.clone(), cloned))
}

/// The user's token for `host`, when they saved one.
fn token_for<'a>(credentials: &'a [Opened], host: &Host) -> Option<&'a SecretString> {
    credentials
        .iter()
        .find(|opened| match (&opened.slot, host) {
            (Slot::GitHub, Host::GitHub) => true,
            (Slot::Forgejo(saved), Host::Forgejo(url)) => saved == url,
            _ => false,
        })
        .map(|opened| &opened.secret)
        .filter(|secret| !secret.expose_secret().is_empty())
}

/// Clones on `node`; `true` when it cloned, `false` when the checkout was
/// there already.
async fn run_clone(
    node: &Online,
    row: &NodeRow,
    repo: &RepoRef,
    folder: &str,
    token: Option<&SecretString>,
) -> Result<bool, ImportError> {
    let url = repo.clone_url();
    let script = clone::script(&Job {
        url: &url,
        folder,
        token,
        github: repo.host == Host::GitHub,
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
        Outcome::Existing(remote) if repo.matches_remote(&remote) => Ok(false),
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
        Outcome::TimedOut => Err(ImportError::TimedOut(repo.project())),
        Outcome::Failed(said) => Err(clone::explain(Failure {
            output: &said,
            project: repo.project(),
            host: repo.host_name(),
            machine: &row.name,
            had_token: token.is_some(),
        })),
    }
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
