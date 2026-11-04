//! Making a new project: a repository on a git host cloned onto the machines picked, or `git init`
//! on one machine. Nothing here deletes a repository; a clone that fails leaves it for another try.

use super::{
    credential_of,
    fork::target_of,
    service::{Chosen, Import, choose_machine, forgejo_of, import, project},
};
use crate::{
    account::{credentials::host_client, user::User},
    app::App,
    nodes::{Online, push_sync},
    protocol::{
        CheckoutFailure, CreateProject, CreatedProject, GitHostKind, ImportedProject,
        NewRepository, ProjectForge,
    },
    store::{NewImport, NodeRow, SyncRow},
};
use beep_projects::credentials::{Opened, Slot};
use beep_projects::git_items::new_repo;
use beep_projects::git_items::{
    Caller, HostError,
    new_repo::{CreateError, Forge, NewRepo},
};
use beep_projects::{
    imports::{
        ImportError,
        clone::tail,
        place::{Folders, project_folder},
        repo::{Host, RepoRef, valid_part},
    },
    quote,
};
use futures::{StreamExt, future::join_all, stream};
use reqwest::Client;
use secrecy::ExposeSecret;
use std::{sync::Arc, time::Duration};

const GITHUB: &str = "github.com";
/// The owner of a project with no git host, as projects are named `owner/repo`.
const LOCAL_OWNER: &str = "local";
/// How many machines clone at once.
const CLONES_AT_ONCE: usize = 4;
/// The longest `git init` may take.
const INIT_LIMIT: Duration = Duration::from_secs(60);
/// The most output kept from `git init`.
const MAX_INIT_OUTPUT: usize = 64 * 1024;
/// What the init script prints last, before what it found.
const MARK: &str = "BEEP_INIT:";

/// Each git host the user has a token for, with who a repository there can belong to.
pub(crate) async fn project_forges(
    app: &App,
    user: &User,
) -> Result<Vec<ProjectForge>, ImportError> {
    let credentials = user.git_credentials(&app.credential_key).await?;
    let client = host_client()?;
    let usable = |opened: &&Opened| !opened.secret.expose_secret().is_empty();
    let mut github: Vec<&Opened> = credentials
        .iter()
        .filter(|opened| opened.github.is_some())
        .filter(usable)
        .collect();
    github.sort_by_key(|opened| !opened.github.as_ref().is_some_and(|login| login.default));
    let github = async {
        if github.is_empty() {
            return None;
        }
        let kind = GitHostKind::GitHub {};
        Some(forge_row(app, &client, GITHUB.to_owned(), kind, &github).await)
    };
    let forgejo = credentials.iter().filter(usable).filter_map(|opened| {
        let Slot::Forgejo(url) = &opened.slot else {
            return None;
        };
        let client = &client;
        Some(async move {
            let kind = GitHostKind::Forgejo {};
            forge_row(app, client, url.host(), kind, &[opened]).await
        })
    });
    let (github, forgejo) = tokio::join!(github, join_all(forgejo));
    Ok(github.into_iter().chain(forgejo).collect())
}

/// One host's row: the owners every token of `accounts` gives, the error when none answered.
async fn forge_row(
    app: &App,
    client: &Client,
    host: String,
    kind: GitHostKind,
    accounts: &[&Opened],
) -> ProjectForge {
    let name = host.as_str();
    let asked = join_all(accounts.iter().map(|opened| async move {
        let target = target_of(opened)
            .await
            .ok_or_else(|| HostError::Private(name.to_owned()))?;
        let caller = Caller {
            client,
            calls: app.git_items.calls(),
            host: name,
            token: &opened.secret,
        };
        let forge = Forge {
            api: target.api,
            base: &target.base,
            login: target.login,
        };
        new_repo::owners(&caller, &forge).await
    }))
    .await;
    let mut owners: Vec<String> = Vec::new();
    let mut error = None;
    for answer in asked {
        match answer {
            Ok(found) => {
                for owner in found {
                    if !owners
                        .iter()
                        .any(|known| known.eq_ignore_ascii_case(&owner))
                    {
                        owners.push(owner);
                    }
                }
            }
            Err(e) => error = Some(e.to_string()),
        }
    }
    ProjectForge {
        host,
        kind,
        error: error.filter(|_| owners.is_empty()),
        owners,
    }
}

