//! The jobs queued for each machine, and what the desktop shows while they
//! wait.

use beep_proto::{
    SessionId,
    node::{
        device::DeviceId,
        mcp_servers::{Harness as McpHarness, NativeServer},
    },
};
use iroh::EndpointId;
use std::{
    collections::{HashMap, HashSet},
    hash::Hash,
    sync::{
        Mutex, PoisonError,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::sync::oneshot;

/// Default fast window for interactive mutations before handing off to the node actor.
pub(crate) const FAST_JOB_WAIT: Duration = Duration::from_millis(250);
/// Default TTL for queued node jobs if a machine stays offline.
const DEFAULT_JOB_TTL: Duration = Duration::from_secs(600);
/// Shorter TTL for transient process signals.
const PROCESS_JOB_TTL: Duration = Duration::from_secs(60);
/// What a job's waiters hear when it was not delivered before its time ran out.
const NOT_SENT: &str = "the machine did not take it in time";

/// Outbound node-level mutation handled asynchronously by a machine's actor.
#[derive(Clone, Debug)]
pub(crate) enum JobKind {
    TerminalStop {
        id: uuid::Uuid,
        task: u32,
        pty: bool,
        command: String,
    },
    DeleteSession {
        agent: String,
        session: SessionId,
    },
    RelocateSession {
        agent: String,
        session: SessionId,
        folder: String,
    },
    StopProcess {
        session: SessionId,
        pid: u32,
    },
    DevicePower {
        id: DeviceId,
        boot: bool,
    },
    InstallAdapters,
    SetSearch {
        search: String,
    },
    ApplyMcpServer {
        harness: McpHarness,
        server: NativeServer,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) enum JobKey {
    TerminalStop { id: uuid::Uuid },
    DeleteSession { session: SessionId },
    RelocateSession { session: SessionId },
    StopProcess { session: SessionId, pid: u32 },
    DevicePower { id: DeviceId },
    InstallAdapters,
    SetSearch,
    ApplyMcpServer { harness: String, name: String },
}

/// What runs in order on a machine.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) enum Lane {
    Session(SessionId),
    Key(JobKey),
}

impl JobKind {
    fn key(&self) -> JobKey {
        match self {
            Self::TerminalStop { id, .. } => JobKey::TerminalStop { id: *id },
            Self::DeleteSession { session, .. } => JobKey::DeleteSession {
                session: session.clone(),
            },
            Self::RelocateSession { session, .. } => JobKey::RelocateSession {
                session: session.clone(),
            },
            Self::StopProcess { session, pid } => JobKey::StopProcess {
                session: session.clone(),
                pid: *pid,
            },
            Self::DevicePower { id, .. } => JobKey::DevicePower { id: id.clone() },
            Self::InstallAdapters => JobKey::InstallAdapters,
            Self::SetSearch { .. } => JobKey::SetSearch,
            Self::ApplyMcpServer { harness, server } => JobKey::ApplyMcpServer {
                harness: harness.as_ref().to_owned(),
                name: server.name.clone(),
            },
        }
    }

    /// The jobs of one lane run one after another; those of different lanes together. A
    /// session's delete and move share a lane, so they keep the order they were asked in.
    pub(super) fn lane(&self) -> Lane {
        match self {
            Self::DeleteSession { session, .. } | Self::RelocateSession { session, .. } => {
                Lane::Session(session.clone())
            }
            _ => Lane::Key(self.key()),
        }
    }

    fn ttl(&self) -> Duration {
        match self {
            Self::StopProcess { .. } => PROCESS_JOB_TTL,
            _ => DEFAULT_JOB_TTL,
        }
    }
}

/// Whether a submitted node job settled during the fast window or stayed queued for retry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum JobOutcome {
    Delivered,
    Queued,
}

type JobWaiter = oneshot::Sender<std::result::Result<(), String>>;

struct QueuedJob {
    id: u64,
    kind: JobKind,
    sending: bool,
    attempts: u32,
    deadline: Instant,
    waiters: Vec<JobWaiter>,
}

#[derive(Default)]
struct HeldOps {
    queues: HashMap<EndpointId, Vec<QueuedJob>>,
    /// The lanes a worker runs now.
    running: HashSet<(EndpointId, Lane)>,
    deleting: HashSet<(EndpointId, SessionId)>,
    relocating: HashMap<(EndpointId, SessionId), String>,
    closing_terminals: HashSet<uuid::Uuid>,
}

/// Per-machine outbound mutation actor: coalesces duplicate actions, gives healthy nodes a
/// fast completion window, and retries degraded or offline nodes in the background.
#[derive(Default)]
pub(crate) struct NodeOps {
    next_id: AtomicU64,
    held: Mutex<HeldOps>,
}

pub(super) enum JobFailure {
    Transport,
    Refused(String),
}

impl NodeOps {
    pub(crate) fn is_deleting_session(&self, machine_id: &str, session: &SessionId) -> bool {
        let Ok(machine) = machine_id.parse::<EndpointId>() else {
            return false;
        };
        self.is_deleting_session_on(machine, session)
    }

    pub(crate) fn is_deleting_session_on(&self, machine: EndpointId, session: &SessionId) -> bool {
        self.held
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .deleting
            .contains(&(machine, session.clone()))
    }

    pub(crate) fn relocated_folder(&self, machine_id: &str, session: &SessionId) -> Option<String> {
        let machine = machine_id.parse::<EndpointId>().ok()?;
        self.held
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .relocating
            .get(&(machine, session.clone()))
            .cloned()
    }

    pub(crate) fn is_closing_terminal(&self, id: uuid::Uuid) -> bool {
        self.held
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .closing_terminals
            .contains(&id)
    }

    /// Queues `kind`; true when its lane has no worker, which the caller starts.
    pub(super) fn push_job(
        &self,
        machine: EndpointId,
        kind: JobKind,
        waiter: Option<JobWaiter>,
    ) -> bool {
        let mut held = self.held.lock().unwrap_or_else(PoisonError::into_inner);

        match &kind {
            JobKind::TerminalStop { id, .. } => {
                held.closing_terminals.insert(*id);
            }
            JobKind::DeleteSession { session, .. } => {
                held.deleting.insert((machine, session.clone()));
                held.relocating.remove(&(machine, session.clone()));
            }
            JobKind::RelocateSession {
                session, folder, ..
            } => {
                held.relocating
                    .insert((machine, session.clone()), folder.clone());
            }
            _ => {}
        }

        let key = kind.key();
        let lane = kind.lane();
        let deadline = Instant::now() + kind.ttl();
        let queue = held.queues.entry(machine).or_default();

        // A delete is idempotent: a second one while the first is sending shares its outcome
        // instead of asking for a session the node has already removed.
        let joins_sending = matches!(kind, JobKind::DeleteSession { .. });

        if let Some(existing) = queue
            .iter_mut()
            .find(|job| job.kind.key() == key && (!job.sending || joins_sending))
        {
            existing.kind = kind;
            existing.deadline = deadline;
            if let Some(waiter) = waiter {
                existing.waiters.push(waiter);
            }
        } else {
            let id = self.next_id.fetch_add(1, Ordering::Relaxed) + 1;
            queue.push(QueuedJob {
                id,
                kind,
                sending: false,
                attempts: 0,
                deadline,
                waiters: waiter.into_iter().collect(),
            });
        }

        held.running.insert((machine, lane))
    }

    /// The next job of `lane` to send, and the jobs of it that waited in the queue past their
    /// time, which are settled as failed: their waiters hear of it and the caller tells the
    /// windows. With none left to send the lane has no worker.
    pub(super) fn next_due(
        &self,
        machine: EndpointId,
        lane: &Lane,
    ) -> (Option<(u64, JobKind, u32)>, Vec<JobKind>) {
        let mut held = self.held.lock().unwrap_or_else(PoisonError::into_inner);
        let now = Instant::now();
        let mut expired = Vec::new();

        if let Some(queue) = held.queues.get_mut(&machine) {
            let mut kept = Vec::new();

            for job in std::mem::take(queue) {
                if job.kind.lane() == *lane && !job.sending && now >= job.deadline {
                    for waiter in job.waiters {
                        let _ = waiter.send(Err(NOT_SENT.to_owned()));
                    }
                    expired.push(job.kind);
                } else {
                    kept.push(job);
                }
            }

            *queue = kept;
        }

        for kind in &expired {
            clear_optimistic(&mut held, machine, kind);
        }

        if let Some(job) = held
            .queues
            .get_mut(&machine)
            .and_then(|queue| queue.iter_mut().find(|job| job.kind.lane() == *lane))
        {
            job.sending = true;
            return (Some((job.id, job.kind.clone(), job.attempts)), expired);
        }

        if held.queues.get(&machine).is_some_and(Vec::is_empty) {
            held.queues.remove(&machine);
        }

        held.running.remove(&(machine, lane.clone()));
        (None, expired)
    }

    /// Frees `lane` after its worker panicked: its jobs are sendable again and the next
    /// `push_job` starts a new worker.
    pub(super) fn free_lane(&self, machine: EndpointId, lane: &Lane) {
        let mut held = self.held.lock().unwrap_or_else(PoisonError::into_inner);

        held.running.remove(&(machine, lane.clone()));

        if let Some(queue) = held.queues.get_mut(&machine) {
            for job in queue.iter_mut().filter(|job| job.kind.lane() == *lane) {
                job.sending = false;
            }
        }
    }

    pub(super) fn settle_job(
        &self,
        machine: EndpointId,
        id: u64,
        result: std::result::Result<(), JobFailure>,
    ) -> Option<(JobKind, std::result::Result<(), String>)> {
        let mut held = self.held.lock().unwrap_or_else(PoisonError::into_inner);
        let queue = held.queues.get_mut(&machine)?;
        let pos = queue.iter().position(|job| job.id == id)?;

        match result {
            Ok(()) => {
                let job = queue.remove(pos);
                clear_optimistic(&mut held, machine, &job.kind);
                for waiter in job.waiters {
                    let _ = waiter.send(Ok(()));
                }
                Some((job.kind, Ok(())))
            }
            Err(JobFailure::Refused(why)) => {
                let job = queue.remove(pos);
                clear_optimistic(&mut held, machine, &job.kind);
                for waiter in job.waiters {
                    let _ = waiter.send(Err(why.clone()));
                }
                Some((job.kind, Err(why)))
            }
            Err(JobFailure::Transport) => {
                let job = &mut queue[pos];
                job.sending = false;
                job.attempts = job.attempts.saturating_add(1);
                // Releasing waiters lets `submit` return `Queued` immediately when offline.
                job.waiters.clear();
                if Instant::now() >= job.deadline {
                    let expired = queue.remove(pos);
                    clear_optimistic(&mut held, machine, &expired.kind);
                    return Some((expired.kind, Err(NOT_SENT.to_owned())));
                }
                None
            }
        }
    }
}

fn clear_optimistic(held: &mut HeldOps, machine: EndpointId, kind: &JobKind) {
    match kind {
        JobKind::TerminalStop { id, .. } => {
            held.closing_terminals.remove(id);
        }
        JobKind::DeleteSession { session, .. } => {
            held.deleting.remove(&(machine, session.clone()));
        }
        JobKind::RelocateSession { session, .. } => {
            held.relocating.remove(&(machine, session.clone()));
        }
        _ => {}
    }
}

#[cfg(test)]
#[path = "../tests/fleet_jobs.rs"]
mod tests;
