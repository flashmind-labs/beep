//! `tools/1` as a node answers it for another node of its mesh: the agent
//! tools that act on this machine. The server runs the same calls for a node
//! that has one; between nodes there is no server to ask, and no rules to
//! check, since the mesh's members are the user's own machines.

use crate::{
    files::transfer::{Incoming, Transfers},
    link::Offers,
    machine::service::logs,
    run::tasks::{self, Table},
};
use anyhow::{Context, Result};
use beep_proto::node::{
    LinkRx, LinkTx,
    process::{Reply as TaskReply, Request as TaskRequest, TaskId},
    sync::{ProjectStatus, State as SyncState},
    tools::{Reply, Request, Tool},
};
use serde::Deserialize;
use serde_json::Value;
use std::{fmt::Write, time::Duration};
use tokio::{
    sync::Semaphore,
    task::spawn_blocking,
    time::{Instant, sleep, timeout},
};

/// How long a peer has to send its call.
const READ_LIMIT: Duration = Duration::from_secs(10);
/// The longest a command may run; the default when the call sets none.
const MAX_RUN_SECS: u64 = 1800;
const DEFAULT_RUN_SECS: u64 = 600;
/// The log lines a call gets when it names no count.
const DEFAULT_LOG_LINES: u32 = 100;

/// Who calls, and what this device lets them do.
#[derive(Clone)]
pub(crate) struct Caller {
    /// The tasks the caller's jobs live in and can see.
    pub table: Table,
    /// Whether a node's `RunOn`, `MachineLog` and `SendFiles` may act on this
    /// device: not when it is a desktop.
    pub may_run_on_here: bool,
    /// This node's synced projects and transfers, when it shares folders.
    pub transfers: Option<Transfers>,
}

impl Caller {
    /// This node's own agent calling it.
    pub(crate) fn local(offers: &Offers) -> Self {
        Self {
            table: Table::Shared,
            may_run_on_here: true,
            transfers: offers.transfers.get(),
        }
    }
}

/// Serves one `tools/1` call: its request, then its reply.
pub(crate) async fn serve(mut tx: LinkTx, mut rx: LinkRx, caller: Caller) -> Result<()> {
    let request: Request = timeout(READ_LIMIT, rx.recv())
        .await
        .context("the peer sent no call")??
        .context("the stream closed before its call")?;
    tx.send(&answer(request, caller).await).await?;
    tx.finish().await
}

/// A result for the agent, or why the call did not work.
type Outcome = Result<String, String>;

pub(crate) async fn answer(request: Request, caller: Caller) -> Reply {
    let outcome = match serde_json::from_str::<Value>(&request.args) {
        Err(e) => Err(format!("The arguments are not JSON: {e}")),
        Ok(args) => match request.tool {
            Tool::RunOn | Tool::MachineLog if !caller.may_run_on_here => Err(
                "This device is a desktop: another node may not run commands on it or read its log."
                    .to_owned(),
            ),
            Tool::RunOn => match parse(args) {
                Ok(args) => run_on(args, caller.table).await,
                Err(e) => Err(e),
            },
            Tool::JobStatus => job(args, false, caller.table).await,
            Tool::Cancel => job(args, true, caller.table).await,
            Tool::MachineLog => machine_log(args).await,
            // Writes outside a project the desktop's owner shares: a node may
            // not send files to a desktop. Syncing is confined to a project
            // this device already syncs, with the peers a desktop named.
            Tool::SendFiles if !caller.may_run_on_here => Err(
                "This device is a desktop: another node may not send files to it.".to_owned(),
            ),
            Tool::SyncProject => sync_project(args, &caller).await,
            Tool::SendFiles => receive_files(args, &caller).await,
            other => Err(format!("A node does not serve {other} for another node.")),
        },
    };
    match outcome {
        Ok(text) => Reply::Done {
            text,
            failed: false,
        },
        Err(text) => Reply::Done { text, failed: true },
    }
}

fn parse<T: for<'de> Deserialize<'de>>(args: Value) -> Result<T, String> {
    serde_json::from_value(args).map_err(|e| format!("The arguments do not fit: {e}"))
}

#[derive(Deserialize)]
struct RunOn {
    command: Option<String>,
    prompt: Option<String>,
    cwd: Option<String>,
    #[serde(default)]
    background: bool,
    timeout_secs: Option<u64>,
}

