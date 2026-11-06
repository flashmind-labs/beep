//! Removing a project from Beep, on request or with a removed machine. Files are never deleted,
//! and what runs on a machine is left to it: the server reaches none.

use crate::{
    account::user::User,
    app::App,
    projects::{
        fleet::RecordedFleet,
        lifecycle::{self, fill_slots},
    },
    store::RemovedProject,
};
use anyhow::Result;
use std::sync::Arc;

/// Removes project `name` and forgets what ran for it; `false` when absent.
pub(crate) async fn remove_project(app: &Arc<App>, user: &Arc<User>, name: &str) -> Result<bool> {
    let Some(removed) = user.remove_project_rows(name).await? else {
        return Ok(false);
    };
    log::info!(
        "project removed; user={} project={name} workers={} terminals={}",
        user.id(),
        removed.workers.len(),
        removed.terminals.len()
    );
    wind_down(app, user, &removed).await;
    Ok(true)
}

/// Winds down after a removed machine: the tasks it left without a worker are blocked.
pub(crate) async fn machine_removed(app: &Arc<App>, user: &Arc<User>, removed: &RemovedProject) {
    let fleet = RecordedFleet::new(app, user);
    for worker in &removed.workers {
        if let Err(e) = lifecycle::machine_removed(&fleet, user, worker).await {
            log::warn!("machine removal: a worker's task not blocked: {e:#}");
        }
    }
    wind_down(app, user, removed).await;
}

/// Forgets what ran for a removed project, each failure logged.
async fn wind_down(app: &Arc<App>, user: &Arc<User>, removed: &RemovedProject) {
    let fleet = RecordedFleet::new(app, user);
    for project in &removed.projects {
        app.projects.forget_project(*project);
    }
    for worker in &removed.workers {
        app.projects.untrack(worker.id);
    }
    if !removed.workers.is_empty()
        && let Err(e) = fill_slots(&fleet, user).await
    {
        log::warn!("project removal: queued tasks not started: {e:#}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::watchers::testing::{Harness, Scripted};
    use beep_projects::imports::records::NewImport;
    use beep_projects::model::RepoName;

    const NAME: &str = "acme/widget";

    async fn import(user: &User, endpoint: &str, folder: &str) {
        user.save_import(NewImport {
            project: NAME,
            endpoint,
            folder,
            url: "https://github.com/acme/widget",
        })
        .await
        .expect("imported");
    }

    async fn places(user: &User) -> Vec<(String, String)> {
        user.imports()
            .await
            .expect("listed")
            .into_iter()
            .map(|row| (row.endpoint, row.folder))
            .collect()
    }

    #[tokio::test]
    async fn a_removed_project_can_be_imported_again_afresh() {
        let harness = Harness::new(Scripted::default()).await;
        let (app, alice) = (&harness.app, &harness.alice);
        let repo = RepoName::parse(NAME).expect("a repo name");
        import(alice, "laptop", "/home/user/widget").await;
        let first = alice.ensure_project(&repo).await.expect("made");
        let chat = alice.ensure_project_agent(first.id).await.expect("a chat");

        assert!(remove_project(app, alice, NAME).await.expect("removed"));
        assert!(places(alice).await.is_empty());
        assert!(alice.projects().await.expect("listed").is_empty());
        assert!(!alice.owns_session(chat).await.expect("read"));
        assert!(!remove_project(app, alice, NAME).await.expect("read"));

        import(alice, "laptop", "/home/user/widget").await;
        assert_eq!(
            places(alice).await,
            [("laptop".to_owned(), "/home/user/widget".to_owned())]
        );
        let second = alice.ensure_project(&repo).await.expect("made");
        assert_ne!(second.id, first.id);
        let fresh = alice.ensure_project_agent(second.id).await.expect("a chat");
        assert_ne!(fresh, chat);
    }
}
