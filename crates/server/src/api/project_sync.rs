//! Keeping a project's folder in step across the caller's machines: turning
//! it on or off per machine, how each machine stands, what sync leaves out,
//! and pinning a project to the machines it is on. The nodes sync node to
//! node; these requests only change and read their lists.

use super::{Reply, agent_sessions::linked_keys};
use crate::{
    nodes::{Online, push_sync},
    protocol::{
        node::sync::{ProjectStatus, Rule, SkippedPath, State},
        *,
    },
    router::Cx,
    store::{NodeRow, SyncRow},
};
use futures::{StreamExt, stream};
use iroh::EndpointId;
use std::collections::{BTreeMap, HashMap};

/// The longest project name.
const MAX_NAME: usize = 200;
/// How many machines are asked how they stand at once.
const ASKING: usize = 8;

fn project_name(name: &str) -> Result<&str, ApiError> {
    let name = name.trim();
    if name.is_empty() || name.len() > MAX_NAME || name.chars().any(char::is_control) {
        return Err(ApiError::bad_request(format!(
            "A project needs a name of 1 to {MAX_NAME} characters."
        )));
    }
    Ok(name)
}

/// The caller's machines whose keys are still linked, by id.
async fn machines(cx: &Cx) -> Result<HashMap<String, NodeRow>, ApiError> {
    let keys = linked_keys(cx).await?;
    Ok(cx
        .user
        .nodes()
        .await?
        .into_iter()
        .filter(|node| keys.contains(&node.endpoint))
        .map(|node| (node.endpoint.clone(), node))
        .collect())
}

/// The caller's machine `machine_id` when it is online now.
fn online(cx: &Cx, machine_id: &str) -> Option<Online> {
    cx.app
        .nodes
        .online(machine_id)
        .filter(|online| online.user == cx.user.id() && online.syncs())
}

/// How each of `endpoints` stands, for those online that answer.
async fn statuses(cx: &Cx, endpoints: Vec<String>) -> HashMap<String, Vec<ProjectStatus>> {
    stream::iter(endpoints.into_iter().filter_map(|id| online(cx, &id)))
        .map(|node| async move {
            let id = node.connection.remote_id().to_string();
            match node.sync_status().await {
                Ok(projects) => Some((id, projects)),
                Err(e) => {
                    log::debug!("sync status not read; node={id} error={e:#}");
                    None
                }
            }
        })
        .buffer_unordered(ASKING)
        .filter_map(|answer| async move { answer })
        .collect()
        .await
}

/// The caller's projects, or only `only`: every synced one, and every
/// pinned one.
async fn projects(cx: &Cx, only: Option<&str>) -> Result<Vec<ProjectSync>, ApiError> {
    let machines = machines(cx).await?;
    let rows: Vec<SyncRow> = cx
        .user
        .project_syncs()
        .await?
        .into_iter()
        .filter(|row| machines.contains_key(&row.endpoint))
        .filter(|row| only.is_none_or(|only| row.project == only))
        .collect();
    let mut endpoints: Vec<String> = rows.iter().map(|row| row.endpoint.clone()).collect();
    endpoints.sort();
    endpoints.dedup();
    let statuses = statuses(cx, endpoints).await;
    let mut out: BTreeMap<String, ProjectSync> = BTreeMap::new();
    for project in cx.user.pinned_projects().await? {
        if only.is_none_or(|only| project == only) {
            out.insert(project.clone(), empty(project, true));
        }
    }
    if let Some(only) = only {
        out.entry(only.to_owned())
            .or_insert_with(|| empty(only.to_owned(), false));
    }
    for row in rows {
        let name = machines
            .get(&row.endpoint)
            .map(|node| node.name.clone())
            .unwrap_or_default();
        let status = statuses
            .get(&row.endpoint)
            .map(|projects| projects.iter().find(|status| status.name == row.project));
        let machine = machine_sync(&row, name, status);
        out.entry(row.project.clone())
            .or_insert_with(|| empty(row.project, false))
            .machines
            .push(machine);
    }
    Ok(out.into_values().collect())
}

fn empty(project: String, pinned: bool) -> ProjectSync {
    ProjectSync {
        project,
        pinned,
        machines: Vec::new(),
    }
}

