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
            Presence, Question, SessionInfo, State, Status,
        },
    },
};
use futures::future::join_all;
use std::{
    cmp::Reverse,
    collections::{HashMap, HashSet},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokio::time::timeout;

/// How long one request to a node may take.
pub const NODE_LIMIT: Duration = Duration::from_secs(3);
/// The agents whose outside sessions are listed, and how many of each.
const EXTERNAL_AGENTS: [&str; 3] = ["claude", "codex", "flsh"];
const EXTERNAL_LIMIT: u32 = 20;
/// How many of a machine's ended conversations and their threads are listed, the newest.
const CONVERSATIONS_LIMIT: u32 = 100;
/// The page size used when asking each machine for project history.
pub const MAX_PROJECT_SESSIONS: usize = 50;
/// A node slower than this to list a project's sessions is logged.
const SLOW_NODE: Duration = Duration::from_secs(1);

/// A session Beep runs, and the session that started it when an agent did.
#[derive(Debug, Clone)]
pub struct Linked {
    pub session: MachineSession,
    pub parent: Option<ParentLink>,
    /// A temporary reviewer of its parent's changes.
    pub reviewer: bool,
}

/// `beep_sessions`, with who started each: what an app needs to nest a
/// child under its parent.
/// With `reviewers`, also the running reviewers that have a parent.
pub async fn linked_sessions<S: Sessions + ?Sized>(
    node: &S,
    host: &Host,
    reviewers: bool,
) -> Vec<Linked> {
    listed_sessions(node, host, reviewers)
        .await
        .unwrap_or_default()
}

/// `linked_sessions`, or `None` when the node's list failed, so a caller that
/// keeps what it listed before can tell a failure from a machine with none.
pub async fn listed_sessions<S: Sessions + ?Sized>(
    node: &S,
    host: &Host,
    reviewers: bool,
) -> Option<Vec<Linked>> {
    let listed = async {
        if reviewers {
            sessions::list_all(node).await
        } else {
            sessions::list(node).await
        }
    };

    match timeout(NODE_LIMIT, listed).await {
        Ok(Ok(listed)) => Some(
            listed
                .into_iter()
                .map(|info| Linked {
                    parent: info.parent.clone(),
                    reviewer: info.reviewer,
                    session: beep_session(host, info),
                })
                .collect(),
        ),
        Ok(Err(e)) => {
            tracing::warn!("agent sessions not listed; node={} error={e:#}", host.id);
            None
        }
        Err(_) => {
            tracing::warn!("agent sessions not listed in time; node={}", host.id);
            None
        }
    }
}

/// The sessions started outside Beep that run now, from scanning what each
/// agent keeps on the node. A scan takes seconds.
pub async fn outside_sessions<S: Sessions + ?Sized>(node: &S, host: &Host) -> Vec<MachineSession> {
    join_all(EXTERNAL_AGENTS.map(|agent| outside(node, host, agent)))
        .await
        .into_iter()
        .flatten()
        .filter(|session| session.taken_over.is_none() && runs_now(session, SystemTime::now()))
        .map(|session| external_session(host, session))
        .collect()
}

/// The assistant's conversations that no longer run, with their threads: the
/// newest sessions agents ran under the conversations' folder in the node's
/// `home`. The node lists only the sessions it holds and `outside_sessions`
/// only those that run, so one that ended is here once its node started
/// again. A scan takes seconds.
pub async fn ended_conversations<S: Sessions + ?Sized>(
    node: &S,
    host: &Host,
    home: &str,
) -> Vec<MachineSession> {
    let folders = vec![conversations_folder(home)];

    match timeout(
        NODE_LIMIT,
        sessions::in_folders(node, folders, CONVERSATIONS_LIMIT),
    )
    .await
    {
        Ok(Ok(found)) => found
            .into_iter()
            .filter(|session| session.taken_over.is_none() && !runs_now(session, SystemTime::now()))
            .map(|session| past_session(host, session))
            .collect(),
        Ok(Err(e)) => {
            tracing::debug!(
                "ended conversations not listed; node={} error={e:#}",
                host.id
            );
            Vec::new()
        }
        Err(_) => {
            tracing::debug!("ended conversations not listed in time; node={}", host.id);
            Vec::new()
        }
    }
}

/// The folder in `home` that holds each assistant conversation's own, as the
/// node makes them, with the separator `home` uses.
fn conversations_folder(home: &str) -> String {
    let separator = if home.contains('\\') { '\\' } else { '/' };
    format!(
        "{}{separator}.beep{separator}assistant",
        home.trim_end_matches(separator)
    )
}

/// The sessions of `agent` started outside Beep; none, with a line in the
/// log, when the node cannot list them in time.
async fn outside<S: Sessions + ?Sized>(node: &S, host: &Host, agent: &str) -> Vec<ExternalSession> {
    match timeout(
        NODE_LIMIT,
        sessions::external(node, agent, EXTERNAL_LIMIT, None),
    )
    .await
    {
        Ok(Ok(sessions)) => sessions,
        Ok(Err(e)) => {
            tracing::debug!(
                "{agent} sessions outside Beep not listed; node={} error={e:#}",
                host.id
            );
            Vec::new()
        }
        Err(_) => {
            tracing::debug!(
                "{agent} sessions outside Beep not listed in time; node={}",
                host.id
            );
            Vec::new()
        }
    }
}

/// The sessions coding agents ran in `folders` on one node, as a project lists them.
pub async fn project_sessions<S: Sessions + ?Sized>(
    node: &S,
    host: &Host,
    folders: Vec<String>,
) -> Vec<MachineSession> {
    let started = Instant::now();
    let (past, owned) = tokio::join!(
        project_history_rows(node, host, folders),
        linked_sessions(node, host, true),
    );

    if started.elapsed() >= SLOW_NODE {
        tracing::info!(
            "project sessions listed slowly; node={} ms={}",
            host.id,
            started.elapsed().as_millis()
        );
    }

    merge_owned(past.unwrap_or_default(), owned)
}

/// What a node's history holds of the sessions that ran in `folders`, as past sessions: the
/// ones Beep owns are not told from the rest, which `merge_owned` does. `None` when the node's
/// history could not be read, so a caller keeps what it had.
pub async fn project_history_rows<S: Sessions + ?Sized>(
    node: &S,
    host: &Host,
    folders: Vec<String>,
) -> Option<Vec<MachineSession>> {
    let limit = u32::try_from(MAX_PROJECT_SESSIONS).unwrap_or(u32::MAX);

    match project_metadata(node, folders, limit).await {
        Ok(found) => Some(
            found
                .into_iter()
                .map(|session| past_session(host, session))
                .collect(),
        ),
        Err(e) => {
            tracing::info!("project sessions not listed; node={} error={e:#}", host.id);
            None
        }
    }
}

/// `past`, a node's history rows, with the sessions Beep owns: a row of an owned session is
/// replaced by the owned one of the same machine, which knows its state now; one with none
/// shows as ended. Background children and reviewers are left out.
pub fn merge_owned(past: Vec<MachineSession>, owned: Vec<Linked>) -> Vec<MachineSession> {
    // `owned` may hold several machines' sessions, and a moved session keeps its agent id.
    let key = |session: &MachineSession| {
        (
            session.machine_id.clone(),
            session.agent.clone(),
            session.id.clone(),
        )
    };
    let children: HashSet<_> = owned
        .iter()
        .filter(|linked| {
            linked.parent.as_ref().is_some_and(|link| link.background) || linked.reviewer
        })
        .map(|linked| key(&linked.session))
        .collect();
    let mut owned: HashMap<_, _> = owned
        .into_iter()
        .map(|linked| (key(&linked.session), linked.session))
        .collect();

    past.into_iter()
        .filter(|past| !children.contains(&key(past)))
        .map(|mut past| {
            // A transcript names the session that took it over, but cannot say
            // whether its turn is working, idle, asking or ended now.
            if past.external {
                return past;
            }

            owned.remove(&key(&past)).unwrap_or_else(|| {
                // No owned row says it runs, so it does not show as running.
                past.status = AgentStatus::Ended {};
                past
            })
        })
        .collect()
}

/// Read bounded metadata pages. Old nodes still answer their original list.
async fn project_metadata<S: Sessions + ?Sized>(
    node: &S,
    folders: Vec<String>,
    limit: u32,
) -> Result<Vec<ExternalSession>, crate::MeshError> {
    let mut before = None;
    let mut found = Vec::new();
    let mut seen = HashSet::new();
    loop {
        let page = sessions::in_folders_page(node, folders.clone(), limit, before.clone()).await?;
        let Some(page) = page else {
            if before.is_none() {
                return sessions::in_folders(node, folders, limit).await;
            }
            return Err(super::said(
                "the node stopped answering session history pages",
            ));
        };
        for session in page.sessions {
            if seen.insert((session.agent.clone(), session.id.clone())) {
                found.push(session);
            }
        }
        match page.next {
            None => return Ok(found),
            Some(next) => {
                if let Some(previous) = &before
                    && (Reverse(next.last_activity), &next.agent, &next.id)
                        <= (
                            Reverse(previous.last_activity),
                            &previous.agent,
                            &previous.id,
                        )
                {
                    return Err(super::said("the session history cursor did not advance"));
                }
                before = Some(next);
            }
        }
    }
}

/// Combines the machines' project histories, newest first. Each machine's
/// list has been paged independently, so quieter machines are kept too.
pub fn project_history(sessions: impl IntoIterator<Item = MachineSession>) -> Vec<MachineSession> {
    let mut sessions: Vec<_> = sessions.into_iter().collect();
    sessions.sort_by_key(|session| Reverse(session.last_activity));
    sessions
}

/// The machine a session runs on.
#[derive(Clone)]
pub struct Host {
    pub id: String,
    pub name: String,
}

/// How long a session waiting for the user stays among those running now
/// after its last entry.
pub const IDLE_SHOWN: Duration = Duration::from_secs(24 * 60 * 60);

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

/// A session Beep started; its `last_activity` is when it last ran, or its
/// start for a node that does not say.
pub fn beep_session(host: &Host, info: SessionInfo) -> MachineSession {
    let started_at = unix(info.started_at);
    let reply = info.reply();
    MachineSession {
        machine_id: host.id.clone(),
        machine_name: host.name.clone(),
        id: info.id,
        agent: info.agent,
        title: info.title,
        cwd: info.cwd,
        status: agent_status(&info.state),
        subagents: info.state.subagents,
        doing: Some(info.state.summary).filter(|summary| !summary.is_empty()),
        started_at,
        last_activity: unix(info.last_activity.max(info.started_at)),
        external: false,
        question: info.question.map(agent_question),
        model: info.model,
        assistant: info.assistant,
        reply,
        changes: info.changes,
        account: info.account,
        persona: info.persona,
        place: info.place,
        continues: info.continues,
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
        subagents: 0,
        model: None,
        assistant: session.assistant,
        reply: None,
        changes: None,
        account: None,
        persona: session.persona,
        place: None,
        continues: session.continues,
    }
}

/// Native reports override cached scans, including an explicit close.
/// Sessions that Beep owns still come from its ordinary session list.
pub fn apply_presence(
    host: &Host,
    presence: &[ExternalSession],
    sessions: &mut Vec<MachineSession>,
) {
    for native in presence {
        let index = sessions.iter().position(|session| {
            session.machine_id == host.id
                && session.id == native.id
                && session.agent == native.agent
        });
        let shown = native.presence.is_live() && native.taken_over.is_none();

        if let Some(index) = index {
            let previous = &mut sessions[index];

            if !previous.external {
                continue;
            }

            if !shown {
                sessions.remove(index);
                continue;
            }

            previous.status = presence_status(native.presence);
            previous.cwd.clone_from(&native.cwd);

            if native.title.is_some() {
                previous.title.clone_from(&native.title);
            }

            previous.last_activity = previous.last_activity.max(unix(native.last_activity));
            continue;
        }

        if shown {
            sessions.push(external_session(host, native.clone()));
        }
    }
}

/// A session as a project lists it, with the same outside presence as Running.
pub fn past_session(host: &Host, session: ExternalSession) -> MachineSession {
    let status = if session.taken_over.is_some() {
        AgentStatus::Running {}
    } else if runs_now(&session, SystemTime::now()) {
        presence_status(session.presence)
    } else {
        AgentStatus::Ended {}
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
        subagents: 0,
        model: None,
        assistant: session.assistant,
        reply: None,
        changes: None,
        account: None,
        persona: session.persona,
        place: None,
        continues: session.continues,
    }
}

/// A status from a newer node reads as working, and so does an idle
/// session whose subagents still run.
fn agent_status(state: &State) -> AgentStatus {
    match state.status {
        Status::Starting => AgentStatus::Starting {},
        Status::Asking => AgentStatus::Asking {},
        Status::Idle if state.subagents == 0 => AgentStatus::Idle {},
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
    let model = info.model.as_ref().map(ToString::to_string);
    AgentSessionView {
        model,
        usage: Some((&info).into()),
        session: beep_session(host, info),
        plan: state
            .plan
            .into_iter()
            .map(|step| AgentPlanStep {
                content: step.content,
                status: plan_status(step.status),
                started_at: step.started_at,
                ended_at: step.ended_at,
            })
            .collect(),
        files: state.files_touched,
        last_message,
        error: state.error,
        queued: state.queued,
        queued_messages: state.queued_messages,
        turns,
        entries: Vec::new(),
        earlier: 0,
        last_stop: state.last_stop,
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
                state: task.state.as_ref().to_owned(),
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
        // Released apps already draw compaction boundaries as notes.
        agent::EntryKind::Compacted => AgentEntryKind::Other {},
        _ => AgentEntryKind::Other {},
    };
    AgentEntry {
        kind,
        text: entry.text,
        tool: entry.tool,
        sent_at: entry.sent_at,
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
                context_window: model.context_window,
                max_context_window: model.max_context_window,
                provider: model.provider,
            })
            .collect(),
    }
}

#[cfg(test)]
#[path = "../tests/machine_listing.rs"]
mod tests;
