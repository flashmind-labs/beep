//! Project folder sync records of released apps. The server reaches no machine, so each shows
//! offline and edits stay in the records; desktops sync projects over their mesh.

use super::{
    Reply,
    own_nodes::{linked_keys, retired},
};
use crate::{
    api::router::Cx,
    protocol::{
        node::sync::{Progress, ProjectStatus, State},
        *,
    },
    store::NodeRow,
};
use beep_projects::imports::records::{ImportRow, SyncRow};
use std::collections::{BTreeMap, HashMap, HashSet};

/// The longest project name.
pub(crate) const MAX_NAME: usize = 200;

/// `name` trimmed, once it is a name a project may have.
pub(crate) fn project_name(name: &str) -> Result<&str, ApiError> {
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

/// The caller's synced or pinned projects, or only `only`.
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
    let paused: HashSet<String> = cx.user.paused_syncs().await?.into_iter().collect();
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
        let machine = machine_sync(&row, name, None, paused.contains(&row.project));
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

/// `status` is `None` when the machine did not answer, `Some(None)` when it answered without the
/// project. A paused machine shows `InSync`.
fn machine_sync(
    row: &SyncRow,
    name: String,
    status: Option<Option<&ProjectStatus>>,
    paused: bool,
) -> MachineSync {
    let mut machine = MachineSync {
        git_only: false,
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
        Some(None) if paused => machine.status = SyncStatus::InSync {},
        Some(None) => {
            machine.status = SyncStatus::Failed {};
            machine.error = Some("The machine is not syncing this folder.".to_owned());
        }
        Some(Some(status)) => {
            machine.git_only = status.git_only;
            machine.status = match status.state {
                State::Scanning => SyncStatus::Scanning {},
                State::Syncing | State::Other => SyncStatus::Syncing {},
                State::Idle | State::Paused => SyncStatus::InSync {},
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

/// How a machine stands in detail: `machine_sync`'s answer plus pause and progress.
fn machine_detail(
    row: &SyncRow,
    name: String,
    status: Option<Option<&ProjectStatus>>,
    paused: bool,
) -> MachineSyncDetail {
    let reported = status.flatten();
    MachineSyncDetail {
        sync: machine_sync(row, name, status, paused),
        paused: reported.is_some_and(|s| s.paused) || (paused && status.is_some()),
        progress: reported.and_then(|s| s.progress).map(sync_progress),
        waiting: reported.map(|s| s.waiting.clone()).unwrap_or_default(),
        can_resolve: false,
    }
}

fn sync_progress(p: Progress) -> SyncProgress {
    SyncProgress {
        files_done: p.files_done,
        files_total: p.files_total,
        bytes_done: p.bytes_done,
        bytes_total: p.bytes_total,
        rate: p.rate,
        eta_secs: p.eta_secs,
    }
}

/// `project` in detail, as `GetProjectSyncDetail` gives it.
async fn detail(cx: &Cx, project: &str) -> Reply<ProjectSyncDetail> {
    let machines = machines(cx).await?;
    let rows: Vec<SyncRow> = cx
        .user
        .project_syncs()
        .await?
        .into_iter()
        .filter(|row| row.project == project && machines.contains_key(&row.endpoint))
        .collect();
    let paused = cx.user.paused_syncs().await?.iter().any(|p| p == project);
    let pinned = cx
        .user
        .pinned_projects()
        .await?
        .iter()
        .any(|p| p == project);
    let machines = rows
        .iter()
        .map(|row| {
            let name = machines
                .get(&row.endpoint)
                .map(|node| node.name.clone())
                .unwrap_or_default();
            machine_detail(row, name, None, paused)
        })
        .collect();
    Ok(ProjectSyncDetail {
        project: project.to_owned(),
        pinned,
        paused,
        machines,
    })
}

pub(super) async fn get_project_sync_detail(
    cx: Cx,
    request: GetProjectSyncDetail,
) -> Reply<ProjectSyncDetail> {
    let name = project_name(&request.project)?;
    detail(&cx, name).await
}

pub(super) async fn pause_project_sync(
    cx: Cx,
    request: PauseProjectSync,
) -> Reply<ProjectSyncDetail> {
    let name = project_name(&request.project)?;
    if request.paused
        && cx
            .user
            .pinned_projects()
            .await?
            .iter()
            .any(|pinned| pinned == name)
    {
        return Err(ApiError::bad_request(format!(
            "{name} is pinned to the machines it is on, so it does not sync."
        )));
    }
    cx.user.pause_sync(name, request.paused).await?;
    detail(&cx, name).await
}

pub(super) async fn resolve_sync_conflict(_: Cx, _: ResolveSyncConflict) -> Reply<()> {
    retired()
}
/// One project as `projects` lists it.
async fn project(cx: &Cx, name: &str) -> Reply<ProjectSync> {
    projects(cx, Some(name))
        .await?
        .into_iter()
        .next()
        .ok_or(ApiError::Internal {})
}

/// Turning a machine's sync off drops its record; turning it on needs the machine, which the
/// server does not reach.
pub(super) async fn set_project_sync(cx: Cx, request: SetProjectSync) -> Reply<ProjectSync> {
    let name = project_name(&request.project)?;
    if !machines(&cx).await?.contains_key(&request.machine_id) {
        return Err(ApiError::NotFound {});
    }
    if request.on {
        return retired();
    }
    cx.user
        .remove_project_sync(name, &request.machine_id)
        .await?;
    project(&cx, name).await
}
pub(super) async fn list_project_sync(cx: Cx, _: ListProjectSync) -> Reply<Vec<ProjectSync>> {
    projects(&cx, None).await
}

pub(super) async fn list_project_syncthing(
    cx: Cx,
    request: ListProjectSyncthing,
) -> Reply<Vec<SyncthingPlace>> {
    let name = project_name(&request.project)?;
    let machines = machines(&cx).await?;
    let imports = cx.user.imports().await?;
    let syncs = cx.user.project_syncs().await?;
    Ok(syncthing_places(name, &imports, &syncs, &machines))
}

/// The folders of `project` that their machine reported as kept in step by Syncthing.
fn syncthing_places(
    project: &str,
    imports: &[ImportRow],
    syncs: &[SyncRow],
    machines: &HashMap<String, NodeRow>,
) -> Vec<SyncthingPlace> {
    let imported = imports
        .iter()
        .filter(|row| row.project.eq_ignore_ascii_case(project))
        .map(|row| (&row.endpoint, &row.folder));
    let synced = syncs
        .iter()
        .filter(|row| row.project == project)
        .map(|row| (&row.endpoint, &row.folder));
    let mut out: Vec<SyncthingPlace> = Vec::new();
    for (endpoint, folder) in imported.chain(synced) {
        let Some(env) = machines.get(endpoint).and_then(|node| node.env.as_ref()) else {
            continue;
        };
        let Some(synced) = env.syncthing_folder(folder) else {
            continue;
        };
        if out
            .iter()
            .any(|place| &place.machine_id == endpoint && &place.folder == folder)
        {
            continue;
        }
        out.push(SyncthingPlace {
            machine_id: endpoint.clone(),
            folder: folder.clone(),
            label: synced.name().to_owned(),
        });
    }
    out
}

pub(super) async fn get_sync_skipped(_: Cx, _: GetSyncSkipped) -> Reply<SyncSkipped> {
    retired()
}

pub(super) async fn pin_project(cx: Cx, request: PinProject) -> Reply<ProjectSync> {
    let name = project_name(&request.project)?;
    cx.user.pin_project(name, request.pinned).await?;
    project(&cx, name).await
}
pub(super) async fn pin_project_to_top(cx: Cx, request: PinProjectToTop) -> Reply<()> {
    let name = project_name(&request.project)?;
    cx.user.pin_project_to_top(name, request.pinned).await?;
    Ok(())
}

pub(super) async fn list_projects_on_top(cx: Cx, _: ListProjectsOnTop) -> Reply<Vec<String>> {
    Ok(cx.user.projects_on_top().await?)
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
    fn a_paused_machine_is_quiet_and_its_progress_carries_over() {
        // An older node stopped the paused project: not a refusal.
        let stopped = machine_detail(&row(), "mac".into(), Some(None), true);
        assert_eq!(stopped.sync.status, SyncStatus::InSync {});
        assert_eq!(stopped.sync.error, None);
        assert!(stopped.paused);
        let offline = machine_detail(&row(), "mac".into(), None, true);
        assert_eq!(offline.sync.status, SyncStatus::Offline {});
        assert!(!offline.paused);

        let paused = ProjectStatus {
            name: "game".into(),
            state: State::Paused,
            paused: true,
            ..ProjectStatus::default()
        };
        let detail = machine_detail(&row(), "mac".into(), Some(Some(&paused)), true);
        assert_eq!(detail.sync.status, SyncStatus::InSync {});
        assert!(detail.paused);

        let moving = ProjectStatus {
            name: "game".into(),
            state: State::Syncing,
            progress: Some(Progress {
                files_done: 142,
                files_total: 610,
                bytes_done: 48_000_000,
                bytes_total: 210_000_000,
                rate: 2_100_000,
                eta_secs: Some(78),
            }),
            waiting: vec!["src/main.rs".into()],
            ..ProjectStatus::default()
        };
        let detail = machine_detail(&row(), "mac".into(), Some(Some(&moving)), false);
        assert_eq!(detail.sync.status, SyncStatus::Syncing {});
        assert!(!detail.paused);
        let progress = detail.progress.expect("progress");
        assert_eq!((progress.files_done, progress.files_total), (142, 610));
        assert_eq!(progress.rate, 2_100_000);
        assert_eq!(progress.eta_secs, Some(78));
        assert_eq!(detail.waiting, ["src/main.rs"]);
    }
}
