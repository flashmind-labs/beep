//! One-request protocols of a node: files, logs, search, background
//! tasks, tool and node updates, and a command in a shell.

use super::{said, unexpected};
use crate::{MeshError, PeerClient};
use beep_proto::node::{
    files, logs,
    process::{self, TaskId},
    search,
    shell::{self, Event},
    tool_update::{self, Tool},
    update,
};
use std::time::Duration;
use tokio::time::timeout;

/// How long a node has to download, check and install a tool.
const TOOL_INSTALL: Duration = Duration::from_secs(10 * 60);
/// How long a node has to download, check and install itself.
const NODE_INSTALL: Duration = Duration::from_secs(5 * 60);
/// How long past its own timeout a command's answer may take.
const GRACE: Duration = Duration::from_secs(15);

/// Sends `request` to the node's shared folders and returns its reply.
pub async fn files(
    client: &PeerClient,
    request: &files::Request,
) -> Result<files::Reply, MeshError> {
    client.call(files::PROTOCOL, request).await
}

/// The last `lines` lines of the node's log.
pub async fn log(client: &PeerClient, lines: u32) -> Result<String, MeshError> {
    match client
        .call(logs::PROTOCOL, &logs::Request { lines })
        .await?
    {
        logs::Reply::Log { text } => Ok(text),
        logs::Reply::Failed { reason } => Err(said(reason)),
        other => Err(unexpected(&other)),
    }
}

/// The folder the node looks for checkouts in, as written.
pub async fn search(client: &PeerClient) -> Result<String, MeshError> {
    ask_search(client, search::Request::Get {}).await
}

/// Has the node look in `folder` (empty for home) and walk it again.
pub async fn set_search(client: &PeerClient, folder: &str) -> Result<String, MeshError> {
    let folder = folder.to_owned();
    ask_search(client, search::Request::Set { folder }).await
}

async fn ask_search(client: &PeerClient, request: search::Request) -> Result<String, MeshError> {
    match client.call(search::PROTOCOL, &request).await? {
        search::Reply::Folder { folder } => Ok(folder),
        search::Reply::Failed { reason } => Err(said(reason)),
        other => Err(unexpected(&other)),
    }
}

/// Background tasks of a node (`process/1`).
pub struct Tasks<'a>(pub &'a PeerClient);

impl Tasks<'_> {
    /// Sends `request` and returns the node's reply.
    pub async fn ask(&self, request: &process::Request) -> Result<process::Reply, MeshError> {
        task_request(self.0, request).await
    }

    /// Starts `command`, giving its task.
    pub async fn start(&self, command: &str, cwd: Option<String>) -> Result<TaskId, MeshError> {
        let request = process::Request::Start {
            command: command.to_owned(),
            cwd,
        };
        match self.ask(&request).await? {
            process::Reply::Started { id } => Ok(id),
            process::Reply::Error { message } => Err(said(message)),
            other => Err(unexpected(&other)),
        }
    }

    /// Stops task `id` if it still runs `command`; one reusing its number is left alone.
    pub async fn stop(&self, id: TaskId, command: &str) -> Result<(), MeshError> {
        match self.ask(&process::Request::Status { id }).await? {
            process::Reply::Status { task, .. }
                if task.command == command && task.state.is_running() =>
            {
                match self.ask(&process::Request::Stop { id }).await? {
                    process::Reply::Error { message } => Err(said(message)),
                    _ => Ok(()),
                }
            }
            _ => Ok(()),
        }
    }
}

/// Sends one `process/1` request.
pub async fn task_request(
    client: &PeerClient,
    request: &process::Request,
) -> Result<process::Reply, MeshError> {
    client.call(process::PROTOCOL, request).await
}

/// Has the node install release `version` of `tool`.
pub async fn install_tool(client: &PeerClient, tool: Tool, version: &str) -> Result<(), MeshError> {
    let request = tool_update::Request::Install {
        tool,
        version: version.to_owned(),
    };
    match client
        .call_within(TOOL_INSTALL, tool_update::PROTOCOL, &request)
        .await?
    {
        tool_update::Reply::Installed {} => Ok(()),
        tool_update::Reply::Failed { reason } => Err(said(reason)),
        other => Err(unexpected(&other)),
    }
}

/// Has the node install the newest release and restart, once its agent
/// sessions are idle or, with `now`, at once.
pub async fn update_node(client: &PeerClient, now: bool) -> Result<update::Event, MeshError> {
    let event = client
        .call_within(
            NODE_INSTALL,
            update::PROTOCOL,
            &update::Request::Update { now },
        )
        .await?;
    match event {
        update::Event::Failed { reason } => Err(said(reason)),
        event => Ok(event),
    }
}

/// How a command run by `exec` ended, with what it printed.
#[derive(Debug, PartialEq)]
pub struct Exec {
    /// `None` when it did not finish: it timed out or ended its shell.
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub output: String,
}

/// Runs `command` in its own shell in the node's home folder.
pub async fn exec(client: &PeerClient, command: &str, limit: Duration) -> Result<Exec, MeshError> {
    let (mut tx, mut rx) = client.open(shell::PROTOCOL).await?;
    let timeout_secs = u32::try_from(limit.as_secs()).unwrap_or(u32::MAX);
    let request = shell::Request::Run {
        command: command.to_owned(),
        timeout_secs,
    };
    tx.send(&request).await.map_err(MeshError::Link)?;
    let answer = async {
        let mut output = String::new();
        loop {
            match rx.recv::<Event>().await.map_err(MeshError::Link)? {
                Some(Event::Output { text }) => output.push_str(&text),
                Some(Event::Done {
                    exit_code,
                    timed_out,
                    ..
                }) => {
                    return Ok(Exec {
                        exit_code,
                        timed_out,
                        output,
                    });
                }
                Some(_) => {}
                None => return Err(said("the node closed the shell")),
            }
        }
    };
    timeout(limit + GRACE, answer)
        .await
        .map_err(|_| said("the node stopped answering"))?
}
