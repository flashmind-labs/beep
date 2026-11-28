//! Per-machine actors for background mutations (`NodeOps`), so degraded or offline
//! `beep-node` links never block UI commands or stall healthy machines.

use super::{
    Fleet, alerts,
    jobs::{JOB_FAILED, JobFailure, JobKind, JobOutcome, Lane, NodeOps},
    projections::Listing,
    session_lists::SessionLists,
    session_mirror::SessionMirror,
};
use crate::error::{DesktopError, Result};
use beep_mesh::{
    PeerClient, RETRY,
    machine::{
        self,
        sessions::{self, NodeWork},
        terminals::{self, Kept as KeptTerminal},
    },
};
use beep_proto::node::{
    agent::{SESSIONS_PROTOCOL, SessionsReply, SessionsRequest},
    device::{
        DeviceId, DeviceState, PROTOCOL as DEVICE_PROTOCOL, Reply as DeviceReply,
        Request as DeviceRequest,
    },
    mcp_servers::{PROTOCOL as MCP_PROTOCOL, Reply as McpReply, Request as McpRequest},
};
use futures::FutureExt;
use iroh::EndpointId;
use std::{panic::AssertUnwindSafe, sync::Arc, time::Duration};
use tauri::{AppHandle, Emitter, Manager};
use tokio::{sync::oneshot, time::timeout};

impl NodeOps {
    /// Submits a job to `machine`'s actor, returning its immediate outcome if the machine answers
    /// within `fast_wait` or `JobOutcome::Queued` if it is slow or offline.
    pub(crate) async fn submit(
        self: &Arc<Self>,
        app: Option<AppHandle>,
        fleet: Arc<Fleet>,
        machine: EndpointId,
        kind: JobKind,
        fast_wait: Duration,
    ) -> Result<JobOutcome> {
        let (tx, rx) = oneshot::channel();
        let lane = kind.lane();

        if self.push_job(machine, kind, Some(tx)) {
            self.spawn_worker(app, fleet, machine, lane);
        }

        match timeout(fast_wait, rx).await {
            Ok(Ok(Ok(()))) => Ok(JobOutcome::Delivered),
            Ok(Ok(Err(why))) => Err(DesktopError::BadRequest(why)),
            Ok(Err(_)) | Err(_) => Ok(JobOutcome::Queued),
        }
    }

    fn spawn_worker(
        self: &Arc<Self>,
        app: Option<AppHandle>,
        fleet: Arc<Fleet>,
        machine: EndpointId,
        lane: Lane,
    ) {
        let this = self.clone();
        tauri::async_runtime::spawn(async move {
            this.run_lane(app, fleet, machine, lane).await;
        });
    }

    /// Sends the jobs of one lane of `machine` in order until none is left. Another lane of the
    /// machine runs meanwhile, so a long job, such as a device boot, holds back only its own.
    pub(super) async fn run_lane(
        self: &Arc<Self>,
        app: Option<AppHandle>,
        fleet: Arc<Fleet>,
        machine: EndpointId,
        lane: Lane,
    ) {
        loop {
            let (due, expired) = self.next_due(machine, &lane);

            for kind in &expired {
                on_settled(app.as_ref(), &fleet, machine, kind, false);
            }

            let Some((id, kind, attempts)) = due else {
                return;
            };
            // A panicking job is dropped with an error for its waiters; the lane carries on.
            let outcome = match AssertUnwindSafe(execute_job(&fleet, machine, &kind, attempts))
                .catch_unwind()
                .await
            {
                Ok(outcome) => outcome,
                Err(_) => Err(JobFailure::Refused(JOB_FAILED.to_owned())),
            };

            match self.settle_job(machine, id, outcome) {
                Some((done_kind, settled)) => {
                    on_settled(app.as_ref(), &fleet, machine, &done_kind, settled.is_ok());
                }
                None => {
                    alerts::until_online(&fleet, machine).await;
                    tokio::time::sleep(RETRY).await;
                }
            }
        }
    }
}

