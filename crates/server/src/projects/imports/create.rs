//! Making a new project: a repository on a git host cloned onto the machines picked, or `git init`
//! on one machine. Nothing here deletes a repository; a clone that fails leaves it for another try.

use super::context::Context;
use crate::{account::user::User, app::App};
use beep_projects::imports::{
    ImportError,
    flow::{self},
    hosts::project_forges as forges,
};
use beep_proto::{CreateProject, CreatedProject, ProjectForge};
use std::sync::Arc;

/// Each git host the user has a token for, with who a repository there can belong to.
pub(crate) async fn project_forges(
    app: &Arc<App>,
    user: &Arc<User>,
) -> Result<Vec<ProjectForge>, ImportError> {
    let context = Context::load(app, user).await?;
    forges(context.hosts(), &context.credentials).await
}

/// Makes the project `request` asks for.
pub(crate) async fn create_project(
    app: &Arc<App>,
    user: &Arc<User>,
    request: &CreateProject,
) -> Result<CreatedProject, ImportError> {
    let context = Context::load(app, user).await?;
    flow::create_project(&context.env(user), request).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{projects::node_tests::World, store::NewCredential};
    use beep_projects::{
        credentials::Slot,
        imports::{
            ImportError,
            flow::clone_onto,
            init::init,
            repo::{Host, RepoRef},
        },
        model::ForgejoUrl,
    };
    use secrecy::SecretString;
    use std::time::Duration;
    use std::{fs, path::Path, process::Command};
    use tempfile::TempDir;
    use tokio::time::timeout;

    const LIMIT: Duration = Duration::from_secs(30);

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
        let folder = node.tmp.join("code/widget");
        let path = folder.to_str().expect("the path is text");
        init(&node.online, "build box", path)
            .await
            .expect("git init runs");
        assert!(folder.join(".git/HEAD").exists());
        assert_eq!(
            init(&node.online, "build box", path).await,
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
        world.node("here").await;
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
        let context = Context::load(app, alice).await.expect("the context loads");
        let env = context.env(alice);
        let machine = |name: &str| {
            context
                .machines
                .iter()
                .find(|machine| machine.name == name)
                .expect("the machine is listed")
        };
        let machines = [machine("gone"), machine("here")];
        let created = clone_onto(&env, &repo, &machines, Some(parent)).await;
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
