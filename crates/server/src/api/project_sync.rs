//! Keeping a project's folder in step across the caller's machines: turning
//! it on or off per machine, how each machine stands, what sync leaves out,
//! pausing it, settling its conflicts, and pinning a project to the
//! machines it is on or to the top of the sidebar. The nodes sync node to node; these requests only change and
//! read their lists.

use super::{
    Reply,
    own_nodes::{self, linked_keys},
};
use crate::{
    nodes::{Online, push_sync},
    protocol::{
        node::sync::{Keep, Progress, ProjectStatus, Rule, SkippedPath, State},
        *,
    },
    router::Cx,
    store::{ImportRow, NodeRow, SyncRow},
};
use futures::{StreamExt, stream};
use iroh::EndpointId;
use std::collections::{BTreeMap, HashMap, HashSet};

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
    own_nodes::online(cx, machine_id).filter(Online::syncs)
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
        let status = statuses
            .get(&row.endpoint)
            .map(|projects| projects.iter().find(|status| status.name == row.project));
        let machine = machine_sync(&row, name, status, paused.contains(&row.project));
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
/// `Some(None)` when it answered without the project: it refused it, or,
/// when the project is `paused`, it is too old to pause and stopped it.
/// A paused machine shows `InSync`: nothing moves, and nothing failed.
fn machine_sync(
    row: &SyncRow,
    name: String,
    status: Option<Option<&ProjectStatus>>,
    paused: bool,
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
        Some(None) if paused => machine.status = SyncStatus::InSync {},
        Some(None) => {
            machine.status = SyncStatus::Failed {};
            machine.error = Some("The machine is not syncing this folder.".to_owned());
        }
        Some(Some(status)) => {
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

/// How a machine stands in detail: `machine_sync`'s answer, and whether it
/// is paused, how far it is and what still waits.
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
    let statuses = statuses(cx, rows.iter().map(|row| row.endpoint.clone()).collect()).await;
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
            let status = statuses
                .get(&row.endpoint)
                .map(|projects| projects.iter().find(|status| status.name == row.project));
            let mut machine = machine_detail(row, name, status, paused);
            machine.can_resolve =
                online(cx, &row.endpoint).is_some_and(|node| node.sync_controls());
            machine
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
    if cx.user.pause_sync(name, request.paused).await? {
        push_sync(&cx.app.nodes, &cx.user, None).await?;
    }
    detail(&cx, name).await
}

pub(super) async fn resolve_sync_conflict(cx: Cx, request: ResolveSyncConflict) -> Reply<()> {
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
    if !node.sync_controls() {
        return Err(ApiError::bad_request(format!(
            "{}'s beep-node is too old to settle conflicts. Update it, or settle it in the folder.",
            node.name
        )));
    }
    let keep = match request.keep {
        ConflictKeep::Original {} => Keep::Original,
        ConflictKeep::Copy {} => Keep::Copy,
    };
    node.sync_resolve(name, &request.path, keep)
        .await
        .map_err(|e| ApiError::bad_request(format!("{e:#}")))
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

/// The folders of `project`, imported or synced, that the machine they are
/// on reported as kept in step by Syncthing.
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
    fn a_machine_shows_offline_refused_or_how_it_stands() {
        let offline = machine_sync(&row(), "mac".into(), None, false);
        assert_eq!(offline.status, SyncStatus::Offline {});
        assert_eq!(offline.machine_name, "mac");

        let refused = machine_sync(&row(), "mac".into(), Some(None), false);
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
        let synced = machine_sync(&row(), "mac".into(), Some(Some(&status)), false);
        assert_eq!(synced.status, SyncStatus::InSync {});
        assert_eq!(synced.files, 3);
        assert_eq!(synced.bytes, 300);
        assert_eq!(synced.conflicts, status.conflicts);
        assert_eq!(synced.rescan_secs, Some(45));
        assert_eq!(synced.hint, status.hint);
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
    fn syncthing_places_are_the_project_folders_a_machine_reported() {
        use crate::protocol::node::{EnvReport, SyncthingFolder};
        let node = |endpoint: &str, folders: Vec<SyncthingFolder>| NodeRow {
            endpoint: endpoint.into(),
            name: endpoint.into(),
            version: "0.1.0".into(),
            env: Some(EnvReport {
                syncthing: folders,
                ..EnvReport::default()
            }),
            seen_at: chrono::Utc::now(),
        };
        let machines: HashMap<String, NodeRow> = [
            node(
                "laptop",
                vec![SyncthingFolder {
                    id: "work-1".into(),
                    label: "Work".into(),
                    path: "/srv/work".into(),
                }],
            ),
            node("build", Vec::new()),
        ]
        .into_iter()
        .map(|node| (node.endpoint.clone(), node))
        .collect();
        let import = |endpoint: &str, folder: &str| ImportRow {
            project: "acme/game".into(),
            endpoint: endpoint.into(),
            folder: folder.into(),
            url: "https://example.com/acme/game".into(),
            imported_at: chrono::Utc::now(),
            fork: None,
        };
        let synced = SyncRow {
            project: "acme/game".into(),
            endpoint: "laptop".into(),
            folder: "/srv/work/game".into(),
        };
        let places = syncthing_places(
            "acme/game",
            &[
                import("laptop", "/srv/work/game"),
                import("build", "/srv/work/game"),
            ],
            &[synced],
            &machines,
        );
        assert_eq!(
            places,
            [SyncthingPlace {
                machine_id: "laptop".into(),
                folder: "/srv/work/game".into(),
                label: "Work".into(),
            }]
        );
        assert!(syncthing_places("acme/other", &[], &[], &machines).is_empty());
    }

    #[test]
    fn a_project_name_is_trimmed_and_bounded() {
        assert_eq!(project_name("  game ").unwrap(), "game");
        assert!(project_name(" ").is_err());
        assert!(project_name("a\nb").is_err());
        assert!(project_name(&"x".repeat(MAX_NAME + 1)).is_err());
    }
}