/// Makes the project `request` asks for. Every machine is checked first, so an offline one stops it
/// before anything exists.
pub(crate) async fn create_project(
    app: &Arc<App>,
    user: &Arc<User>,
    request: &CreateProject,
) -> Result<CreatedProject, ImportError> {
    let name = request.name.trim();
    if !valid_part(name) || name.ends_with(".git") {
        return Err(ImportError::BadName);
    }
    let mut ids: Vec<&str> = Vec::new();
    for id in request.machine_ids.iter().map(|id| id.trim()) {
        if !id.is_empty() && !ids.contains(&id) {
            ids.push(id);
        }
    }
    if ids.is_empty() {
        return Err(ImportError::NoMachine);
    }
    if request.repository.is_none() && ids.len() > 1 {
        return Err(ImportError::LocalOnOne);
    }
    let mut chosen = Vec::with_capacity(ids.len());
    for id in ids {
        chosen.push(choose_machine(app, user, Some(id)).await?);
    }
    match &request.repository {
        Some(new) => {
            let repo = create_repository(app, user, new, name).await?;
            Ok(clone_onto(app, user, &repo, &chosen, None).await)
        }
        None => create_local(user, name, chosen).await,
    }
}

/// Creates the repository on its host with the owner's token.
async fn create_repository(
    app: &App,
    user: &User,
    new: &NewRepository,
    name: &str,
) -> Result<RepoRef, ImportError> {
    let credentials = user.git_credentials(&app.credential_key).await?;
    let host = if new.host == GITHUB {
        Host::GitHub
    } else {
        let server = forgejo_of(&credentials)
            .into_iter()
            .find(|server| server.host() == new.host);
        Host::Forgejo(server.ok_or_else(|| ImportError::UnknownHost(new.host.clone()))?)
    };
    let wanted = RepoRef::new(host, &new.owner, name)?;
    let host_name = wanted.host_name();
    let opened = credential_of(&wanted, &credentials)
        .ok_or_else(|| ImportError::NoToken(host_name.clone()))?;
    let target = target_of(opened)
        .await
        .ok_or_else(|| ImportError::Forge(HostError::Private(host_name.clone())))?;
    let client = host_client()?;
    let caller = Caller {
        client: &client,
        calls: app.git_items.calls(),
        host: &host_name,
        token: &opened.secret,
    };
    let forge = Forge {
        api: target.api,
        base: &target.base,
        login: target.login,
    };
    let repository = NewRepo {
        owner: &new.owner,
        name,
        private: new.private,
    };
    let made = new_repo::create(&caller, &forge, &repository)
        .await
        .map_err(|e| match e {
            CreateError::NameTaken => ImportError::NameTaken {
                repo: wanted.project(),
                host: host_name.clone(),
            },
            CreateError::Denied => ImportError::CannotCreate {
                owner: new.owner.clone(),
                host: host_name.clone(),
            },
            CreateError::Host(e) => ImportError::Forge(e),
        })?;
    log::info!("repository created; project={made} host={host_name}");
    wanted.sibling(&made)
}

/// Imports `repo` onto each machine, under `parent` when given, else its projects folder; those that
/// landed join sync. A machine that fails is listed and the repository stays.
async fn clone_onto(
    app: &Arc<App>,
    user: &Arc<User>,
    repo: &RepoRef,
    machines: &[Chosen],
    parent: Option<&str>,
) -> CreatedProject {
    let url = repo.web_url();
    // Collected first: a stream's `map` closure over borrows is not `Send` enough for the router.
    let clones: Vec<_> = machines
        .iter()
        .map(|machine| {
            import(
                app,
                user,
                Import {
                    repo: &url,
                    machine_id: Some(&machine.row.endpoint),
                    folder: None,
                    parent,
                },
            )
        })
        .collect();
    let answers: Vec<_> = stream::iter(clones)
        .buffered(CLONES_AT_ONCE)
        .collect()
        .await;
    let mut imported = Vec::new();
    let mut failed = Vec::new();
    for (machine, answer) in machines.iter().zip(answers) {
        match answer {
            Ok(landed) => imported.push(landed),
            Err(e) => {
                log::info!(
                    "clone failed; project={} node={}",
                    repo.project(),
                    machine.row.endpoint
                );
                failed.push(CheckoutFailure {
                    machine_id: machine.row.endpoint.clone(),
                    machine_name: machine.row.name.clone(),
                    why: e.to_string(),
                });
            }
        }
    }
    if let Err(e) = join_sync(app, user, &imported, &mut failed).await {
        log::warn!(
            "new project not synced; project={} error={e:#}",
            repo.project()
        );
    }
    CreatedProject {
        project: repo.project(),
        url,
        imported,
        failed,
    }
}