async fn run_on(args: RunOn, table: Table) -> Outcome {
    let command = match (args.command, args.prompt) {
        (Some(command), None) if !command.trim().is_empty() => command,
        (None, Some(_)) => return Err("A node runs commands for another node, not agents.".into()),
        _ => return Err("Give `command`.".into()),
    };
    if args.background {
        return match tasks::answer(
            table,
            TaskRequest::Start {
                command,
                cwd: args.cwd,
            },
        )
        .await
        {
            TaskReply::Started { id } => Ok(format!(
                "Started it in the background. Job id: {id}. Check it with job_status, \
                 stop it with cancel."
            )),
            other => Err(task_error(other)),
        };
    }
    let secs = args
        .timeout_secs
        .unwrap_or(DEFAULT_RUN_SECS)
        .clamp(1, MAX_RUN_SECS);
    status_text(tasks::run_for(table, command, args.cwd, Duration::from_secs(secs)).await)
}

#[derive(Deserialize)]
struct JobArgs {
    id: String,
}

async fn job(args: Value, cancel: bool, table: Table) -> Outcome {
    let JobArgs { id } = parse(args)?;
    let id = id
        .trim()
        .parse()
        .map(TaskId)
        .map_err(|_| format!("{id:?} is not a job id."))?;
    let request = if cancel {
        TaskRequest::Stop { id }
    } else {
        TaskRequest::Status { id }
    };
    status_text(tasks::answer(table, request).await)
}

#[derive(Deserialize)]
struct LogArgs {
    lines: Option<u32>,
}

async fn machine_log(args: Value) -> Outcome {
    let LogArgs { lines } = parse(args)?;
    let lines = lines.unwrap_or(DEFAULT_LOG_LINES);
    spawn_blocking(move || logs::recent(None, lines))
        .await
        .map_err(|e| format!("{e}"))?
        .map_err(|e| format!("{e:#}"))
}

#[derive(Deserialize)]
struct SyncArgs {
    project: String,
}

/// How long `sync_project` waits for the project to settle.
const SYNC_WAIT: Duration = Duration::from_secs(90);
const SYNC_POLL: Duration = Duration::from_millis(500);

/// The most `sync_project` calls waiting on a project at once, from every
/// peer together.
const MAX_SYNCS: usize = 4;
static SYNC_SLOTS: Semaphore = Semaphore::const_new(MAX_SYNCS);

/// Scans the project here, then waits until it has what the other machines
/// have, and says how it stands.
async fn sync_project(args: Value, caller: &Caller) -> Outcome {
    let SyncArgs { project } = parse(args)?;
    // Capped for node peers only; the server's calls are not theirs to crowd.
    let _slot =
        if matches!(caller.table, Table::Peer(_)) {
            Some(SYNC_SLOTS.try_acquire().map_err(|_| {
                format!("busy: {MAX_SYNCS} syncs are already waiting on this machine")
            })?)
        } else {
            None
        };
    let Some(transfers) = &caller.transfers else {
        return Err("This machine shares no folders, so it syncs no projects.".to_owned());
    };
    let syncs = transfers.syncs();
    let status = syncs.rescan(&project).await.map_err(|e| format!("{e:#}"))?;
    let deadline = Instant::now() + SYNC_WAIT;
    let mut status = status;
    while !settled(&status) && Instant::now() < deadline {
        sleep(SYNC_POLL).await;
        match syncs.status().into_iter().find(|s| s.name == project) {
            Some(now) => status = now,
            None => return Err(format!("{project} stopped syncing here.")),
        }
    }
    Ok(sync_line(&status))
}

/// Whether a project has nothing left to bring in line.
fn settled(status: &ProjectStatus) -> bool {
    match status.state {
        SyncState::Idle => status.pending == 0 && status.progress.is_none(),
        SyncState::Failed | SyncState::Paused => true,
        _ => false,
    }
}

