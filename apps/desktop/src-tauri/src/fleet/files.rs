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
        self, Exec, agent_files, listing,
        sessions::{self, Reading, WorkRead, WorkWrite},
    },
};
use beep_projects::{
    Clip, Shell, ShellOutput,
    branches::{self, read_branches},
    for_machine,
    history::{self, CommitId, HistoryAsk},
    imports::{
        folder::{self, FolderPath},
        found::MachineFound,
    },
    is_windows,
};
use beep_proto::SessionId;
use beep_proto::{
    AGENT_FILE_CHUNK, AgentFileChunk, ApiError, ListWorkTree, MachineFolder, MachineSession,
    MachineUpdate, ProjectBranch, ReadWorkFile, WorkDiff, WorkTree, WriteWorkFile,
    node::{
        agent::DiffAsk,
        events::{GitOutput, GitRead, Read, Value},
        tool_update::Tool,
        update::ReleaseChannel,
    },
};
use iroh::EndpointId;
use std::str::FromStr;
use std::time::Duration;
use tokio::time::timeout;

/// How long one machine gets to look for checkouts again and name them.
const MACHINE_LIMIT: Duration = Duration::from_secs(30);
/// What a project's default branch is taken to be until its checkout says:
/// the scripts read `origin`'s HEAD first and `branches` gives what they found.
pub(crate) const FALLBACK_DEFAULT: &str = "main";
/// The most checkouts one machine names.
const FOLDERS_PER_MACHINE: u32 = 100;

/// A machine as a project's shell, running POSIX scripts through the node: on Windows, in the
/// sh of Git for Windows.
pub(crate) struct PeerShell {
    client: PeerClient,
    windows: bool,
    complete_output: bool,
    projection: Option<(String, GitOutput)>,
}

impl PeerShell {
    /// The shell of `machine`. Whether it is Windows comes from what it last reported, else
    /// from asking it.
    pub(crate) async fn on(fleet: &Fleet, machine: EndpointId) -> Result<Self> {
        let client = fleet.client(machine).await?;
        let windows = match fleet.reach().last_env(machine) {
            Some(env) => is_windows(&env.os),
            None => agent_files::on_windows(&client).await?,
        };
        Ok(Self {
            client,
            windows,
            complete_output: false,
            projection: None,
        })
    }
    /// Framed Git responses validate their byte lengths and bound their own output.
    pub(crate) fn complete_output(mut self) -> Self {
        self.complete_output = true;
        self
    }

    /// Use the current subscribed Git projection when this view has a live files topic.
    pub(crate) async fn snapshot_git(mut self, root: &str, query: GitRead) -> Result<Self> {
        if let Some(value) = self
            .client
            .snapshot(Read::Git {
                root: root.to_owned(),
                query: query.clone(),
            })
            .await
        {
            let Value::Git(output) = value? else {
                return Err(DesktopError::BadRequest(
                    "unexpected Git snapshot result".into(),
                ));
            };
            let script = match query {
                GitRead::State => beep_projects::checkout_sync::local_state_script(root),
                GitRead::Branches { fallback } => branches::branches_script(root, &fallback),
                GitRead::History {
                    revision,
                    offset,
                    search,
                } => {
                    let revision = revision.as_deref().map(CommitId::from_str).transpose()?;
                    history::history_script(&HistoryAsk {
                        folder: root,
                        revision: revision.as_ref(),
                        offset,
                        search: &search,
                    })
                }
                GitRead::Files { commit } => {
                    history::files_script(root, &CommitId::from_str(&commit)?)
                }
            };
            self.projection = Some((script, output));
        }

        Ok(self)
    }
}

#[async_trait]
impl Shell for PeerShell {
    async fn run(&self, script: &str, limit: Duration) -> anyhow::Result<ShellOutput> {
        if let Some((expected, output)) = &self.projection
            && expected == script
        {
            return Ok(ShellOutput {
                exit_code: output.exit_code,
                timed_out: output.timed_out,
                output: output.output.clone(),
            });
        }

        let script = for_machine(script, self.windows);
        let Exec {
            exit_code,
            timed_out,
            output,
        } = machine::exec(&self.client, &script, limit).await?;
        let output = if self.complete_output {
            output
        } else {
            let mut clip = Clip::default();
            clip.push(&output);
            clip.finish()
        };
        Ok(ShellOutput {
            exit_code,
            timed_out,
            output,
        })
    }
}