/// Starts syncing the checkouts with each other; a machine that refuses is listed and keeps its copy.
async fn join_sync(
    app: &App,
    user: &User,
    imported: &[ImportedProject],
    failed: &mut Vec<CheckoutFailure>,
) -> anyhow::Result<()> {
    let syncing: Vec<&ImportedProject> = imported
        .iter()
        .filter(|landed| {
            app.nodes
                .online(&landed.machine_id)
                .is_some_and(|node| node.syncs())
        })
        .collect();
    if syncing.len() < 2 {
        return Ok(());
    }
    for landed in &syncing {
        user.set_project_sync(&SyncRow {
            project: landed.project.clone(),
            endpoint: landed.machine_id.clone(),
            folder: landed.folder.clone(),
        })
        .await?;
    }
    let mut refused = false;
    for refusal in push_sync(&app.nodes, user, None).await? {
        let endpoint = refusal.node.to_string();
        let Some(landed) = syncing.iter().find(|landed| {
            landed.machine_id == endpoint && landed.project == refusal.refused.project
        }) else {
            continue;
        };
        user.remove_project_sync(&landed.project, &endpoint).await?;
        refused = true;
        failed.push(CheckoutFailure {
            machine_id: endpoint,
            machine_name: landed.machine_name.clone(),
            why: ImportError::CannotSync {
                machine: landed.machine_name.clone(),
                why: refusal.refused.message,
            }
            .to_string(),
        });
    }
    if refused {
        push_sync(&app.nodes, user, None).await?;
    }
    Ok(())
}

/// `git init` in the machine's projects folder, imported there with no git host.
async fn create_local(
    user: &User,
    name: &str,
    mut chosen: Vec<Chosen>,
) -> Result<CreatedProject, ImportError> {
    let Some(Chosen { row, node }) = chosen.pop() else {
        return Err(ImportError::NoMachine);
    };
    let project_name = format!("{LOCAL_OWNER}/{name}");
    let imports = user.imports().await?;
    if imports
        .iter()
        .any(|one| one.project.eq_ignore_ascii_case(&project_name))
    {
        return Err(ImportError::ProjectExists(name.to_owned()));
    }
    let env = row.env.as_ref();
    let folders = Folders {
        os: env.map(|env| env.os.as_str()),
        home: env.map(|env| env.home.as_str()),
        workspace: node.workspace.as_deref(),
    };
    let folder =
        project_folder(folders, &[name]).ok_or_else(|| ImportError::NoHome(row.name.clone()))?;
    init(&node, &row, &folder).await?;
    let saved = user
        .save_import(NewImport {
            project: &project_name,
            endpoint: &row.endpoint,
            folder: &folder,
            url: "",
        })
        .await?;
    log::info!(
        "local project created; project={project_name} node={}",
        row.endpoint
    );
    Ok(CreatedProject {
        project: project_name,
        url: String::new(),
        imported: vec![project(saved, row.name, true)],
        failed: Vec::new(),
    })
}

/// Makes `folder` a new repository, unless something is there.
async fn init(node: &Online, row: &NodeRow, folder: &str) -> Result<(), ImportError> {
    let mut output = String::new();
    let mut on_output = |text: &str| {
        if output.len() < MAX_INIT_OUTPUT {
            output.push_str(text);
        }
    };
    node.exec(&init_script(folder), INIT_LIMIT, &mut on_output)
        .await
        .map_err(|e| ImportError::Link {
            machine: row.name.clone(),
            detail: format!("{e:#}"),
        })?;
    let said = output
        .lines()
        .rev()
        .find_map(|line| line.trim_end_matches('\r').strip_prefix(MARK));
    match said.map(str::trim) {
        Some("made") => Ok(()),
        Some("occupied") => Err(ImportError::Occupied {
            folder: folder.to_owned(),
            machine: row.name.clone(),
        }),
        Some("nogit") => Err(ImportError::NoGit(row.name.clone())),
        _ => Err(ImportError::InitFailed {
            machine: row.name.clone(),
            detail: tail(&output),
        }),
    }
}

