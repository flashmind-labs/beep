//! Coding-agent sessions on the user's machines, for the bar's and the
//! window's agent views: which machines can run one, starting one, reading
//! one in full and steering it. Answering a question goes through
//! `center_answer_agent`.

use crate::{
    beep::{Beep, epoch_ms},
    error::Result,
};
use beep_proto::{
    AgentEntry, AgentHost, AgentQuestion, AgentSessionView, AgentSteer, GetAgentSession,
    ListAgentHosts, ListAgentSessions, MachineSession, StartAgentSession, SteerAgentSession,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::State;
use uuid::Uuid;

type Service<'a> = State<'a, Arc<Beep>>;

/// A machine that can run a coding agent.
#[derive(Debug, Serialize)]
pub struct HostRow {
    machine_id: String,
    machine_name: String,
    agents: Vec<String>,
    workspace: Option<String>,
    home: Option<String>,
    capabilities: Vec<&'static str>,
}

impl From<AgentHost> for HostRow {
    fn from(host: AgentHost) -> Self {
        Self {
            machine_id: host.machine_id,
            machine_name: host.machine_name,
            agents: host.agents,
            workspace: host.workspace,
            home: host.home,
            capabilities: host
                .capabilities
                .into_iter()
                .map(<&'static str>::from)
                .collect(),
        }
    }
}

/// One session as the views list it.
#[derive(Debug, Serialize)]
pub struct SessionRow {
    machine_id: String,
    machine_name: String,
    id: String,
    agent: String,
    title: Option<String>,
    cwd: String,
    status: &'static str,
    doing: Option<String>,
    /// In milliseconds since the Unix epoch.
    started_at: u64,
    last_activity: u64,
    external: bool,
    question: Option<AgentQuestion>,
}

impl From<MachineSession> for SessionRow {
    fn from(session: MachineSession) -> Self {
        Self {
            status: session.status.into(),
            started_at: epoch_ms(session.started_at),
            last_activity: epoch_ms(session.last_activity),
            machine_id: session.machine_id,
            machine_name: session.machine_name,
            id: session.id,
            agent: session.agent,
            title: session.title,
            cwd: session.cwd,
            doing: session.doing,
            external: session.external,
            question: session.question,
        }
    }
}

#[derive(Debug, Serialize)]
struct StepRow {
    content: String,
    status: &'static str,
}

#[derive(Debug, Serialize)]
struct EntryRow {
    kind: &'static str,
    text: String,
}

impl From<AgentEntry> for EntryRow {
    fn from(entry: AgentEntry) -> Self {
        Self {
            kind: entry.kind.into(),
            text: entry.text,
        }
    }
}

/// One session in full.
#[derive(Debug, Serialize)]
pub struct SessionPage {
    session: SessionRow,
    plan: Vec<StepRow>,
    files: Vec<String>,
    last_message: String,
    error: Option<String>,
    queued: u32,
    turns: u32,
    entries: Vec<EntryRow>,
    earlier: u32,
}

impl From<AgentSessionView> for SessionPage {
    fn from(view: AgentSessionView) -> Self {
        Self {
            session: view.session.into(),
            plan: view
                .plan
                .into_iter()
                .map(|step| StepRow {
                    content: step.content,
                    status: step.status.into(),
                })
                .collect(),
            files: view.files,
            last_message: view.last_message,
            error: view.error,
            queued: view.queued,
            turns: view.turns,
            entries: view.entries.into_iter().map(EntryRow::from).collect(),
            earlier: view.earlier,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Started {
    session: SessionRow,
    why: String,
}

/// What `agent_steer` does to a session, as the page sends it.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Steer {
    Prompt { text: String },
    Cancel,
    Close,
}

impl From<Steer> for AgentSteer {
    fn from(steer: Steer) -> Self {
        match steer {
            Steer::Prompt { text } => Self::Prompt { text },
            Steer::Cancel => Self::Cancel {},
            Steer::Close => Self::Close {},
        }
    }
}

#[tauri::command]
pub async fn agent_hosts(beep: Service<'_>) -> Result<Vec<HostRow>> {
    let hosts = beep.call(ListAgentHosts {}).await?;
    Ok(hosts.into_iter().map(HostRow::from).collect())
}

#[tauri::command]
pub async fn agent_sessions(beep: Service<'_>) -> Result<Vec<SessionRow>> {
    let sessions = beep.call(ListAgentSessions {}).await?;
    Ok(sessions.into_iter().map(SessionRow::from).collect())
}

/// Starts a session; no `machine_id` lets the server pick. `chat` hears
/// its turn ends and questions.
#[tauri::command]
pub async fn agent_start(
    beep: Service<'_>,
    machine_id: Option<String>,
    agent: Option<String>,
    cwd: Option<String>,
    prompt: String,
    chat: Option<Uuid>,
) -> Result<Started> {
    let started = beep
        .call(StartAgentSession {
            machine_id,
            agent,
            cwd,
            prompt,
            needs: Vec::new(),
            chat,
        })
        .await?;
    Ok(Started {
        session: started.session.into(),
        why: started.why,
    })
}

#[tauri::command]
pub async fn agent_view(
    beep: Service<'_>,
    machine_id: String,
    agent: String,
    session: String,
    last: u32,
) -> Result<SessionPage> {
    let view = beep
        .call(GetAgentSession {
            machine_id,
            agent,
            session,
            last,
        })
        .await?;
    Ok(view.into())
}

#[tauri::command]
pub async fn agent_steer(
    beep: Service<'_>,
    machine_id: String,
    agent: String,
    session: String,
    steer: Steer,
) -> Result<()> {
    beep.call(SteerAgentSession {
        machine_id,
        agent,
        session,
        steer: steer.into(),
    })
    .await
}
