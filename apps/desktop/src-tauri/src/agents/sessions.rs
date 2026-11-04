//! Coding-agent sessions on the user's machines, for the bar's and the
//! window's agent views: which machines can run one, starting one and
//! reading one in full. `steer` steers it, `media` reads its files.

use crate::{
    agents::work_copy,
    beep::{Service, epoch_ms},
    center::AgentsShown,
    chat::markdown,
    error::Result,
    projects::{known, unknown_op},
};
use beep_proto::{
    AgentEntry, AgentEntryKind, AgentHost, AgentMode, AgentModel, AgentModels, AgentQuestion,
    AgentSessionModes, AgentSessionUsage, AgentSessionView, GetAgentSession, GetAgentSessionModes,
    GetAgentSessionUsage, ListAgentHosts, ListAgentModels, ListAgentSessions, MachinePick,
    MachineSession, PreviewAgentMachine, ResumeAgentSession, StartAgentSession,
    node::agent::{CheckoutPlan, Effort},
};
use serde::Serialize;
use std::time::Instant;
use tauri::{State, WebviewWindow};
use uuid::Uuid;

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
    /// What the agent said or thought, as the chat shows Beep's replies:
    /// its Markdown as cleaned HTML. Only on the agent's own entries and
    /// its thoughts.
    #[serde(skip_serializing_if = "Option::is_none")]
    html: Option<String>,
    /// How long a thought took, in milliseconds, once it ended.
    #[serde(skip_serializing_if = "Option::is_none")]
    ms: Option<u64>,
}

impl From<AgentEntry> for EntryRow {
    fn from(entry: AgentEntry) -> Self {
        // The bar draws the pictures of image lines after the text.
        let (html, ms) = match entry.kind {
            AgentEntryKind::Agent {} => (
                Some(markdown::to_html(&without_image_lines(&entry.text))),
                None,
            ),
            AgentEntryKind::Thought { ms } => (Some(markdown::to_html(&entry.text)), ms),
            _ => (None, None),
        };
        Self {
            kind: entry.kind.into(),
            text: entry.text,
            html,
            ms,
        }
    }
}

