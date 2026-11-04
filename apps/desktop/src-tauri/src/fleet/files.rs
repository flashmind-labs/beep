//! A session's folder, diffs, checkouts and projects' branches on the
//! machines of the mesh, and the few things asked of a machine itself:
//! its log, its update, where it looks for checkouts, its tools.

use super::{
    Fleet,
    agents::{chunk, work},
};
use crate::error::{DesktopError, Result};
use async_trait::async_trait;
use beep_mesh::{
    PeerClient,
    machine::{
        self, Exec, listing,
        sessions::{self, WorkRead, WorkWrite},
    },
};
use beep_projects::{
    Clip, Shell, ShellOutput,
    branches::{self, read_branches},
    imports::{
        folder::{FolderPath, look},
        found::MachineFound,
    },
};
use beep_proto::{
    AGENT_FILE_CHUNK, AgentFileChunk, ApiError, ListWorkTree, MAX_WORK_UPLOAD, MachineFolder,
    MachineSession, MachineUpdate, ProjectBranch, ReadWorkFile, WorkDiff, WorkTree, WriteWorkFile,
    node::{agent::DiffAsk, tool_update::Tool},
};
use futures::{StreamExt, stream};
use iroh::EndpointId;
use std::{cmp::Reverse, collections::HashSet, time::Duration};

/// Machines asked at once.
const IN_FLIGHT: usize = 4;
/// The most checkouts one machine names.
const FOLDERS_PER_MACHINE: u32 = 100;

/// A machine as a project's shell, running scripts through the node.
pub(crate) struct PeerShell(pub PeerClient);

#[async_trait]
impl Shell for PeerShell {
    async fn run(&self, script: &str, limit: Duration) -> anyhow::Result<ShellOutput> {
        let Exec {
            exit_code,
            timed_out,
            output,
        } = machine::exec(&self.0, script, limit).await?;
        let mut clip = Clip::default();
        clip.push(&output);
        Ok(ShellOutput {
            exit_code,
            timed_out,
            output: clip.finish(),
        })
    }
}

/// Where a project's checkout is: a machine and the folder on it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Place {
    pub machine: EndpointId,
    pub folder: String,
}

/// The folders `ask.depth` levels down from `ask.path` in a session's working folder.
pub(crate) async fn tree(
    fleet: &Fleet,
    machine: EndpointId,
    ask: &ListWorkTree,
) -> Result<WorkTree> {
    let client = fleet.client(machine).await?;
    work(fleet, machine, sessions::tree(&client, ask).await?)
}

/// One `ReadWorkFile`.
pub(crate) async fn read(
    fleet: &Fleet,
    machine: EndpointId,
    read: &ReadWorkFile,
) -> Result<AgentFileChunk> {
    let client = fleet.client(machine).await?;
    let len = read
        .len
        .min(u32::try_from(AGENT_FILE_CHUNK).unwrap_or(u32::MAX));
    let part = WorkRead {
        root: &read.root,
        path: &read.path,
        offset: read.offset,
        len,
    };
    chunk(sessions::read_file(&client, part).await?, fleet, machine)
}

/// Writes one part of a file into a session's folder; gives the path it went to.
pub(crate) async fn write(
    fleet: &Fleet,
    machine: EndpointId,
    write: WriteWorkFile,
) -> Result<String> {
    if write.size > MAX_WORK_UPLOAD {
        return Err(ApiError::bad_request(format!(
            "Files up to {} MB can be dropped on a session.",
            MAX_WORK_UPLOAD / (1024 * 1024)
        ))
        .into());
    }
    if write.data.len() > AGENT_FILE_CHUNK {
        return Err(ApiError::bad_request("A part of the file is too large.").into());
    }
    let client = fleet.client(machine).await?;
    let part = WorkWrite {
        root: write.root,
        path: write.path,
        offset: write.offset,
        size: write.size,
        data: write.data,
    };
    work(fleet, machine, sessions::write_file(&client, part).await?)
}

/// What changed in the checkout `ask.root`.
pub(crate) async fn work_diff(
    fleet: &Fleet,
    machine: EndpointId,
    ask: DiffAsk,
) -> Result<WorkDiff> {
    let client = fleet.client(machine).await?;
    work(fleet, machine, sessions::diff(&client, ask).await?)
}

/// What changed on `branch` of a project against its default branch, read
/// at the checkout `place`.
pub(crate) async fn branch_diff(
    fleet: &Fleet,
    place: &Place,
    default_branch: &str,
    branch: &str,
    only: Vec<String>,
    lockfiles: bool,
) -> Result<WorkDiff> {
    let branch = beep_projects::model::branch_name(branch)
        .ok_or_else(|| ApiError::bad_request(format!("{} is not a branch name.", branch.trim())))?;
    let ask = DiffAsk {
        root: place.folder.clone(),
        base: default_branch.to_owned(),
        head: branch.to_owned(),
        only,
        lockfiles,
    };
    work_diff(fleet, place.machine, ask).await
}

