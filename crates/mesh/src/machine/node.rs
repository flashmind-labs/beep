//! One-request protocols of a node: files, logs, search, background
//! tasks, tool and node updates, a command in a shell, and its WSL
//! machines.

use super::{said, stream_closed, timed_out, unexpected};
use crate::{MeshError, PeerClient};
use beep_proto::SecretText;
use beep_proto::node::{
    files, logs,
    process::{self, TaskId},
    search,
    setup::{self, Need},
    shell::{self, Event},
    sign_in,
    tool_update::{self, Tool},
    update, wsl,
};
use serde::{Serialize, de::DeserializeOwned};
use std::time::Duration;
use tokio::time::timeout;

/// How long a node has to download, check and install a tool.
const TOOL_INSTALL: Duration = Duration::from_secs(10 * 60);
/// How long a node has to download, check and install itself.
const NODE_INSTALL: Duration = Duration::from_secs(5 * 60);
/// How long past its own timeout a command's answer may take.
const GRACE: Duration = Duration::from_secs(15);
/// How long the node has to read its log.
const LOG_LIMIT: Duration = Duration::from_secs(30);
/// How long the node has to answer about where it searches.
const SEARCH_LIMIT: Duration = Duration::from_secs(10);
/// How long a quick walk for checkouts may take: the node bounds it at 8 seconds.
const RESCAN_LIMIT: Duration = Duration::from_secs(20);
/// How long a request to the node's tasks or terminals may take; a stop
/// waits for the process group to end.
pub(super) const TASK_LIMIT: Duration = Duration::from_secs(20);
/// How long a sign-in request may take: a start waits up to 30 seconds for
/// the agent's link.
const SIGN_IN_LIMIT: Duration = Duration::from_secs(45);
/// How long a request to the node's shared folders may take.
const FILES_LIMIT: Duration = Duration::from_secs(30);
/// How long one step of adding a WSL machine may take: installing
/// beep-node in it downloads a release.
const WSL_STEP: Duration = Duration::from_secs(10 * 60);

/// How a failed exchange reads: what the node was doing when it went quiet
/// or closed the stream.
#[derive(Clone, Copy)]
pub(super) struct Words {
    pub(super) silent: &'static str,
    pub(super) closed: &'static str,
}

/// Sends one request on a new stream and reads one reply within `limit`.
pub(super) async fn exchange<Q: Serialize, R: DeserializeOwned>(
    client: &PeerClient,
    limit: Duration,
    protocol: &str,
    request: &Q,
    words: Words,
) -> Result<R, MeshError> {
    let (mut tx, mut rx) = client.open(protocol).await?;
    tx.send(request).await.map_err(MeshError::Link)?;
    timeout(limit, rx.recv::<R>())
        .await
        .map_err(|e| timed_out(e, words.silent))?
        .map_err(MeshError::Link)?
        .ok_or_else(|| stream_closed(words.closed))
}

/// Sends `request` to the node's shared folders and returns its reply.
pub async fn files(
    client: &PeerClient,
    request: &files::Request,
) -> Result<files::Reply, MeshError> {
    let words = Words {
        silent: "the node stopped answering",
        closed: "the node closed the files session",
    };
    exchange(client, FILES_LIMIT, files::PROTOCOL, request, words).await
}

/// Sends `request` to the node's agent sign-ins and returns how that
/// sign-in stands.
pub async fn sign_in(
    client: &PeerClient,
    request: &sign_in::Request,
) -> Result<sign_in::State, MeshError> {
    if let sign_in::Request::State { tool } = request
        && let Some(reply) = client
            .snapshot(beep_proto::node::events::Read::SignIn { tool: tool.clone() })
            .await
    {
        return match reply? {
            beep_proto::node::events::Value::SignIn(state) => Ok(state),
            other => Err(unexpected(&other)),
        };
    }

    let words = Words {
        silent: "the machine did not answer about the sign-in",
        closed: "the machine closed the sign-in without an answer",
    };
    exchange(client, SIGN_IN_LIMIT, sign_in::PROTOCOL, request, words).await
}

