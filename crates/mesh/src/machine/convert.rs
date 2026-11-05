//! What a node's replies look like to an app.

use beep_proto::{
    AgentEntry, AgentEntryKind, AgentModel, AgentModelPick, AgentModels, AgentOption,
    AgentPlanStatus, AgentPlanStep, AgentQuestion, AgentSessionTasks, AgentSessionView,
    AgentStatus, AgentTask, MachineCapability, MachineSession,
    node::{
        Capability,
        agent::{
            self, AnswerOption, ExternalSession, ModelList, OptionKind, PlanStatus, Presence,
            Question, SessionInfo, Status,
        },
    },
};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

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
pub fn runs_now(session: &ExternalSession, now: SystemTime) -> bool {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_an_app_that_reads_thoughts_gets_them() {
        let entries = vec![
            agent::Entry::new(agent::EntryKind::User, "go".into()),
            agent::Entry {
                kind: agent::EntryKind::Thought,
                text: "hmm".into(),
                ms: Some(4000),
            },
            agent::Entry::new(agent::EntryKind::Agent, "done".into()),
        ];
        let kinds = |thoughts| -> Vec<AgentEntryKind> {
            entries_for(entries.clone(), thoughts)
                .into_iter()
                .map(|entry| entry.kind)
                .collect()
        };
        assert_eq!(kinds(true).len(), 3);
        assert_eq!(
            kinds(false),
            [AgentEntryKind::User {}, AgentEntryKind::Agent {}]
        );
    }

    #[test]
    fn a_recent_outside_session_offers_resume_unless_it_can_be_followed() {
        let host = Host {
            id: "m".into(),
            name: "build-box".into(),
        };
        let codex = ExternalSession {
            id: "s".into(),
            agent: "codex".into(),
            presence: Presence::Recent,
            ..ExternalSession::default()
        };
        assert_eq!(
            past_session(&host, codex.clone()).status,
            AgentStatus::Ended {}
        );
        let claude = ExternalSession {
            agent: "claude".into(),
            ..codex
        };
        assert_eq!(past_session(&host, claude).status, AgentStatus::Running {});
    }
}
