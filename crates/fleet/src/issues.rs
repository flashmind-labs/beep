//! A project's issues and pull requests, read from its git host through a machine that has the
//! project. No server is involved: the desktop asks the machine's own `gh` or `teacli`.

use crate::{
    desk::Desk,
    error::Result,
    open_items::merge,
    projects::{DeskMachines, DeskProject},
    records::{now_ms, time},
};
use beep_projects::{
    credentials::{Slot, last4},
    git_items::board::{Login, OpenedItems, read},
    issues::{
        MachineView, issue_view, list_open, list_open_prs, machine_view, pull_request_head,
        pull_request_view,
    },
};
use beep_proto::{
    LabeledProjectLists, ProjectIssue, ProjectLists, ProjectPullRequest, PullRequestHead,
};
use secrecy::ExposeSecret;
use std::future::Future;
use std::{
    sync::{Mutex, PoisonError},
    time::{Duration, Instant},
};
use uuid::Uuid;

/// A project's id on this desktop. The apps address a project by an id, and with no server to
/// make one it comes from the name, the same on every desktop.
pub fn project_id(name: &str) -> Uuid {
    Uuid::new_v5(
        &Uuid::NAMESPACE_URL,
        format!("beep:project:{name}").as_bytes(),
    )
}

impl Desk {
    /// The name of the project `id` is the id of, when this mesh has it.
    pub fn project_name(&self, id: Uuid) -> Option<String> {
        crate::records::projects(&self.mesh)
            .into_iter()
            .find_map(|stored| (project_id(&stored.name) == id).then_some(stored.name))
    }

    /// What reading `name`'s host needs. With `local_only`, and a checkout of the project on this
    /// Mac, only this Mac is asked, so nothing waits on another machine.
    pub(crate) async fn host_reads(
        &self,
        name: &str,
        local_only: bool,
    ) -> Result<(DeskProject, DeskMachines)> {
        let store = DeskProject::of(&self.mesh, name)?;
        if local_only
            && store.is_on(self.local)
            && let Some(local) = self.local_machine().await
        {
            return Ok((store, DeskMachines::of(&[local])));
        }
        Ok((store, DeskMachines::of(&self.machines().await)))
    }

    /// Runs `read` against what reading the project's host needs, asking this Mac alone when the
    /// project is checked out here, and the others only if that failed.
    async fn host_first<T, R, F>(&self, project: &str, read: R) -> Result<T>
    where
        R: Fn(DeskProject, DeskMachines) -> F,
        F: Future<Output = Result<T>>,
    {
        let (store, machines) = self.host_reads(project, true).await?;
        let here = store.is_on(self.local);
        match read(store, machines).await {
            Err(_) if here => {
                let (store, machines) = self.host_reads(project, false).await?;
                read(store, machines).await
            }
            done => done,
        }
    }

    /// The project's open issues no task tracks.
    pub async fn issues(&self, project: &str) -> Result<Vec<ProjectIssue>> {
        self.host_first(project, |store, machines| async move {
            let open = list_open(&store, &machines).await?;
            Ok(open.into_iter().map(issue_view).collect())
        })
        .await
    }

    /// Every open pull request of the project, newest first.
    pub async fn pull_requests(&self, project: &str) -> Result<Vec<ProjectPullRequest>> {
        self.host_first(project, |store, machines| async move {
            let listed = list_open_prs(&store, &machines).await?;
            Ok(listed.into_iter().map(pull_request_view).collect())
        })
        .await
    }

    /// The branch pull request `number` is made from.
    pub async fn pr_head(&self, project: &str, number: u64) -> Result<PullRequestHead> {
        self.host_first(project, |store, machines| async move {
            let head = pull_request_head(&store, &machines, number).await?;
            Ok(PullRequestHead {
                number: head.number,
                branch: head.branch,
                fork: head.fork,
            })
        })
        .await
    }

    /// The project's open pull requests, with their labels, and its issues no task tracks. A list
    /// that cannot be read says why beside the other.
    pub async fn lists(&self, project: &str) -> Result<LabeledProjectLists> {
        let local = self.lists_on(project, true).await?;
        let failed = local.lists.prs_error.is_some() || local.lists.issues_error.is_some();
        if failed && DeskProject::of(&self.mesh, project)?.is_on(self.local) {
            // This Mac could not read it; the others may.
            return self.lists_on(project, false).await;
        }
        Ok(local)
    }