fn on_settled(
    app: Option<&AppHandle>,
    fleet: &Fleet,
    machine: EndpointId,
    kind: &JobKind,
    ok: bool,
) {
    match kind {
        // The command that asked for the delete took the session out of every list. Only a
        // refusal needs more: the machine still has it.
        JobKind::DeleteSession { .. } if !ok => {
            if let Some(app) = app {
                if let Some(lists) = app.try_state::<SessionLists>() {
                    lists.stale(machine);
                }
                if let Some(mirror) = app.try_state::<Arc<SessionMirror>>() {
                    mirror.delete_failed(machine);
                    // The windows draw at once; the relist follows.
                    super::home_signal::poke(app);
                }
            }
        }
        // The node pushes a move that went through, but a past session is only in the lists'
        // history answer, which still has its old folder. Only a refusal needs the mirror read.
        JobKind::RelocateSession {
            session, folder, ..
        } => {
            if let Some(app) = app {
                if ok && let Some(lists) = app.try_state::<SessionLists>() {
                    lists.relocated(machine, session.clone(), folder.clone());
                }
                if !ok && let Some(mirror) = app.try_state::<Arc<SessionMirror>>() {
                    mirror.moved(machine);
                }
            }
        }
        JobKind::DevicePower { .. } => {
            fleet.projections().devices.forget(&machine);
            if let Some(app) = app {
                let _ = app.emit("devices-changed", ());
            }
        }
        JobKind::SetSearch { .. } if !ok => {
            fleet.projections().search.forget(&machine);
        }
        JobKind::InstallAdapters => {
            fleet.projections().harnesses.forget(&machine);
        }
        JobKind::TerminalStop { .. } if !ok => {
            if let Some(app) = app {
                super::home_signal::poke(app);
            }
        }
        JobKind::DeleteSession { .. }
        | JobKind::TerminalStop { .. }
        | JobKind::StopProcess { .. }
        | JobKind::SetSearch { .. }
        | JobKind::ApplyMcpServer { .. } => {}
    }
}

async fn client_within(
    fleet: &Fleet,
    machine: EndpointId,
    limit: Duration,
) -> std::result::Result<PeerClient, JobFailure> {
    match timeout(limit, fleet.client(machine)).await {
        Ok(Ok(client)) => Ok(client),
        Ok(Err(err)) if err.is_transport() => Err(JobFailure::Transport),
        Ok(Err(err)) => Err(JobFailure::Refused(err.to_string())),
        Err(_) => Err(JobFailure::Transport),
    }
}

