//! Coding-agent sessions on the user's machines, for the bar's and the
//! window's agent views: which machines can run one, starting one, reading
//! one in full and steering it. Answering a question goes through
//! `center_answer_agent`.

use crate::{
    beep::{Beep, epoch_ms},
    error::{DesktopError, Result},
    projects::unknown_op,
};
use base64::{Engine, engine::general_purpose::STANDARD};
use beep_proto::{
    AgentEntry, AgentHost, AgentQuestion, AgentSessionView, AgentSteer, ApiError, GetAgentSession,
    ListAgentHosts, ListAgentSessions, MachinePick, MachineSession, PreviewAgentMachine,
    ResumeAgentSession, StartAgentSession, SteerAgentSession, Upload, check_agent_images,
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
    Prompt {
        text: String,
        /// Pasted or dropped images, sent after the text.
        #[serde(default)]
        images: Vec<PastedImage>,
    },
    Cancel,
    Close,
}

/// An image as the page sends it: its bytes in base64.
#[derive(Debug, Deserialize)]
pub struct PastedImage {
    name: String,
    mime: String,
    data: String,
}

impl TryFrom<Steer> for AgentSteer {
    type Error = DesktopError;

    /// Images go as `PromptImages`, checked against its limits here first;
    /// a message without them goes as `Prompt`, which every server reads.
    fn try_from(steer: Steer) -> Result<Self> {
        Ok(match steer {
            Steer::Prompt { text, images } if images.is_empty() => Self::Prompt { text },
            Steer::Prompt { text, images } => {
                let images = images
                    .into_iter()
                    .map(|image| -> Result<Upload> {
                        let data = STANDARD.decode(&image.data).map_err(|_| {
                            DesktopError::BadRequest(format!("{} could not be read", image.name))
                        })?;
                        Ok(Upload {
                            name: image.name,
                            mime: image.mime,
                            data,
                        })
                    })
                    .collect::<Result<Vec<Upload>>>()?;
                check_agent_images(&images)?;
                Self::PromptImages { text, images }
            }
            Steer::Cancel => Self::Cancel {},
            Steer::Close => Self::Close {},
        })
    }
}

/// True when the server did not know `PromptImages`: one from before it
/// reads op 3 of `AgentSteer` as unknown.
fn older_than_images(error: &DesktopError) -> bool {
    matches!(error, DesktopError::Api(ApiError::BadRequest { message }) if message.contains("unknown op 3"))
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

/// The machine Auto would pick, as the start form shows it.
#[derive(Debug, Serialize)]
pub struct PickView {
    machine_id: String,
    machine_name: String,
    why: String,
}

impl From<MachinePick> for PickView {
    fn from(pick: MachinePick) -> Self {
        Self {
            machine_id: pick.machine_id,
            machine_name: pick.machine_name,
            why: pick.why,
        }
    }
}

/// Where Auto would start the form's session now; `None` from a server
/// too old to say. A start no machine fits is an error saying why.
#[tauri::command]
pub async fn agent_preview(
    beep: Service<'_>,
    agent: Option<String>,
    cwd: Option<String>,
    prompt: String,
) -> Result<Option<PickView>> {
    let asked = beep
        .call(PreviewAgentMachine {
            agent,
            cwd,
            prompt,
            needs: Vec::new(),
        })
        .await;
    match asked {
        Ok(pick) => Ok(Some(pick.into())),
        Err(e) if unknown_op(&e) => Ok(None),
        Err(e) => Err(e),
    }
}

/// Resumes a session that ran outside Beep, as a project lists it; it then
/// runs as one Beep started.
#[tauri::command]
pub async fn agent_resume(
    beep: Service<'_>,
    machine_id: String,
    agent: String,
    session: String,
    prompt: Option<String>,
) -> Result<Started> {
    let started = beep
        .call(ResumeAgentSession {
            machine_id,
            agent,
            session,
            prompt,
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
    let steer = AgentSteer::try_from(steer)?;
    let images = matches!(steer, AgentSteer::PromptImages { .. });
    let sent = beep
        .call(SteerAgentSession {
            machine_id,
            agent,
            session,
            steer,
        })
        .await;
    match sent {
        Err(e) if images && older_than_images(&e) => Err(DesktopError::BadRequest(
            "This Beep server does not take images yet. Send the message without them.".into(),
        )),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pasted(data: &[u8]) -> PastedImage {
        PastedImage {
            name: "shot.png".into(),
            mime: "image/png".into(),
            data: STANDARD.encode(data),
        }
    }

    #[test]
    fn a_message_without_images_goes_as_text() {
        let steer = Steer::Prompt {
            text: "hi".into(),
            images: Vec::new(),
        };
        assert_eq!(
            AgentSteer::try_from(steer).unwrap(),
            AgentSteer::Prompt { text: "hi".into() }
        );
    }

    #[test]
    fn images_go_as_prompt_images_within_the_limits() {
        let png = b"\x89PNG\r\n\x1a\n".to_vec();
        let steer = Steer::Prompt {
            text: "look".into(),
            images: vec![pasted(&png)],
        };
        let AgentSteer::PromptImages { images, .. } = AgentSteer::try_from(steer).unwrap() else {
            panic!("an image goes as PromptImages");
        };
        assert_eq!(images[0].data, png);
        let six = Steer::Prompt {
            text: "look".into(),
            images: (0..6).map(|_| pasted(&png)).collect(),
        };
        assert!(AgentSteer::try_from(six).is_err());
    }

    #[test]
    fn an_old_server_is_told_apart() {
        let old = DesktopError::Api(ApiError::bad_request("unknown op 3"));
        assert!(older_than_images(&old));
        assert!(!older_than_images(&DesktopError::Api(
            ApiError::bad_request("The message is empty.")
        )));
    }
}
