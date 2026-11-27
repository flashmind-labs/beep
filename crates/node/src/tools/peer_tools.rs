//! `tools/1` as a node answers it for another device of its mesh: the agent
//! tools that act on this machine. The server runs the same calls for a node
//! that has one; between nodes there is no server to ask, and no rules to
//! check, since the mesh's members are the user's own machines.

use super::{caller::Caller, desktop::answer as answer_projects, team::answer as answer_team};
use crate::{
    files::transfer::Incoming,
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
    select,
    sync::{Semaphore, broadcast::error::RecvError},
    time::{Instant, sleep_until, timeout},
};

/// How long a peer has to send its call.
const READ_LIMIT: Duration = Duration::from_secs(10);
/// The longest a command may run; the default when the call sets none.
const MAX_RUN_SECS: u64 = 1800;
const DEFAULT_RUN_SECS: u64 = 600;

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
    if request.tool == Tool::HarnessInfo {
        return match &caller.agents {
            Some(agents) => super::harness::answer(agents, &request).await,
            None => Reply::Done {
                text: "This machine has no coding harnesses available.".to_owned(),
                failed: true,
            },
        };
    }

    if request.tool == Tool::Projects {
        return answer_projects(caller.project_tools.as_ref(), &request).await;
    }

    // Only the user's desktops join teams; nodes ask and deliver.
    if request.tool == Tool::TeamJoin {
        let team = caller
            .team
            .as_ref()
            .filter(|_| !matches!(caller.table, Table::Peer(_)));
        return crate::team::join::answer(team, &request).await;
    }

    if matches!(
        request.tool,
        Tool::TeamRoster | Tool::TeamInvite | Tool::TeamApp
    ) {
        return crate::team::apps::answer(
            caller.team.as_ref(),
            caller.mesh.as_ref(),
            caller.desktop_here,
            &request,
        )
        .await;
    }

    if matches!(
        request.tool,
        Tool::Team | Tool::TeamAsk | Tool::TeamDeliver | Tool::TeamPickup
    ) {
        return match (&caller.team, &caller.agents) {
            (Some(team), Some(agents)) => {
                let from = match caller.table {
                    Table::Peer(peer) => Some(peer),
                    Table::Shared => None,
                };
                answer_team(team, agents, from, &request).await
            }
            _ => Reply::Done {
                text: "This machine does not act for a team.".to_owned(),
                failed: true,
            },
        };
    }

    if matches!(
        request.tool,
        Tool::StartAgent
            | Tool::AgentStatus
            | Tool::SendToAgent
            | Tool::StopAgent
            | Tool::ListAgents
            | Tool::AgentParent
            | Tool::AgentCount
            | Tool::AgentNote
    ) {
        return agent_call(request, &caller).await;
    }
    if request.tool == Tool::ForgeReach {
        let outcome = match &caller.agents {
            Some(agents) => serde_json::to_string(&agents.ci.reach()).map_err(|e| e.to_string()),
            None => Err("This node watches no CI runs.".to_owned()),
        };
        return match outcome {
            Ok(text) => Reply::Done {
                text,
                failed: false,
            },
            Err(text) => Reply::Done { text, failed: true },
        };
    }

    let outcome = match serde_json::from_str::<Value>(&request.args) {
        Err(e) => Err(format!("The arguments are not JSON: {e}")),
        Ok(args) => match request.tool {
            Tool::RunOn => match (&caller.commands_off, parse(args)) {
                (Some(off), _) => Err(off.clone()),
                (None, Ok(args)) => run_on(args, caller.table).await,
                (None, Err(e)) => Err(e),
            },
            Tool::JobStatus => job(args, false, caller.table).await,
            Tool::Cancel => job(args, true, caller.table).await,
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

/// An agent tool another device's agent calls on this machine.
async fn agent_call(request: Request, caller: &Caller) -> Reply {
    let failed = |text: &str| Reply::Done {
        text: text.to_owned(),
        failed: true,
    };
    let Some(agents) = &caller.agents else {
        return failed("Agents are started through the node's own Beep MCP server.");
    };

    if request.tool == Tool::ListAgents {
        return crate::agents::family::peer::listed(&agents.sessions, &agents.links, &request.args);
    }

    let Some(peer) = caller.from else {
        return failed("Agents are started through the node's own Beep MCP server.");
    };

    if request.tool == Tool::StartAgent
        && let Some(reason) = &caller.commands_off
    {
        return failed(reason);
    }

    super::children::answer_peer(agents, peer, request).await
}

fn parse<T: for<'de> Deserialize<'de>>(args: Value) -> Result<T, String> {
    serde_json::from_value(args).map_err(|e| format!("The arguments do not fit: {e}"))
}

#[derive(Deserialize)]
struct RunOn {
    command: Option<String>,
    prompt: Option<String>,
    agent: Option<String>,
    cwd: Option<String>,
    #[serde(default)]
    background: bool,
    timeout_secs: Option<u64>,
}

async fn run_on(args: RunOn, table: Table) -> Outcome {
    // An older node sends `prompt` (and `agent`) to start an agent. That would
    // skip the parent link and every limit `start_agent` keeps, so it is refused.
    if args.prompt.is_some() || args.agent.is_some() {
        return Err("remote_run starts commands only. Start an agent with start_agent.".into());
    }

    let command = match args.command {
        Some(command) if !command.trim().is_empty() => command,
        _ => return Err("Give `command`.".into()),
    };

    let secs = args
        .timeout_secs
        .unwrap_or(DEFAULT_RUN_SECS)
        .clamp(1, MAX_RUN_SECS);
    let limit = Duration::from_secs(secs);

    if args.background {
        return match tasks::start_for(table, command, args.cwd, limit).await {
            TaskReply::Started { id } => Ok(format!(
                "Started it in the background. Job id: {id}. Check it with remote_run_status, \
                 stop it with remote_run_cancel."
            )),
            other => Err(task_error(other)),
        };
    }

    status_text(tasks::run_for(table, command, args.cwd, limit).await)
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
struct SyncArgs {
    project: String,
}

/// How long `sync_project` waits for the project to settle.
const SYNC_WAIT: Duration = Duration::from_secs(90);

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
        return Err("Sync did not start on this machine, so it syncs no projects.".to_owned());
    };
    let syncs = transfers.syncs();
    // Listening before the scan starts, so no change in between is missed.
    let mut rings = crate::events::system().subscribe();
    let mut projects = syncs.changes();
    let status = syncs.rescan(&project).await.map_err(|e| format!("{e:#}"))?;
    let deadline = Instant::now() + SYNC_WAIT;
    let mut status = status;

    while !settled(&status) {
        let woke = select! {
            _ = sleep_until(deadline) => break,
            ring = rings.recv() => match ring {
                Ok(change) => change.topic.name() == "sync",
                // Rings were missed: look again.
                Err(RecvError::Lagged(_)) => true,
                Err(RecvError::Closed) => break,
            },
            () = projects.changed() => true,
        };

        if !woke {
            continue;
        }

        match syncs.status_for(&project) {
            Some(now) => status = now,
            None => return Err(format!("{project} stopped syncing here.")),
        }
    }

    if status.git_only && status.state != SyncState::Idle {
        return Err(sync_line(&status));
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
        SyncState::Idle if status.pending == 0 && status.git_only =>
            "Git checkout ready; selected ignored folders are in sync. Other uncommitted edits stay on their machine and require an explicit handoff".to_owned(),
        SyncState::Idle if status.pending == 0 => format!("in sync, {} files", status.files),
        SyncState::Failed => format!(
            "failed: {}",
            status.error.as_deref().unwrap_or("no reason given")
        ),
        SyncState::Paused => "paused by the user".to_owned(),
        _ => match &status.progress {
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
        return Err(
            "This machine's file transfers did not start, so it takes no files.".to_owned(),
        );
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
#[path = "../tests/link_peer_tools.rs"]
mod tests;