/// `text` without its `[image: name]` lines.
fn without_image_lines(text: &str) -> String {
    text.lines()
        .filter(|line| {
            let line = line.trim();
            !(line.starts_with("[image: ") && line.ends_with(']'))
        })
        .collect::<Vec<_>>()
        .join("\n")
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
/// its turn ends and questions; `model`, from `agent_models`, picks the
/// agent's model, `effort` how hard it thinks and `checkout` the new
/// branch or worktree it starts on.
// The arguments are the fields of the window's `invoke` call.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn agent_start(
    beep: Service<'_>,
    machine_id: Option<String>,
    agent: Option<String>,
    cwd: Option<String>,
    prompt: String,
    chat: Option<Uuid>,
    model: Option<String>,
    effort: Option<Effort>,
    checkout: Option<CheckoutPlan>,
) -> Result<Started> {
    let started = beep
        .call(StartAgentSession {
            machine_id,
            agent,
            cwd,
            prompt,
            needs: Vec::new(),
            chat,
            model,
            effort: effort.map(Into::into),
            checkout: checkout.map(Into::into),
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

/// The models an agent can run on a machine, as the start form's Model
/// field lists them.
#[derive(Debug, Serialize)]
pub struct ModelsView {
    /// "listed", "named" or "fixed".
    pick: &'static str,
    current: Option<String>,
    models: Vec<ModelView>,
}

#[derive(Debug, Serialize)]
pub struct ModelView {
    id: String,
    name: String,
    description: Option<String>,
}

impl From<AgentModels> for ModelsView {
    fn from(list: AgentModels) -> Self {
        Self {
            pick: list.pick.into(),
            current: list.current,
            models: list.models.into_iter().map(ModelView::from).collect(),
        }
    }
}

impl From<AgentModel> for ModelView {
    fn from(model: AgentModel) -> Self {
        Self {
            id: model.id,
            name: model.name,
            description: model.description,
        }
    }
}

/// The models `agent` can run on `machine_id`; `None` from a server too
/// old to say, which the form takes as none to pick.
#[tauri::command]
pub async fn agent_models(
    beep: Service<'_>,
    machine_id: String,
    agent: String,
) -> Result<Option<ModelsView>> {
    match beep.call(ListAgentModels { machine_id, agent }).await {
        Ok(list) => Ok(Some(list.into())),
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

/// One session in full. The calling webview's session view shows it, so
/// its notices stay quiet while the view keeps reading it.
#[tauri::command]
pub async fn agent_view(
    window: WebviewWindow,
    beep: Service<'_>,
    shown: State<'_, AgentsShown>,
    machine_id: String,
    agent: String,
    session: String,
    last: u32,
) -> Result<SessionPage> {
    shown.read(
        window.label(),
        &machine_id,
        &agent,
        &session,
        Instant::now(),
    );
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

/// The folder on this Mac for a session's folder, to open in an editor; `None`
/// when this Mac does not have it.
#[tauri::command]
pub async fn session_folder_here(
    beep: Service<'_>,
    machine_id: String,
    cwd: String,
) -> Result<Option<String>> {
    let folder = work_copy::folder_here(&beep, &machine_id, &cwd).await;
    Ok(folder.map(|f| f.to_string_lossy().into_owned()))
}

/// How full a Beep session's context is and what its finished turns
/// used; `None` from a server too old to say.
#[tauri::command]
pub async fn agent_usage(
    beep: Service<'_>,
    machine_id: String,
    agent: String,
    session: String,
) -> Result<Option<AgentSessionUsage>> {
    known(
        beep.call(GetAgentSessionUsage {
            machine_id,
            agent,
            session,
        })
        .await,
    )
}

/// A session's modes for the page.
#[derive(Debug, Serialize)]
pub struct ModesRow {
    current: Option<String>,
    modes: Vec<ModeRow>,
    /// The plan waiting for approval, as cleaned HTML.
    plan: Option<String>,
}

#[derive(Debug, Serialize)]
struct ModeRow {
    id: String,
    name: String,
}

impl From<AgentSessionModes> for ModesRow {
    fn from(modes: AgentSessionModes) -> Self {
        Self {
            current: modes.current,
            modes: modes
                .modes
                .into_iter()
                .map(|AgentMode { id, name }| ModeRow { id, name })
                .collect(),
            plan: modes.plan.as_deref().map(markdown::to_html),
        }
    }
}

/// The modes of a Beep session and the plan it waits on approval of; `None`
/// from a server too old to say.
#[tauri::command]
pub async fn agent_modes(
    beep: Service<'_>,
    machine_id: String,
    agent: String,
    session: String,
) -> Result<Option<ModesRow>> {
    let modes = known(
        beep.call(GetAgentSessionModes {
            machine_id,
            agent,
            session,
        })
        .await,
    )?;
    Ok(modes.map(Into::into))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_lines_stay_out_of_the_agents_html() {
        let said = EntryRow::from(AgentEntry {
            kind: AgentEntryKind::Agent {},
            text: "Saved chart.png\n[image: image-0123456789abcdef]".into(),
        });
        let html = said.html.unwrap();
        assert!(html.contains("Saved chart.png"), "{html}");
        assert!(!html.contains("image-0123"), "{html}");
        assert!(said.text.ends_with("[image: image-0123456789abcdef]"));
    }

    #[test]
    fn what_the_agent_says_comes_as_clean_html() {
        let said = EntryRow::from(AgentEntry {
            kind: AgentEntryKind::Agent {},
            text: "See [sqlx #19](https://example.com/19) and `cargo test`.<script>x()</script>"
                .into(),
        });
        let html = said.html.expect("an agent entry carries its HTML");
        assert!(
            html.contains(r#"<a href="https://example.com/19""#),
            "{html}"
        );
        assert!(html.contains("<code>cargo test</code>"), "{html}");
        assert!(!html.contains("<script"), "{html}");
        let tool = EntryRow::from(AgentEntry {
            kind: AgentEntryKind::Tool {},
            text: "Read file [completed]".into(),
        });
        assert_eq!(tool.html, None);
    }

    #[test]
    fn a_thought_comes_as_html_with_its_time() {
        let thought = EntryRow::from(AgentEntry {
            kind: AgentEntryKind::Thought { ms: Some(12_000) },
            text: "**Reading** the `tests`".into(),
        });
        assert_eq!(thought.kind, "thought");
        assert_eq!(thought.ms, Some(12_000));
        let html = thought.html.expect("a thought carries its HTML");
        assert!(html.contains("<strong>Reading</strong>"), "{html}");
    }
}