/// The last `lines` lines of the node's log.
pub async fn log(client: &PeerClient, lines: u32) -> Result<String, MeshError> {
    let words = Words {
        silent: "the node took too long to read its log",
        closed: "the node closed the stream without its log",
    };
    let reply = exchange(
        client,
        LOG_LIMIT,
        logs::PROTOCOL,
        &logs::Request { lines },
        words,
    )
    .await?;

    match reply {
        logs::Reply::Log { text } => Ok(text),
        logs::Reply::Failed { reason } => Err(said(reason)),
        other => Err(unexpected(&other)),
    }
}

/// The folder the node looks for checkouts in, as written.
pub async fn search(client: &PeerClient) -> Result<String, MeshError> {
    ask_search(client, search::Request::Get {}, SEARCH_LIMIT).await
}

/// Has the node look in `folder` (empty for home) and walk it again.
pub async fn set_search(client: &PeerClient, folder: &str) -> Result<String, MeshError> {
    let folder = folder.to_owned();
    ask_search(client, search::Request::Set { folder }, SEARCH_LIMIT).await
}

/// Has the node walk its search folder again, quickly, so checkouts made since count.
pub async fn rescan_search(client: &PeerClient) -> Result<String, MeshError> {
    ask_search(client, search::Request::Rescan {}, RESCAN_LIMIT).await
}

async fn ask_search(
    client: &PeerClient,
    request: search::Request,
    limit: Duration,
) -> Result<String, MeshError> {
    let words = Words {
        silent: "the node took too long to answer",
        closed: "the node closed the stream without an answer",
    };

    match exchange(client, limit, search::PROTOCOL, &request, words).await? {
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
        let read = beep_proto::node::events::Read::Process(request.clone());

        if read.topic().is_some()
            && let Some(value) = timeout(TASK_LIMIT, self.0.snapshot(read))
                .await
                .map_err(|e| timed_out(e, "the process subscription did not answer in time"))?
        {
            return match value? {
                beep_proto::node::events::Value::Process(reply) => Ok(reply),
                _ => Err(said("the process subscription returned another resource")),
            };
        }

        let words = Words {
            silent: "the node stopped answering",
            closed: "the node closed the process session",
        };
        exchange(self.0, TASK_LIMIT, process::PROTOCOL, request, words).await
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

/// The tool updates most recently found by the node.
pub async fn tool_updates(client: &PeerClient) -> Result<Vec<tool_update::Available>, MeshError> {
    let words = Words {
        silent: "the node did not answer about tool updates",
        closed: "the node closed the tool update check",
    };
    exchange(
        client,
        Duration::from_secs(60),
        tool_update::CHECK,
        &tool_update::Check {},
        words,
    )
    .await
}

/// Has the node install release `version` of `tool`.
pub async fn install_tool(client: &PeerClient, tool: Tool, version: &str) -> Result<(), MeshError> {
    let request = tool_update::Request::Install {
        tool,
        version: version.to_owned(),
    };
    let words = Words {
        silent: "the install took too long",
        closed: "the node closed the install without an answer",
    };

    match exchange(client, TOOL_INSTALL, tool_update::PROTOCOL, &request, words).await? {
        tool_update::Reply::Installed {} => Ok(()),
        tool_update::Reply::Failed { reason } => Err(said(reason)),
        other => Err(unexpected(&other)),
    }
}

/// Has the node install `need`: `true` once it is installed, `false` when
/// the system finishes it in a window of its own, such as the Xcode tools
/// dialog on macOS.
pub async fn set_up(client: &PeerClient, need: Need) -> Result<bool, MeshError> {
    let words = Words {
        silent: "the install took too long",
        closed: "the node closed the install without an answer",
    };

    match exchange(
        client,
        TOOL_INSTALL,
        setup::PROTOCOL,
        &setup::Request::Install { need },
        words,
    )
    .await?
    {
        setup::Reply::Installed {} => Ok(true),
        setup::Reply::Started {} => Ok(false),
        setup::Reply::Failed { reason } => Err(said(reason)),
        other => Err(unexpected(&other)),
    }
}

/// What the node said it did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Updated {
    pub from: String,
    pub to: String,
    pub restarting: bool,
    pub note: String,
}

/// Has the node install the newest release and restart, once its agent
/// sessions are idle or, with `now`, at once.
pub async fn update_node(client: &PeerClient, now: bool) -> Result<Updated, MeshError> {
    let request = update::Request::Update { now };
    let words = Words {
        silent: "the update took too long",
        closed: "the node closed the update without an answer",
    };
    let event = exchange(client, NODE_INSTALL, update::PROTOCOL, &request, words).await?;
    updated(event)
}

fn updated(event: update::Event) -> Result<Updated, MeshError> {
    match event {
        update::Event::Newest { version } => Ok(Updated {
            from: version.clone(),
            to: version,
            restarting: false,
            note: String::new(),
        }),
        update::Event::Installed {
            from,
            to,
            restarting,
            note,
        } => Ok(Updated {
            from,
            to,
            restarting,
            note,
        }),
        update::Event::Failed { reason } => Err(said(reason)),
        other => Err(unexpected(&other)),
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
                None => return Err(stream_closed("the node closed the shell")),
            }
        }
    };
    timeout(limit + GRACE, answer)
        .await
        .map_err(|e| timed_out(e, "the node stopped answering"))?
}