/// `status` is `None` when the machine is offline or did not answer, and
/// `Some(None)` when it answered without the project: it refused it.
fn machine_sync(
    row: &SyncRow,
    name: String,
    status: Option<Option<&ProjectStatus>>,
) -> MachineSync {
    let mut machine = MachineSync {
        machine_id: row.endpoint.clone(),
        machine_name: name,
        folder: row.folder.clone(),
        status: SyncStatus::Offline {},
        files: 0,
        bytes: 0,
        pending: 0,
        conflicts: Vec::new(),
        rescan_secs: None,
        hint: None,
        error: None,
    };
    match status {
        None => {}
        Some(None) => {
            machine.status = SyncStatus::Failed {};
            machine.error = Some("The machine is not syncing this folder.".to_owned());
        }
        Some(Some(status)) => {
            machine.status = match status.state {
                State::Scanning => SyncStatus::Scanning {},
                State::Syncing | State::Other => SyncStatus::Syncing {},
                State::Idle => SyncStatus::InSync {},
                State::Failed => SyncStatus::Failed {},
            };
            machine.files = status.files;
            machine.bytes = status.bytes;
            machine.pending = status.pending;
            machine.conflicts = status.conflicts.clone();
            machine.rescan_secs = status.rescan_secs;
            machine.hint = status.hint.clone();
            machine.error = status.error.clone();
        }
    }
    machine
}

/// One project as `projects` lists it.
async fn project(cx: &Cx, name: &str) -> Reply<ProjectSync> {
    projects(cx, Some(name))
        .await?
        .into_iter()
        .next()
        .ok_or(ApiError::Internal {})
}

pub(super) async fn set_project_sync(cx: Cx, request: SetProjectSync) -> Reply<ProjectSync> {
    let name = project_name(&request.project)?;
    let machines = machines(&cx).await?;
    let machine = machines
        .get(&request.machine_id)
        .ok_or(ApiError::NotFound {})?;
    let id: EndpointId = request
        .machine_id
        .parse()
        .map_err(|_| ApiError::NotFound {})?;
    if !request.on {
        if cx
            .user
            .remove_project_sync(name, &request.machine_id)
            .await?
        {
            push_sync(&cx.app.nodes, &cx.user, None).await?;
        }
        return project(&cx, name).await;
    }
    let folder = request
        .folder
        .as_deref()
        .map(str::trim)
        .filter(|folder| !folder.is_empty())
        .ok_or_else(|| {
            ApiError::bad_request(format!("Name the project's folder on {}.", machine.name))
        })?;
    if cx
        .user
        .pinned_projects()
        .await?
        .iter()
        .any(|pinned| pinned == name)
    {
        return Err(ApiError::bad_request(format!(
            "{name} is pinned to the machines it is on. Unpin it to sync it."
        )));
    }
    if online(&cx, &request.machine_id).is_none() {
        return Err(ApiError::bad_request(format!(
            "{} is offline, or its beep-node is too old or shares no folders.",
            machine.name
        )));
    }
    let row = SyncRow {
        project: name.to_owned(),
        endpoint: request.machine_id.clone(),
        folder: folder.to_owned(),
    };
    let before = cx
        .user
        .project_syncs()
        .await?
        .into_iter()
        .find(|old| old.project == row.project && old.endpoint == row.endpoint);
    cx.user.set_project_sync(&row).await?;
    let refused = push_sync(&cx.app.nodes, &cx.user, Some(id))
        .await?
        .into_iter()
        .find(|refusal| refusal.node == id && refusal.refused.project == name);
    if let Some(refusal) = refused {
        // Back to the folder it synced before, if any.
        match before {
            Some(before) => cx.user.set_project_sync(&before).await?,
            None => {
                cx.user
                    .remove_project_sync(name, &request.machine_id)
                    .await?;
            }
        }
        push_sync(&cx.app.nodes, &cx.user, None).await?;
        return Err(ApiError::bad_request(format!(
            "{} cannot sync {folder}: {}",
            machine.name, refusal.refused.message
        )));
    }
    project(&cx, name).await
}

pub(super) async fn list_project_sync(cx: Cx, _: ListProjectSync) -> Reply<Vec<ProjectSync>> {
    projects(&cx, None).await
}

