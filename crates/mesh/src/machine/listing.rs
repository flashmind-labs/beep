//! A node's sessions as the apps list them, and what a node's replies look
//! like to an app.

use super::sessions::{self, Sessions};
use beep_proto::{
    AgentEntry, AgentEntryKind, AgentModel, AgentModelPick, AgentModels, AgentOption,
    AgentPlanStatus, AgentPlanStep, AgentQuestion, AgentSessionTasks, AgentSessionView,
    AgentStatus, AgentTask, MachineCapability, MachineSession,
    node::{
        Capability,
        agent::{
            self, AnswerOption, ExternalSession, ModelList, OptionKind, ParentLink, PlanStatus,
            Presence, Question, SessionInfo, Status,
        },
    },
};
use futures::future::join_all;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::time::timeout;

/// How long one request to a node may take.
pub const NODE_LIMIT: Duration = Duration::from_secs(3);
/// The agents whose outside sessions are listed, and how many of each.
const EXTERNAL_AGENTS: [&str; 2] = ["claude", "codex"];
const EXTERNAL_LIMIT: u32 = 20;
/// The most sessions a project lists.
pub const MAX_PROJECT_SESSIONS: usize = 50;
/// A node slower than this to list a project's sessions is logged.
const SLOW_NODE: Duration = Duration::from_secs(1);

/// The sessions on one node, Beep's and those started outside it that run
/// now: none when its list fails. Beep's wait for the outside scan; a caller
/// that cannot wait asks for each half apart.
pub async fn node_sessions<S: Sessions + ?Sized>(node: &S, host: &Host) -> Vec<MachineSession> {
    let (mut beep, outside) = tokio::join!(beep_sessions(node, host), outside_sessions(node, host));
    beep.extend(outside);
    beep
}

/// The sessions Beep runs on one node: none when its list fails.
async fn beep_sessions<S: Sessions + ?Sized>(node: &S, host: &Host) -> Vec<MachineSession> {
    linked_sessions(node, host)
        .await
        .into_iter()
        .map(|linked| linked.session)
        .collect()
}

/// A session Beep runs, and the session that started it when an agent did.
#[derive(Debug, Clone)]
pub struct Linked {
    pub session: MachineSession,
    pub parent: Option<ParentLink>,
}

/// `beep_sessions`, with who started each: what an app needs to nest a
/// child under its parent. `MachineSession` stays as it is for the server's
/// apps.
pub async fn linked_sessions<S: Sessions + ?Sized>(node: &S, host: &Host) -> Vec<Linked> {
    match timeout(NODE_LIMIT, sessions::list(node)).await {
        Ok(Ok(listed)) => listed
            .into_iter()
            .map(|info| Linked {
                parent: info.parent.clone(),
                session: beep_session(host, info),
            })
            .collect(),
        Ok(Err(e)) => {
            tracing::warn!("agent sessions not listed; node={} error={e:#}", host.id);
            Vec::new()
        }
        Err(_) => {
            tracing::warn!("agent sessions not listed in time; node={}", host.id);
            Vec::new()
        }
    }
}

/// The sessions started outside Beep that run now, from scanning what each
/// agent keeps on the node. A scan takes seconds.
pub async fn outside_sessions<S: Sessions + ?Sized>(node: &S, host: &Host) -> Vec<MachineSession> {
    join_all(EXTERNAL_AGENTS.map(|agent| outside(node, agent)))
        .await
        .into_iter()
        .flatten()
        .filter(|session| session.taken_over.is_none() && runs_now(session, SystemTime::now()))
        .map(|session| external_session(host, session))
        .collect()
}

/// The sessions of `agent` started outside Beep; none when the node cannot list them.
async fn outside<S: Sessions + ?Sized>(node: &S, agent: &str) -> Vec<ExternalSession> {
    match timeout(
        NODE_LIMIT,
        sessions::external(node, agent, EXTERNAL_LIMIT, None),
    )
    .await
    {
        Ok(Ok(sessions)) => sessions,
        _ => Vec::new(),
    }
}