fn init_script(folder: &str) -> String {
    format!(
        "d={folder}\n\
         if [ -n \"$(ls -A \"$d\" 2>/dev/null)\" ]; then\n\
         \x20 printf '\\n{MARK} occupied\\n'\n\
         elif ! command -v git >/dev/null 2>&1; then\n\
         \x20 printf '\\n{MARK} nogit\\n'\n\
         elif mkdir -p \"$d\" && git -C \"$d\" init --quiet; then\n\
         \x20 printf '\\n{MARK} made\\n'\n\
         fi\n",
        folder = quote(folder),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        projects::node_tests::{TestNode, World},
        store::NewCredential,
    };
    use beep_projects::model::ForgejoUrl;
    use secrecy::SecretString;
    use std::{fs, path::Path, process::Command};
    use tempfile::TempDir;
    use tokio::time::timeout;

    const LIMIT: Duration = Duration::from_secs(30);

    async fn chosen(world: &World, node: &TestNode) -> Chosen {
        choose_machine(
            &world.harness.app,
            &world.harness.alice,
            Some(&node.id.to_string()),
        )
        .await
        .expect("the machine is online")
    }

    fn git(dir: &Path, args: &[&str]) {
        let out = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .output()
            .expect("git runs");
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    #[tokio::test]
    async fn git_init_runs_on_the_machine_and_refuses_a_folder_in_use() {
        let world = World::new().await;
        let node = world.node("build box").await;
        let Chosen { row, node: online } = chosen(&world, &node).await;
        let folder = node.tmp.join("code/widget");
        let path = folder.to_str().expect("the path is text");
        init(&online, &row, path).await.expect("git init runs");
        assert!(folder.join(".git/HEAD").exists());
        assert_eq!(
            init(&online, &row, path).await,
            Err(ImportError::Occupied {
                folder: path.to_owned(),
                machine: "build box".to_owned(),
            })
        );
    }

    #[tokio::test]
    async fn a_machine_that_fails_is_listed_and_the_others_keep_the_project() {
        let world = World::new().await;
        let (app, alice) = (&world.harness.app, &world.harness.alice);
        let gone = world.node("gone").await;
        let here = world.node("here").await;
        let url: ForgejoUrl = "http://127.0.0.1:1".parse().expect("a server URL");
        let slot = Slot::Forgejo(url.clone());
        alice
            .save_credential(
                &app.credential_key,
                NewCredential {
                    slot: &slot,
                    secret: &SecretString::from("tok"),
                    last4: "tok",
                    account: None,
                    to_machines: true,
                },
            )
            .await
            .expect("the token saves");
        let repo = RepoRef::new(Host::Forgejo(url), "acme", "widget").expect("a repository");
        // Nothing listens on the server, so a checkout already there stands in for a clone.
        let parent = TempDir::new().expect("a temp folder");
        let checkout = parent.path().join("acme/widget");
        fs::create_dir_all(&checkout).expect("a folder");
        git(&checkout, &["init", "--quiet"]);
        git(&checkout, &["remote", "add", "origin", &repo.clone_url()]);
        let machines = [chosen(&world, &gone).await, chosen(&world, &here).await];
        // It drops after the check that comes before creating the repository.
        gone.online.connection.close(0u32.into(), b"test");
        timeout(LIMIT, async {
            while app.nodes.online(&gone.id.to_string()).is_some() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("the node goes offline");
        let parent = parent.path().to_str().expect("the path is text");
        let created = clone_onto(app, alice, &repo, &machines, Some(parent)).await;
        assert_eq!(created.project, "acme/widget");
        assert_eq!(created.imported.len(), 1);
        assert_eq!(created.imported[0].machine_name, "here");
        assert_eq!(created.failed.len(), 1);
        assert_eq!(created.failed[0].machine_name, "gone");
        assert_eq!(created.failed[0].why, "gone is offline.");
        let listed = alice.imports().await.expect("the imports");
        assert_eq!(listed.len(), 1, "{listed:?}");
    }
}