    async fn lists_on(&self, project: &str, local_only: bool) -> Result<LabeledProjectLists> {
        let (store, machines) = self.host_reads(project, local_only).await?;
        let (prs, issues) = tokio::join!(
            beep_projects::issues::list_open_prs(&store, &machines),
            list_open(&store, &machines),
        );
        let (mut pull_requests, mut pr_labels) = (Vec::new(), Vec::new());
        let prs_error = match prs {
            Ok(listed) => {
                for one in listed {
                    pr_labels.push(one.pr.labels.clone());
                    pull_requests.push(pull_request_view(one));
                }
                None
            }
            Err(e) => Some(e.to_string()),
        };
        let (issues, issues_error) = match issues {
            Ok(open) => (open.into_iter().map(issue_view).collect(), None),
            Err(e) => (Vec::new(), Some(e.to_string())),
        };
        Ok(LabeledProjectLists {
            lists: ProjectLists {
                pull_requests,
                prs_error,
                issues,
                issues_error,
                synced_at: Some(time(now_ms())),
            },
            pr_labels,
        })
    }

    /// What the machine that holds `project` says about its account and the repository, asking
    /// this Mac alone when the project is checked out here, and the others only if that failed.
    pub(crate) async fn machine_view(&self, project: &str) -> Option<MachineView> {
        let (store, machines) = self.host_reads(project, true).await.ok()?;
        let here = store.is_on(self.local);
        match machine_view(&store, &machines).await {
            None if here => {
                let (store, machines) = self.host_reads(project, false).await.ok()?;
                machine_view(&store, &machines).await
            }
            view => view,
        }
    }

    /// The open pull requests and issues of every imported project, read through the machines,
    /// with the user's own on every git host they have a token for.
    pub async fn git_items(&self) -> Result<OpenedItems> {
        let opened = self.opened();
        let logins: Vec<Login<'_>> = opened
            .iter()
            .map(|opened| Login {
                slot: &opened.slot,
                secret: &opened.secret,
                account: opened.github.as_ref().map(|github| github.login.as_str()),
            })
            .filter(|login| matches!(login.slot, Slot::GitHub | Slot::Forgejo(_)))
            .filter(|login| !login.secret.expose_secret().is_empty())
            .collect();
        let mut names = project_names(&self.mesh);
        names.sort();
        let key: Vec<String> = logins
            .iter()
            .map(|login| {
                let tail = last4(login.secret.expose_secret());
                format!("{:?}/{tail}/{:?}", login.slot, login.account)
            })
            .chain(names.iter().map(|name| format!("project/{name}")))
            .collect();
        if let Some(kept) = fresh(&key) {
            return Ok(kept);
        }
        let (tokens, projects) =
            tokio::join!(read(&self.hosts, &logins), self.read_projects(&names));
        let access = self.access(&logins, &projects).await;
        let items = merge(tokens?, projects, &access);
        *BOARD.lock().unwrap_or_else(PoisonError::into_inner) = Some(Kept {
            key,
            at: Instant::now(),
            items: items.clone(),
        });
        Ok(items)
    }
}

/// How long the list is kept when every host answered.
const FRESH: Duration = Duration::from_secs(180);
/// How long it is kept when a host failed, to try that one again sooner.
const FRESH_AFTER_ERROR: Duration = Duration::from_secs(60);

/// The last list, and the tokens it was read with, so a token change reads again.
struct Kept {
    key: Vec<String>,
    at: Instant,
    items: OpenedItems,
}

/// A host's rate limit is not a desktop's to spend on every call, so Home's list is kept for a few
/// minutes, as the server keeps it.
static BOARD: Mutex<Option<Kept>> = Mutex::new(None);

fn fresh(key: &[String]) -> Option<OpenedItems> {
    let kept = BOARD.lock().unwrap_or_else(PoisonError::into_inner);
    let kept = kept.as_ref()?;
    let failed = kept
        .items
        .items
        .found
        .hosts
        .iter()
        .any(|host| host.error.is_some());
    let ttl = if failed { FRESH_AFTER_ERROR } else { FRESH };
    (kept.key == key && kept.at.elapsed() < ttl).then(|| kept.items.clone())
}

/// The names of the projects the mesh records.
pub fn project_names(mesh: &beep_mesh::Mesh) -> Vec<String> {
    crate::records::projects(mesh)
        .into_iter()
        .map(|stored| stored.name)
        .collect()
}

/// Whether the mesh records `project`, which then has no server to ask about its places.
pub fn has_project(mesh: &beep_mesh::Mesh, project: &str) -> bool {
    crate::records::find_project(mesh, project).is_some()
}

/// Where `project` is checked out, by machine and folder, as the mesh records it.
pub fn places(mesh: &beep_mesh::Mesh, project: &str) -> Vec<(iroh::EndpointId, String)> {
    crate::records::find_project(mesh, project)
        .map(|stored| {
            stored
                .places
                .into_iter()
                .map(|(machine, place)| (machine, place.folder))
                .collect()
        })
        .unwrap_or_default()
}