/// The sessions coding agents ran in `folders` on one node, as a project lists them.
pub async fn project_sessions<S: Sessions + ?Sized>(
    node: &S,
    host: &Host,
    folders: Vec<String>,
) -> Vec<MachineSession> {
    let limit = u32::try_from(MAX_PROJECT_SESSIONS).unwrap_or(u32::MAX);
    let started = Instant::now();
    let listed = timeout(NODE_LIMIT, sessions::in_folders(node, folders, limit)).await;
    if started.elapsed() >= SLOW_NODE {
        tracing::info!(
            "project sessions listed slowly; node={} ms={}",
            host.id,
            started.elapsed().as_millis()
        );
    }
    match listed {
        Ok(Ok(found)) => found
            .into_iter()
            .map(|session| past_session(host, session))
            .collect(),
        Ok(Err(e)) => {
            tracing::info!("project sessions not listed; node={} error={e:#}", host.id);
            Vec::new()
        }
        Err(_) => {
            tracing::info!("project sessions not listed in time; node={}", host.id);
            Vec::new()
        }
    }
}

/// The machine a session runs on.
#[derive(Clone)]
pub struct Host {
    pub id: String,
    pub name: String,
}

/// How long a session waiting for the user stays among those running now
/// after its last entry.
const IDLE_SHOWN: Duration = Duration::from_secs(24 * 60 * 60);

fn unix(seconds: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(seconds)
}

/// Whether a session started outside Beep shows as running now: a process
/// runs it, and it is in a turn or was active within `IDLE_SHOWN`. A
/// terminal left open for days on an answered prompt is not.
fn runs_now(session: &ExternalSession, now: SystemTime) -> bool {
    match session.presence {
        Presence::Working | Presence::Running => true,
        Presence::Waiting => now
            .duration_since(unix(session.last_activity))
            .is_ok_and(|idle| idle < IDLE_SHOWN),
        _ => false,
    }
}

/// A session Beep started; its `last_activity` is its start, as the node reports no later time.
pub fn beep_session(host: &Host, info: SessionInfo) -> MachineSession {
    let started_at = unix(info.started_at);
    MachineSession {
        machine_id: host.id.clone(),
        machine_name: host.name.clone(),
        id: info.id,
        agent: info.agent,
        title: info.title,
        cwd: info.cwd,
        status: agent_status(info.state.status),
        doing: Some(info.state.summary).filter(|summary| !summary.is_empty()),
        started_at,
        last_activity: started_at,
        external: false,
        question: info.question.map(agent_question),
    }
}

pub fn external_session(host: &Host, session: ExternalSession) -> MachineSession {
    MachineSession {
        machine_id: host.id.clone(),
        machine_name: host.name.clone(),
        id: session.id,
        agent: session.agent,
        title: session.title,
        cwd: session.cwd,
        status: presence_status(session.presence),
        doing: None,
        started_at: unix(session.started_at),
        last_activity: unix(session.last_activity),
        external: true,
        question: None,
    }
}

/// A session as a project lists it; only Claude Code's outside sessions can be followed live.
pub fn past_session(host: &Host, session: ExternalSession) -> MachineSession {
    let followed = session.agent == "claude";
    let status = match session.presence {
        _ if session.taken_over.is_some() => AgentStatus::Running {},
        _ if !followed => AgentStatus::Ended {},
        Presence::Working => AgentStatus::Working {},
        Presence::Waiting => AgentStatus::Waiting {},
        Presence::Running | Presence::Recent => AgentStatus::Running {},
        _ => AgentStatus::Ended {},
    };
    let external = session.taken_over.is_none();
    MachineSession {
        machine_id: host.id.clone(),
        machine_name: host.name.clone(),
        id: session.taken_over.unwrap_or(session.id),
        agent: session.agent,
        title: session
            .title
            .or_else(|| Some(session.last_prompt).filter(|prompt| !prompt.is_empty())),
        cwd: session.cwd,
        status,
        doing: None,
        started_at: unix(session.started_at),
        last_activity: unix(session.last_activity),
        external,
        question: None,
    }
}

/// A status from a newer node reads as working.
fn agent_status(status: Status) -> AgentStatus {
    match status {
        Status::Starting => AgentStatus::Starting {},
        Status::Asking => AgentStatus::Asking {},
        Status::Idle => AgentStatus::Idle {},
        Status::Ended => AgentStatus::Ended {},
        _ => AgentStatus::Working {},
    }
}