pub(super) async fn get_sync_skipped(cx: Cx, request: GetSyncSkipped) -> Reply<SyncSkipped> {
    let name = project_name(&request.project)?;
    let synced = cx
        .user
        .project_syncs()
        .await?
        .iter()
        .any(|row| row.project == name && row.endpoint == request.machine_id);
    if !synced || !machines(&cx).await?.contains_key(&request.machine_id) {
        return Err(ApiError::NotFound {});
    }
    let node = online(&cx, &request.machine_id).ok_or(ApiError::NotFound {})?;
    let skipped = node
        .sync_skipped(name)
        .await
        .map_err(|e| ApiError::bad_request(format!("{e:#}")))?;
    Ok(SyncSkipped {
        paths: skipped.paths.into_iter().filter_map(skipped_file).collect(),
        through_git: skipped.through_git,
        truncated: skipped.truncated,
    })
}

/// A path left out as the app shows it; one left out by a rule this
/// server does not know is not shown.
fn skipped_file(path: SkippedPath) -> Option<SkippedFile> {
    let rule = match path.rule {
        Rule::Git => SkipRule::Git {},
        Rule::GitIgnore => SkipRule::GitIgnore {},
        Rule::Default => SkipRule::Default {
            marker: path.marker.unwrap_or_default(),
        },
        Rule::BeepIgnore => SkipRule::BeepIgnore {},
        Rule::Link => SkipRule::Link {},
        Rule::Other => return None,
    };
    Some(SkippedFile {
        path: path.path,
        folder: path.folder,
        rule,
    })
}

pub(super) async fn pin_project(cx: Cx, request: PinProject) -> Reply<ProjectSync> {
    let name = project_name(&request.project)?;
    cx.user.pin_project(name, request.pinned).await?;
    if request.pinned {
        push_sync(&cx.app.nodes, &cx.user, None).await?;
    }
    project(&cx, name).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row() -> SyncRow {
        SyncRow {
            project: "game".into(),
            endpoint: "node".into(),
            folder: "/srv/game".into(),
        }
    }

    #[test]
    fn a_machine_shows_offline_refused_or_how_it_stands() {
        let offline = machine_sync(&row(), "mac".into(), None);
        assert_eq!(offline.status, SyncStatus::Offline {});
        assert_eq!(offline.machine_name, "mac");

        let refused = machine_sync(&row(), "mac".into(), Some(None));
        assert_eq!(refused.status, SyncStatus::Failed {});
        assert!(refused.error.is_some());

        let status = ProjectStatus {
            name: "game".into(),
            folder: "/srv/game".into(),
            state: State::Idle,
            files: 3,
            bytes: 300,
            conflicts: vec!["plan (conflict 1a2b3c4d).md".into()],
            rescan_secs: Some(45),
            hint: Some("sysctl fs.inotify.max_user_watches=524288".into()),
            ..ProjectStatus::default()
        };
        let synced = machine_sync(&row(), "mac".into(), Some(Some(&status)));
        assert_eq!(synced.status, SyncStatus::InSync {});
        assert_eq!(synced.files, 3);
        assert_eq!(synced.bytes, 300);
        assert_eq!(synced.conflicts, status.conflicts);
        assert_eq!(synced.rescan_secs, Some(45));
        assert_eq!(synced.hint, status.hint);
    }

    #[test]
    fn a_default_rule_keeps_its_marker_and_an_unknown_rule_is_dropped() {
        let path = |rule, marker: Option<&str>| SkippedPath {
            path: "target".into(),
            folder: true,
            rule,
            marker: marker.map(str::to_owned),
        };
        assert_eq!(
            skipped_file(path(Rule::Default, Some("Cargo.toml"))),
            Some(SkippedFile {
                path: "target".into(),
                folder: true,
                rule: SkipRule::Default {
                    marker: "Cargo.toml".into()
                },
            })
        );
        assert_eq!(skipped_file(path(Rule::Other, None)), None);
    }

    #[test]
    fn a_project_name_is_trimmed_and_bounded() {
        assert_eq!(project_name("  game ").unwrap(), "game");
        assert!(project_name(" ").is_err());
        assert!(project_name("a\nb").is_err());
        assert!(project_name(&"x".repeat(MAX_NAME + 1)).is_err());
    }
}