/// Has a Windows node install beep-node in its WSL distribution `distro`,
/// keep it running and join it to the mesh with `ticket`. `step` hears each
/// step as it begins, in words for the user.
pub async fn add_wsl(
    client: &PeerClient,
    distro: &str,
    ticket: SecretText,
    step: impl FnMut(String),
) -> Result<(), MeshError> {
    let request = wsl::Request::Add {
        distro: distro.to_owned(),
        ticket,
    };
    ask_wsl(client, &request, step).await
}

/// Has a Windows node stop running the node of its WSL distribution `distro`.
pub async fn stop_wsl(client: &PeerClient, distro: &str) -> Result<(), MeshError> {
    let request = wsl::Request::Stop {
        distro: distro.to_owned(),
    };
    ask_wsl(client, &request, drop).await
}

/// Has a Windows node run the node of its WSL distribution `distro` again.
pub async fn start_wsl(client: &PeerClient, distro: &str) -> Result<(), MeshError> {
    let request = wsl::Request::Start {
        distro: distro.to_owned(),
    };
    ask_wsl(client, &request, drop).await
}

/// Has a Windows node look again at what each of its WSL distributions has.
pub async fn probe_wsl(client: &PeerClient) -> Result<(), MeshError> {
    ask_wsl(client, &wsl::Request::Probe {}, drop).await
}

async fn ask_wsl(
    client: &PeerClient,
    request: &wsl::Request,
    mut step: impl FnMut(String),
) -> Result<(), MeshError> {
    let (mut tx, mut rx) = client.open(wsl::PROTOCOL).await?;
    tx.send(request).await.map_err(MeshError::Link)?;

    loop {
        let event = timeout(WSL_STEP, rx.recv::<wsl::Event>())
            .await
            .map_err(|e| timed_out(e, "the machine stopped answering"))?
            .map_err(MeshError::Link)?;

        match event {
            Some(wsl::Event::Step { text }) => step(text),
            Some(wsl::Event::Done {}) => return Ok(()),
            Some(wsl::Event::Failed { reason }) => return Err(said(reason)),
            Some(other) => return Err(unexpected(&other)),
            None => {
                return Err(stream_closed(
                    "the machine closed the stream without an answer",
                ));
            }
        }
    }
}

#[cfg(test)]
#[path = "../tests/machine_node.rs"]
mod tests;