async fn execute_job(
    fleet: &Fleet,
    machine: EndpointId,
    kind: &JobKind,
    attempts: u32,
) -> std::result::Result<(), JobFailure> {
    // Test seam: a job of agent "panic" crashes, one of agent "ok" settles at once.
    #[cfg(test)]
    if let JobKind::DeleteSession { agent, .. } | JobKind::RelocateSession { agent, .. } = kind {
        match agent.as_str() {
            "panic" => panic!("job"),
            "ok" => return Ok(()),
            _ => {}
        }
    }

    match kind {
        JobKind::TerminalStop {
            id,
            task,
            pty,
            command,
        } => {
            let client = client_within(fleet, machine, Duration::from_secs(4)).await?;
            let kept = KeptTerminal {
                task: *task,
                pty: *pty,
                command,
            };
            match timeout(Duration::from_secs(5), terminals::stop(&client, &kept)).await {
                Ok(Ok(())) => {
                    super::terminal_rows::forget(fleet, *id)
                        .await
                        .map_err(|_| JobFailure::Transport)?;
                    Ok(())
                }
                Ok(Err(err)) if err.is_transport() => Err(JobFailure::Transport),
                Ok(Err(err)) => Err(JobFailure::Refused(format!(
                    "{}: {err:#}",
                    fleet.name_of(machine)
                ))),
                Err(_) => Err(JobFailure::Transport),
            }
        }
        JobKind::DeleteSession { agent, session } => {
            let client = client_within(fleet, machine, Duration::from_secs(4)).await?;
            match timeout(
                Duration::from_secs(8),
                sessions::delete_session(&client, agent, session),
            )
            .await
            {
                Ok(Ok(NodeWork::Done(()))) => Ok(()),
                Ok(Ok(NodeWork::Refused(why))) => {
                    let lower = why.to_ascii_lowercase();
                    if attempts > 0 && (lower.contains("not found") || lower.contains("no such")) {
                        return Ok(());
                    }
                    Err(JobFailure::Refused(format!(
                        "{}: {why}",
                        fleet.name_of(machine)
                    )))
                }
                Ok(Err(err)) if err.is_transport() => Err(JobFailure::Transport),
                Ok(Err(err)) => Err(JobFailure::Refused(format!(
                    "Could not delete the session on {}: {err:#}",
                    fleet.name_of(machine)
                ))),
                Err(_) => Err(JobFailure::Transport),
            }
        }
        JobKind::RelocateSession {
            agent,
            session,
            folder,
        } => {
            let client = client_within(fleet, machine, Duration::from_secs(4)).await?;
            match timeout(
                Duration::from_secs(8),
                sessions::relocate_session(&client, agent, session, folder),
            )
            .await
            {
                Ok(Ok(NodeWork::Done(()))) => Ok(()),
                Ok(Ok(NodeWork::Refused(why))) => Err(JobFailure::Refused(why)),
                Ok(Err(err)) if err.is_transport() => Err(JobFailure::Transport),
                Ok(Err(err)) => Err(JobFailure::Refused(err.to_string())),
                Err(_) => Err(JobFailure::Transport),
            }
        }
        JobKind::StopProcess { session, pid } => {
            let client = client_within(fleet, machine, Duration::from_secs(4)).await?;
            let reply = client
                .call_within::<_, SessionsReply>(
                    Duration::from_secs(8),
                    SESSIONS_PROTOCOL,
                    &SessionsRequest::StopProcess {
                        session: session.clone(),
                        pid: *pid,
                    },
                )
                .await;
            match reply {
                Ok(SessionsReply::ProcessStopped {}) => Ok(()),
                Ok(SessionsReply::Error { message }) => {
                    let lower = message.to_ascii_lowercase();
                    if attempts > 0 && (lower.contains("not running") || lower.contains("no such"))
                    {
                        return Ok(());
                    }
                    Err(JobFailure::Refused(message))
                }
                Ok(_) => Err(JobFailure::Refused(
                    "The machine cannot stop running commands yet.".into(),
                )),
                Err(err) if err.is_transport() => Err(JobFailure::Transport),
                Err(err) => Err(JobFailure::Refused(err.to_string())),
            }
        }
        JobKind::DevicePower { id, boot } => {
            let client = client_within(fleet, machine, Duration::from_secs(4)).await?;
            let request = if *boot {
                DeviceRequest::Boot { id: id.clone() }
            } else {
                DeviceRequest::Shutdown { id: id.clone() }
            };
            let (_tx, mut rx) = client
                .open_sending(DEVICE_PROTOCOL, &request)
                .await
                .map_err(|err| {
                    if err.is_transport() {
                        JobFailure::Transport
                    } else {
                        JobFailure::Refused(err.to_string())
                    }
                })?;
            match timeout(Duration::from_secs(60), rx.recv::<DeviceReply>()).await {
                Ok(Ok(Some(DeviceReply::Done {}))) => Ok(()),
                Ok(Ok(Some(DeviceReply::Failed { reason } | DeviceReply::Ended { reason }))) => {
                    Err(JobFailure::Refused(reason))
                }
                Ok(Ok(Some(other))) => Err(JobFailure::Refused(format!(
                    "unexpected device reply {other:?}"
                ))),
                Ok(Ok(None)) | Err(_) => Err(JobFailure::Transport),
                Ok(Err(_)) => Err(JobFailure::Transport),
            }
        }
        JobKind::InstallAdapters => {
            let client = client_within(fleet, machine, Duration::from_secs(4)).await?;
            match timeout(Duration::from_secs(15), sessions::install_adapters(&client)).await {
                Ok(Ok(true)) => Ok(()),
                Ok(Ok(false)) => Err(JobFailure::Refused(format!(
                    "Update beep-node on {} to manage its coding agents here.",
                    fleet.name_of(machine)
                ))),
                Ok(Err(err)) if err.is_transport() => Err(JobFailure::Transport),
                Ok(Err(err)) => Err(JobFailure::Refused(err.to_string())),
                Err(_) => Err(JobFailure::Transport),
            }
        }
        JobKind::SetSearch { search } => {
            let client = client_within(fleet, machine, Duration::from_secs(4)).await?;
            match timeout(
                Duration::from_secs(10),
                machine::set_search(&client, search.trim()),
            )
            .await
            {
                Ok(Ok(saved)) => {
                    fleet.projections().search.put(machine, saved);
                    Ok(())
                }
                Ok(Err(err)) if err.is_transport() => Err(JobFailure::Transport),
                Ok(Err(err)) => Err(JobFailure::Refused(err.to_string())),
                Err(_) => Err(JobFailure::Transport),
            }
        }
        JobKind::ApplyMcpServer { harness, server } => {
            let client = client_within(fleet, machine, Duration::from_secs(4)).await?;
            let reply = client
                .call_within::<_, McpReply>(
                    Duration::from_secs(10),
                    MCP_PROTOCOL,
                    &McpRequest::Apply {
                        harness: *harness,
                        server: server.clone(),
                    },
                )
                .await;
            match reply {
                Ok(McpReply::Applied) => Ok(()),
                Ok(McpReply::Refused(err)) => Err(JobFailure::Refused(err)),
                Ok(_) => Err(JobFailure::Refused("Unexpected MCP server reply.".into())),
                Err(beep_mesh::MeshError::Declined { reason, .. })
                    if reason == format!("this node does not offer {MCP_PROTOCOL}") =>
                {
                    Err(JobFailure::Refused(
                        "Update Beep on this machine to manage its MCP servers.".into(),
                    ))
                }
                Err(err) if err.is_transport() => Err(JobFailure::Transport),
                Err(err) => Err(JobFailure::Refused(err.to_string())),
            }
        }
    }
}

/// Optimistically marks `id` booting or off in `machine`'s cached device list.
pub(crate) fn mark_device_power(fleet: &Fleet, machine: EndpointId, id: &DeviceId, boot: bool) {
    fleet.projections().devices.edit(&machine, |listing| {
        let Listing::Devices(devices) = listing else {
            return;
        };

        if let Some(device) = devices.iter_mut().find(|d| d.id == *id) {
            device.state = if boot {
                DeviceState::Booting
            } else {
                DeviceState::Off
            };
        }
    });
}