/// Where a project's checkout is: a machine and the folder on it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Place {
    pub machine: EndpointId,
    pub folder: String,
}

/// The folders `ask.depth` levels down from `ask.path` in a session's
/// working folder; a pane that follows the folder keeps the read live.
pub(crate) async fn tree(
    fleet: &Fleet,
    machine: EndpointId,
    ask: &ListWorkTree,
    reading: Reading,
) -> Result<WorkTree> {
    let client = fleet.client(machine).await?;
    work(fleet, machine, sessions::tree(&client, ask, reading).await?)
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

/// Mutations always run on the owning machine, never on a synced copy.
pub(crate) async fn edit_files(
    fleet: &Fleet,
    machine: EndpointId,
    root: String,
    action: beep_proto::node::agent::FileAction,
) -> Result<()> {
    let client = fleet.client(machine).await?;
    work(
        fleet,
        machine,
        sessions::edit_files(&client, root, action).await?,
    )
}

/// Asks a language server on `machine` for the definition at 0-based `line` and
/// UTF-16 `column` of `path` inside `root`.
pub(crate) async fn lsp_definition(
    fleet: &Fleet,
    machine: EndpointId,
    root: String,
    path: beep_proto::node::agent::RelPath,
    line: u32,
    column: u32,
    text: Option<String>,
) -> Result<beep_proto::node::agent::LspDefinitions> {
    let client = fleet.client(machine).await?;
    work(
        fleet,
        machine,
        sessions::lsp_definition(&client, root, path, line, column, text).await?,
    )
}

/// Whether each of `paths` is on `machine`, in order; `None` from a node from before the
/// question.
pub(crate) async fn present(
    fleet: &Fleet,
    machine: EndpointId,
    paths: Vec<String>,
) -> Result<Option<Vec<bool>>> {
    let client = fleet.client(machine).await?;
    Ok(sessions::present(&client, paths).await?)
}

/// The BLAKE3 hash of each of `paths` inside `root` on `machine`, in order;
/// `None` from a node from before the question.
pub(crate) async fn hashes(
    fleet: &Fleet,
    machine: EndpointId,
    root: &str,
    paths: Vec<String>,
) -> Result<Option<Vec<Option<[u8; 32]>>>> {
    let client = fleet.client(machine).await?;
    Ok(sessions::hashes(&client, root, paths).await?)
}

/// Writes one part of a file into a session's folder; gives the path it went to.
pub(crate) async fn write(
    fleet: &Fleet,
    machine: EndpointId,
    write: WriteWorkFile,
) -> Result<String> {
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

/// Writes an attachment into its session's cache on the owning host.
pub(crate) async fn upload(
    fleet: &Fleet,
    machine: EndpointId,
    agent: String,
    session: SessionId,
    write: WriteWorkFile,
) -> Result<String> {
    let client = fleet.client(machine).await?;
    let part = WorkWrite {
        root: write.root,
        path: write.path,
        offset: write.offset,
        size: write.size,
        data: write.data,
    };
    work(
        fleet,
        machine,
        sessions::upload_file(&client, agent, session, part).await?,
    )
}

/// Removes an upload abandoned before a new chat started.
pub(crate) async fn discard_draft_attachment(
    fleet: &Fleet,
    machine: EndpointId,
    draft: SessionId,
    path: String,
) -> Result<()> {
    let client = fleet.client(machine).await?;
    work(
        fleet,
        machine,
        sessions::discard_draft_attachment(&client, draft, path).await?,
    )
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

/// The checkouts `machine` has, after a quick walk of its search folder so one made since the
/// last walk counts; `Err` says in words why it could not say.
pub(crate) async fn found_on(
    fleet: &Fleet,
    machine: EndpointId,
) -> std::result::Result<MachineFound, String> {
    let ask = async {
        let client = fleet.client(machine).await.map_err(|e| e.to_string())?;
        // A node from before rescans closes the stream; the list it has still answers.
        if let Err(e) = beep_mesh::machine::rescan_search(&client).await {
            log::info!(
                "{} did not look for checkouts again: {e}",
                fleet.name_of(machine)
            );
        }

        let folders = sessions::found_folders(&client, FOLDERS_PER_MACHINE)
            .await
            .map_err(|e| e.to_string())?;
        let host = fleet.host_of(machine);
        Ok(MachineFound {
            machine_id: host.id,
            machine_name: host.name,
            folders,
        })
    };
    timeout(MACHINE_LIMIT, ask)
        .await
        .unwrap_or_else(|_| Err("it did not answer in time".to_owned()))
}

pub(super) fn in_any_folder(cwd: &str, folders: &[String]) -> bool {
    folders.iter().any(|folder| {
        let base = folder.trim_end_matches(['/', '\\']);
        cwd == folder
            || cwd == base
            || cwd
                .strip_prefix(base)
                .is_some_and(|rest| rest.starts_with('/') || rest.starts_with('\\'))
    })
}

/// Strictly checks whether any session is currently open in `folders` on `machine` before
/// removing or deleting a checkout. Refuses with an error if an online machine fails or times out
/// instead of silently treating a slow node as having no active sessions.
pub(crate) async fn has_open_project_session(
    fleet: &Fleet,
    machine: EndpointId,
    folders: &[String],
    rows: &super::session_mirror::Rows,
) -> Result<bool> {
    if folders.is_empty() {
        return Ok(false);
    }

    let machine_id = machine.to_string();
    let open = |s: &MachineSession| {
        s.machine_id == machine_id
            && !matches!(s.status, beep_proto::AgentStatus::Ended {})
            && !fleet.node_ops().is_deleting_session_on(machine, &s.id)
            && in_any_folder(&s.cwd, folders)
    };

    let client = match timeout(Duration::from_secs(10), fleet.client(machine)).await {
        Ok(Ok(client)) => client,
        Ok(Err(DesktopError::Offline(_))) => {
            return Ok(rows.iter().any(open));
        }
        Ok(Err(err)) => return Err(err),
        Err(_) => {
            return Err(DesktopError::TimedOut(format!(
                "Could not verify whether an agent is working on {}; try again once it answers.",
                fleet.name_of(machine)
            )));
        }
    };

    let host = fleet.host_of(machine);
    let (listed, mut outside) = timeout(Duration::from_secs(10), async {
        tokio::join!(
            listing::listed_sessions(&client, &host, false),
            listing::outside_sessions(&client, &host),
        )
    })
    .await
    .map_err(|_| {
        DesktopError::TimedOut(format!(
            "Could not verify whether an agent is working on {}; try again once it answers.",
            fleet.name_of(machine)
        ))
    })?;
    let Some(linked) = listed else {
        return Err(DesktopError::TimedOut(format!(
            "Could not verify whether an agent is working on {}; try again once it answers.",
            fleet.name_of(machine)
        )));
    };

    let live_beep_open = linked.into_iter().any(|item| {
        !matches!(item.session.status, beep_proto::AgentStatus::Ended {})
            && !fleet
                .node_ops()
                .is_deleting_session_on(machine, &item.session.id)
            && in_any_folder(&item.session.cwd, folders)
    });
    if live_beep_open {
        return Ok(true);
    }

    rows.with_presence(machine, &mut outside);
    Ok(outside.iter().any(open) || rows.iter().filter(|s| s.external).any(open))
}

/// A folder of a machine for the folder picker, `path` else its home, then the folders two
/// levels below it, read in one run so opening them is instant.
pub(crate) async fn folder(
    fleet: &Fleet,
    machine: EndpointId,
    path: Option<&str>,
    depth: u8,
) -> Result<Vec<MachineFolder>> {
    let start = match path.map(FolderPath::parse).transpose() {
        Ok(Some(Some(picked))) => picked,
        Ok(_) => FolderPath::Home(String::new()),
        Err(e) => return Err(DesktopError::BadRequest(e.to_string())),
    };
    let shell = PeerShell::on(fleet, machine).await?;
    folder::tree(&shell, &fleet.name_of(machine), &start, depth)
        .await
        .map_err(|e| DesktopError::BadRequest(e.to_string()))
}

/// The default branch and the branches of the checkout at `place`.
pub(crate) async fn branches(
    fleet: &Fleet,
    place: &Place,
    default_branch: &str,
) -> Result<(String, Vec<ProjectBranch>)> {
    let shell = PeerShell::on(fleet, place.machine)
        .await?
        .snapshot_git(
            &place.folder,
            GitRead::Branches {
                fallback: default_branch.to_owned(),
            },
        )
        .await?;
    read_branches(&shell, &place.folder, default_branch)
        .await
        .map_err(|e| DesktopError::BadRequest(format!("{}: {e:#}", fleet.name_of(place.machine))))
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
    channel: ReleaseChannel,
) -> Result<MachineUpdate> {
    let client = fleet.client(machine).await?;
    let updated = machine::update_node(&client, now, channel)
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