fn sync_line(status: &ProjectStatus) -> String {
    let mut line = match status.state {
        SyncState::Idle if status.pending == 0 => format!("in sync, {} files", status.files),
        SyncState::Failed => format!(
            "failed: {}",
            status.error.as_deref().unwrap_or("no reason given")
        ),
        SyncState::Paused => "paused by the user".to_owned(),
        _ => match status.progress {
            Some(progress) => format!(
                "syncing, {} of {} files",
                progress.files_done, progress.files_total
            ),
            None => format!("syncing, {} files pending", status.pending),
        },
    };
    if !status.conflicts.is_empty() {
        // Writing to a string cannot fail.
        let _ = write!(line, "; {} conflict copies", status.conflicts.len());
    }
    line
}

/// Takes the file another node's agent offers: the call's args are the
/// `Incoming` that node built after offering it.
async fn receive_files(args: Value, caller: &Caller) -> Outcome {
    let Some(transfers) = &caller.transfers else {
        return Err("This machine shares no folders, so it takes no files.".to_owned());
    };
    let incoming: Incoming = parse(args)?;
    let name = incoming.name.clone();
    let received = transfers
        .receive(incoming)
        .await
        .map_err(|e| format!("{e:#}"))?;
    Ok(format!(
        "{name} landed at {} ({} bytes)",
        received.path.display(),
        received.size
    ))
}

fn status_text(reply: TaskReply) -> Outcome {
    let TaskReply::Status {
        task,
        lines,
        earlier,
    } = reply
    else {
        return Err(task_error(reply));
    };
    let mut text = format!("{}: {}", task.command, task.state);
    if earlier {
        text.push_str("\n(earlier output left out)");
    }
    for line in lines {
        write!(text, "\n{line}").map_err(|e| e.to_string())?;
    }
    Ok(text)
}

fn task_error(reply: TaskReply) -> String {
    match reply {
        TaskReply::Error { message } => message,
        other => format!("This node answered {other:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn caller(table: Table, may_run_on_here: bool) -> Caller {
        Caller {
            table,
            may_run_on_here,
            transfers: None,
        }
    }

    fn run_on() -> Request {
        Request {
            session: "a3f9c1".to_owned(),
            tool: Tool::RunOn,
            args: r#"{"command":"echo ran-here"}"#.to_owned(),
            approved: Default::default(),
        }
    }

    #[tokio::test]
    async fn a_desktop_takes_no_files_from_a_node_but_syncs_for_it() {
        let table = Table::Peer(iroh::SecretKey::generate().public());
        let on_desktop = caller(table, false);
        let send = Request {
            session: "a3f9c1".to_owned(),
            tool: Tool::SendFiles,
            args: "{}".to_owned(),
            approved: Default::default(),
        };
        let Reply::Done { text, failed } = answer(send, on_desktop.clone()).await else {
            panic!("not done");
        };
        assert!(failed && text.contains("desktop"), "{text}");
        // Syncing is not refused for being a desktop: it is confined to a
        // project the device already syncs.
        let sync = Request {
            session: "a3f9c1".to_owned(),
            tool: Tool::SyncProject,
            args: r#"{"project":"game"}"#.to_owned(),
            approved: Default::default(),
        };
        let Reply::Done { text, .. } = answer(sync, on_desktop).await else {
            panic!("not done");
        };
        assert!(!text.contains("desktop"), "{text}");
    }

    #[tokio::test]
    async fn a_desktop_does_not_show_its_log_to_a_node() {
        let table = Table::Peer(iroh::SecretKey::generate().public());
        let caller = caller(table, false);
        let request = Request {
            session: "a3f9c1".to_owned(),
            tool: Tool::MachineLog,
            args: r#"{"lines":5}"#.to_owned(),
            approved: Default::default(),
        };
        let Reply::Done { text, failed } = answer(request, caller).await else {
            panic!("not done");
        };
        assert!(failed && text.contains("desktop"), "{text}");
    }

    #[tokio::test]
    async fn a_desktop_refuses_run_on_from_a_node_and_a_node_takes_it() {
        let table = Table::Peer(iroh::SecretKey::generate().public());
        let on_desktop = caller(table, false);
        let Reply::Done { text, failed } = answer(run_on(), on_desktop).await else {
            panic!("not done");
        };
        assert!(failed && text.contains("desktop"), "{text}");
        assert!(!text.contains("ran-here"));
        let on_node = caller(table, true);
        let Reply::Done { text, failed } = answer(run_on(), on_node).await else {
            panic!("not done");
        };
        assert!(!failed && text.contains("ran-here"), "{text}");
    }
}