/// Only live presences are listed; any other reads as running.
fn presence_status(presence: Presence) -> AgentStatus {
    match presence {
        Presence::Working => AgentStatus::Working {},
        Presence::Waiting => AgentStatus::Waiting {},
        Presence::Stopped => AgentStatus::Ended {},
        _ => AgentStatus::Running {},
    }
}

fn agent_question(question: Question) -> AgentQuestion {
    AgentQuestion {
        id: question.id,
        text: question.text,
        detail: question.detail,
        options: question.options.into_iter().map(agent_option).collect(),
    }
}

fn agent_option(option: AnswerOption) -> AgentOption {
    AgentOption {
        id: option.id,
        name: option.name,
        allow: matches!(option.kind, OptionKind::AllowOnce | OptionKind::AllowAlways),
    }
}

/// A session Beep runs, in full but for its transcript.
pub fn beep_view(host: &Host, info: SessionInfo) -> AgentSessionView {
    let state = info.state.clone();
    let last_message = info.last_message.clone();
    let turns = info.turns;
    AgentSessionView {
        session: beep_session(host, info),
        plan: state
            .plan
            .into_iter()
            .map(|step| AgentPlanStep {
                content: step.content,
                status: plan_status(step.status),
            })
            .collect(),
        files: state.files_touched,
        last_message,
        error: state.error,
        queued: state.queued,
        turns,
        entries: Vec::new(),
        earlier: 0,
    }
}

/// The background tasks a node reports, as an app reads them.
pub fn session_tasks(tasks: Vec<agent::Task>) -> AgentSessionTasks {
    AgentSessionTasks {
        tasks: tasks
            .into_iter()
            .map(|task| AgentTask {
                id: task.id,
                name: task.name,
                description: task.description,
                state: task.state.as_str().to_owned(),
                can_stop: task.can_stop,
                output_file: task.output_file,
                summary: task.summary,
                entry: task.entry,
            })
            .collect(),
    }
}

/// A status from a newer node reads as pending.
fn plan_status(status: PlanStatus) -> AgentPlanStatus {
    match status {
        PlanStatus::InProgress => AgentPlanStatus::InProgress {},
        PlanStatus::Completed => AgentPlanStatus::Completed {},
        _ => AgentPlanStatus::Pending {},
    }
}

/// The entries an app reads: thoughts only for one that reads them.
pub fn entries_for(entries: Vec<agent::Entry>, thoughts: bool) -> Vec<AgentEntry> {
    entries
        .into_iter()
        .filter(|entry| thoughts || entry.kind != agent::EntryKind::Thought)
        .map(entry)
        .collect()
}

fn entry(entry: agent::Entry) -> AgentEntry {
    let kind = match entry.kind {
        agent::EntryKind::User => AgentEntryKind::User {},
        agent::EntryKind::Agent => AgentEntryKind::Agent {},
        agent::EntryKind::Tool => AgentEntryKind::Tool {},
        agent::EntryKind::Question => AgentEntryKind::Question {},
        agent::EntryKind::Thought => AgentEntryKind::Thought { ms: entry.ms },
        _ => AgentEntryKind::Other {},
    };
    AgentEntry {
        kind,
        text: entry.text,
    }
}

/// What an app calls a capability a machine reports.
pub fn machine_capability(capability: Capability) -> MachineCapability {
    match capability {
        Capability::Gpu => MachineCapability::Gpu {},
        Capability::Cuda => MachineCapability::Cuda {},
        Capability::Blender => MachineCapability::Blender {},
        Capability::Unreal => MachineCapability::Unreal {},
        Capability::Xcode => MachineCapability::Xcode {},
        Capability::Android => MachineCapability::Android {},
        Capability::Docker => MachineCapability::Docker {},
    }
}

/// What the app sees of the models a node found.
pub fn agent_models(list: ModelList) -> AgentModels {
    let pick = match list.pick {
        agent::ModelPick::Listed => AgentModelPick::Listed {},
        agent::ModelPick::Named => AgentModelPick::Named {},
        _ => AgentModelPick::Fixed {},
    };
    AgentModels {
        pick,
        current: list.current,
        models: list
            .models
            .into_iter()
            .map(|model| AgentModel {
                id: model.id,
                name: model.name,
                description: model.description,
            })
            .collect(),
    }
}
