//! Removing a project from Beep, on request or once no machine has its folder. Files are never deleted.

use crate::{
    account::user::User,
    app::App,
    nodes::{Online, SHELL, push_sync},
    projects::{
        fleet::{Fleet, NodeFleet},
        lifecycle::{self, fill_slots},
        review::drive::close_session,
    },
    store::RemovedProject,
};
use anyhow::Result;
use beep_projects::{
    Shell,
    imports::gone::{self, Place},
};
use std::{sync::Arc, time::Duration};

/// The longest looking for a machine's project folders may take.
const LOOK_LIMIT: Duration = Duration::from_secs(20);

/// Removes project `name` and stops what runs for it; `false` when absent. Offline machines keep running.
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
    if removed.synced {
        push_sync(&app.nodes, user, None).await?;
    }
    Ok(true)
}

/// Winds down after a removed machine: the tasks it left without a worker are blocked and what ran for its projects stops.
pub(crate) async fn machine_removed(app: &Arc<App>, user: &Arc<User>, removed: &RemovedProject) {
    let fleet = NodeFleet::new(app, user);
    for worker in &removed.workers {
        if let Err(e) = lifecycle::machine_removed(&fleet, user, worker).await {
            log::warn!("machine removal: a worker's task not blocked: {e:#}");
        }
    }
    wind_down(app, user, removed).await;
}

/// Stops what ran for a removed project, each failure logged.
async fn wind_down(app: &Arc<App>, user: &Arc<User>, removed: &RemovedProject) {
    let fleet = NodeFleet::new(app, user);
    for project in &removed.projects {
        app.projects.forget_project(*project);
    }
    for worker in &removed.workers {
        app.projects.untrack(worker.id);
        if let Some(session) = &worker.session
            && let Err(e) = fleet
                .close_session(&worker.machine, worker.agent, session)
                .await
        {
            log::warn!("project removal: a worker's session was not closed: {e:#}");
        }
    }
    for terminal in &removed.terminals {
        let Some(node) = online(app, user, &terminal.endpoint) else {
            continue;
        };
        if let Err(e) = node.stop_terminal(terminal).await {
            log::warn!("project removal: a terminal was not stopped: {e:#}");
        }
    }
    for reviewer in &removed.reviewers {
        if let Some(node) = online(app, user, &reviewer.endpoint) {
            close_session(&node, &reviewer.agent, &reviewer.session).await;
        }
    }
    if !removed.workers.is_empty()
        && let Err(e) = fill_slots(&fleet, user).await
    {
        log::warn!("project removal: queued tasks not started: {e:#}");
    }
}

/// The user's machine `endpoint` when it is online now.
fn online(app: &App, user: &User, endpoint: &str) -> Option<Online> {
    app.nodes
        .online(endpoint)
        .filter(|online| online.user == user.id())
}

/// Drops `endpoint` from projects whose folder it no longer has; returns how many were dropped.
pub(crate) async fn drop_missing(
    app: &Arc<App>,
    user: &Arc<User>,
    node: &Online,
    endpoint: &str,
) -> Result<usize> {
    if !node.offers(SHELL) {
        return Ok(0);
    }
    let places = gone::places(
        &user.imports().await?,
        &user.project_syncs().await?,
        endpoint,
    );
    if places.is_empty() {
        return Ok(0);
    }
    let ran = node.run(&gone::script(&places), LOOK_LIMIT).await?;
    let finished = !ran.timed_out && ran.exit_code == Some(0);
    let missing: Vec<&Place> = gone::missing(&ran.output, finished, places.len())
        .into_iter()
        .filter_map(|n| places.get(n))
        .collect();
    drop_places(app, user, endpoint, &missing).await
}

/// Drops `endpoint` from each of `missing`'s projects, removing orphans; returns how many.
pub(crate) async fn drop_places(
    app: &Arc<App>,
    user: &Arc<User>,
    endpoint: &str,
    missing: &[&Place],
) -> Result<usize> {
    let mut dropped = 0;
    let mut resync = false;
    for place in missing {
        let done = user
            .drop_project_folder(&place.project, endpoint, &place.folder)
            .await?;
        if !done.dropped {
            continue;
        }
        dropped += 1;
        resync |= done.synced;
        log::info!(
            "project folder gone; user={} project={} node={endpoint} left={}",
            user.id(),
            place.project,
            done.left
        );
        if done.left == 0 {
            remove_project(app, user, &place.project).await?;
        }
    }
    if resync {
        push_sync(&app.nodes, user, None).await?;
    }
    if dropped > 0 {
        user.changed("");
    }
    Ok(dropped)
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

    fn gone(folder: &str) -> Place {
        Place {
            project: NAME.into(),
            folder: folder.into(),
        }
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

    #[tokio::test]
    async fn a_missing_folder_drops_its_machine_and_the_last_one_the_project() {
        let harness = Harness::new(Scripted::default()).await;
        let (app, alice) = (&harness.app, &harness.alice);
        import(alice, "laptop", "/home/user/widget").await;
        import(alice, "build", "/srv/widget").await;
        let repo = RepoName::parse(NAME).expect("a repo name");
        alice.ensure_project(&repo).await.expect("made");

        let dropped = drop_places(app, alice, "build", &[&gone("/srv/widget")])
            .await
            .expect("dropped");
        assert_eq!(dropped, 1);
        assert_eq!(
            places(alice).await,
            [("laptop".to_owned(), "/home/user/widget".to_owned())]
        );
        assert_eq!(alice.projects().await.expect("listed").len(), 1);

        // A folder the machine's import no longer names is left alone.
        let stale = drop_places(app, alice, "laptop", &[&gone("/home/user/old")])
            .await
            .expect("looked");
        assert_eq!(stale, 0);
        assert_eq!(places(alice).await.len(), 1);

        drop_places(app, alice, "laptop", &[&gone("/home/user/widget")])
            .await
            .expect("dropped");
        assert!(places(alice).await.is_empty());
        assert!(alice.projects().await.expect("listed").is_empty());
    }

    #[tokio::test]
    async fn an_offline_machine_drops_nothing() {
        let harness = Harness::new(Scripted::default()).await;
        let (app, alice) = (&harness.app, &harness.alice);
        import(alice, "build", "/srv/widget").await;
        // No node is online: the look ends at once and proves nothing.
        assert!(app.checkouts.check(app, alice, "build"));
        app.checkouts.wait_idle().await;
        assert_eq!(
            places(alice).await,
            [("build".to_owned(), "/srv/widget".to_owned())]
        );
    }
}
