//! Reading the user's projects and what is live on them.

use crate::{
    account::user::User,
    app::App,
    protocol::{GitItem, GitItemKind, GitReview},
    store::TaskRow,
};
use beep_projects::model::RepoName;
use std::collections::HashMap;

/// Where a project is checked out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Place {
    pub(super) machine: String,
    /// The machine's key, as sessions name it.
    pub(super) endpoint: String,
    pub(super) folder: String,
}

/// One project as the store keeps it.
#[derive(Debug)]
pub(super) struct Project {
    pub(super) name: RepoName,
    pub(super) url: Option<String>,
    pub(super) places: Vec<Place>,
    /// Its tasks not done or dropped.
    pub(super) tasks: Vec<TaskRow>,
}

/// The user's projects by name, with their places and open tasks.
pub(super) async fn load(user: &User) -> anyhow::Result<Vec<Project>> {
    let machines: HashMap<String, String> = user
        .nodes()
        .await?
        .into_iter()
        .map(|node| (node.endpoint, node.name))
        .collect();
    let imports = user.imports().await?;
    let mut projects = Vec::new();

    for row in user.projects().await? {
        let name = row.name.to_string();
        let mut url = None;
        let mut places = Vec::new();
        // A machine since unlinked has no name and is left out.
        for import in imports
            .iter()
            .filter(|import| import.project.eq_ignore_ascii_case(&name))
        {
            url.get_or_insert_with(|| import.url.clone());

            if let Some(machine) = machines.get(&import.endpoint) {
                places.push(Place {
                    machine: machine.clone(),
                    endpoint: import.endpoint.clone(),
                    folder: import.folder.clone(),
                });
            }
        }

        for checkout in user.checkouts(row.id).await? {
            let endpoint = checkout.machine.to_string();
            let Some(machine) = machines.get(&endpoint) else {
                continue;
            };
            let place = Place {
                machine: machine.clone(),
                endpoint,
                folder: checkout.path.as_ref().to_owned(),
            };
            if !places.contains(&place) {
                places.push(place);
            }
        }

        places.sort_by(|a, b| a.machine.cmp(&b.machine).then(a.folder.cmp(&b.folder)));
        let tasks = user
            .project_tasks(row.id)
            .await?
            .into_iter()
            .filter(|task| task.state.is_open())
            .collect();
        projects.push(Project {
            name: row.name,
            url,
            places,
            tasks,
        });
    }

    Ok(projects)
}

/// What the git hosts say now.
#[derive(Debug, Default)]
pub(super) struct Live {
    pub(super) items: Vec<GitItem>,
    /// What could not be read, for the model.
    pub(super) notes: Vec<String>,
}

pub(super) async fn live(app: &App, user: &User) -> Live {
    let (items, notes) = git_items(app, user).await;
    Live { items, notes }
}

/// The user's open items on their git hosts, and notes on hosts that did not answer.
pub(super) async fn git_items(app: &App, user: &User) -> (Vec<GitItem>, Vec<String>) {
    let logins = match user.git_logins(&app.credential_key).await {
        Ok(logins) => logins,
        Err(e) => {
            log::warn!("projects tool: git logins not read: {e:#}");
            return (Vec::new(), vec!["PRs and issues could not be read.".into()]);
        }
    };

    if logins.is_empty() {
        return (
            Vec::new(),
            vec![
                "No GitHub or Forgejo token is saved in Settings > Credentials, so PRs and \
                 issues are not listed."
                    .into(),
            ],
        );
    }

    match app.git_items.list(user.id(), logins).await {
        Ok(listed) => {
            let listed = listed.found;
            let notes = listed
                .hosts
                .iter()
                .filter_map(|host| {
                    let error = host.error.as_deref()?;
                    Some(format!("{} did not answer: {error}", host.host))
                })
                .collect();
            (listed.items, notes)
        }
        Err(e) => {
            log::warn!("projects tool: git items not read: {e:#}");
            (Vec::new(), vec!["PRs and issues could not be read.".into()])
        }
    }
}

pub(super) fn items_of<'a>(project: &Project, live: &'a Live) -> Vec<&'a GitItem> {
    let name = project.name.to_string();
    live.items
        .iter()
        .filter(|item| item.repo.eq_ignore_ascii_case(&name))
        .collect()
}

/// A pull request waiting on the user: a review request, or theirs with changes asked or failing checks.
pub(super) fn waits_on_user(item: &GitItem) -> bool {
    item.kind == (GitItemKind::PullRequest {})
        && (item.review_requested
            || (item.authored
                && (item.review == (GitReview::ChangesRequested {}) || item.checks_failing)))
}