/// The checkouts coding agents worked in, per online machine; a machine
/// that cannot say is left out.
pub(crate) async fn found_folders(fleet: &Fleet) -> Vec<MachineFound> {
    let asks: Vec<_> = fleet
        .online()
        .into_iter()
        .map(|machine| async move {
            let client = fleet.client(machine).await.ok()?;
            let folders = sessions::found_folders(&client, FOLDERS_PER_MACHINE)
                .await
                .ok()?;
            let host = fleet.host_of(machine);
            Some(MachineFound {
                machine_id: host.id,
                machine_name: host.name,
                folders,
            })
        })
        .collect();
    stream::iter(asks)
        .buffer_unordered(IN_FLIGHT)
        .filter_map(|found| async move { found })
        .collect()
        .await
}

/// The sessions coding agents ran in a project's checkouts, newest first.
pub(crate) async fn project_sessions(fleet: &Fleet, places: &[Place]) -> Vec<MachineSession> {
    let machines: HashSet<EndpointId> = places.iter().map(|place| place.machine).collect();
    let asks: Vec<_> = machines
        .into_iter()
        .map(|machine| async move {
            let folders: Vec<String> = places
                .iter()
                .filter(|place| place.machine == machine)
                .map(|place| place.folder.clone())
                .collect();
            match fleet.client(machine).await {
                Ok(client) => {
                    listing::project_sessions(&client, &fleet.host_of(machine), folders).await
                }
                Err(_) => Vec::new(),
            }
        })
        .collect();
    let mut sessions: Vec<MachineSession> = stream::iter(asks)
        .buffer_unordered(IN_FLIGHT)
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .flatten()
        .collect();
    sessions.sort_by_key(|session| Reverse(session.last_activity));
    sessions.truncate(listing::MAX_PROJECT_SESSIONS);
    sessions
}

/// One folder of a machine for the folder browser: `path`, else its home.
pub(crate) async fn folder(
    fleet: &Fleet,
    machine: EndpointId,
    path: Option<&str>,
) -> Result<MachineFolder> {
    let start = match path.map(FolderPath::parse).transpose() {
        Ok(Some(Some(picked))) => picked,
        Ok(_) => FolderPath::Home(String::new()),
        Err(e) => return Err(DesktopError::BadRequest(e.to_string())),
    };
    let shell = PeerShell(fleet.client(machine).await?);
    look(&shell, &fleet.name_of(machine), &start)
        .await
        .map_err(|e| DesktopError::BadRequest(e.to_string()))
}

/// The default branch and the branches of the checkout at `place`.
pub(crate) async fn branches(
    fleet: &Fleet,
    place: &Place,
    default_branch: &str,
) -> Result<(String, Vec<ProjectBranch>)> {
    let shell = PeerShell(fleet.client(place.machine).await?);
    read_branches(&shell, &place.folder, default_branch)
        .await
        .map_err(|e| DesktopError::BadRequest(format!("{}: {e:#}", fleet.name_of(place.machine))))
}

/// Gives `branch` a worktree in the checkout at `place`.
pub(crate) async fn add_worktree(
    fleet: &Fleet,
    place: &Place,
    branch: &str,
    default_branch: &str,
) -> Result<ProjectBranch> {
    let branch = beep_projects::model::branch_name(branch)
        .ok_or_else(|| ApiError::bad_request("That is not a branch name."))?;
    let name = fleet.name_of(place.machine);
    let shell = PeerShell(fleet.client(place.machine).await?);
    branches::add_worktree(&shell, &name, &place.folder, branch, default_branch)
        .await
        .map_err(|why| DesktopError::BadRequest(format!("{name}: {why}")))
}

/// The last `lines` lines of a machine's log.
pub(crate) async fn machine_log(fleet: &Fleet, machine: EndpointId, lines: u32) -> Result<String> {
    let client = fleet.client(machine).await?;
    Ok(machine::log(&client, lines).await?)
}

/// Has a machine update its beep-node, once its agent sessions are idle or,
/// with `now`, at once.
pub(crate) async fn update_machine(
    fleet: &Fleet,
    machine: EndpointId,
    now: bool,
) -> Result<MachineUpdate> {
    let client = fleet.client(machine).await?;
    let updated = machine::update_node(&client, now)
        .await
        .map_err(|e| DesktopError::BadRequest(format!("{e:#}")))?;
    Ok(MachineUpdate {
        from: updated.from,
        to: updated.to,
        restarting: updated.restarting,
        note: updated.note,
    })
}

/// Where a machine looks for git checkouts, as written.
pub(crate) async fn machine_search(fleet: &Fleet, machine: EndpointId) -> Result<String> {
    let client = fleet.client(machine).await?;
    Ok(machine::search(&client).await?)
}

/// Has a machine look for checkouts in `folder` (empty for home) and walk it again.
pub(crate) async fn set_machine_search(
    fleet: &Fleet,
    machine: EndpointId,
    folder: &str,
) -> Result<String> {
    let client = fleet.client(machine).await?;
    Ok(machine::set_search(&client, folder.trim()).await?)
}

/// Installs release `latest` of `tool` on a machine.
pub(crate) async fn update_tool(
    fleet: &Fleet,
    machine: EndpointId,
    tool: Tool,
    latest: &str,
) -> Result<()> {
    let client = fleet.client(machine).await?;
    machine::install_tool(&client, tool, latest)
        .await
        .map_err(|e| DesktopError::BadRequest(format!("Could not update {tool}: {e:#}")))
}
