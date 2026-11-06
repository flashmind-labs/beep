//! Coding-agent sessions on the user's machines, for the bar's and the
//! window's agent views: which machines can run one, starting one and
//! reading one in full. `steer` steers it, `media` reads its files.

use crate::{
    agents::work_copy,
    beep::{Beep, Service, epoch_ms},
    center::AgentsShown,
    chat::markdown,
    error::{DesktopError, Result},
    fleet::{Fleet, Machines, SessionParent, agents as direct, extra},
    machines::list::Outside,
    projects::{known, unknown_op},
};
use beep_mesh::Key;
use beep_proto::{
    AgentEntry, AgentEntryKind, AgentHost, AgentMode, AgentModel, AgentModels, AgentQuestion,
    AgentSessionModes, AgentSessionUsage, AgentSessionView, AgentTask, GetAgentSession,
    GetAgentSessionModes, GetAgentSessionTasks, GetAgentSessionUsage, ListAgentHosts,
    ListAgentModels, ListAgentSessions, MachinePick, MachineSession, PreviewAgentMachine,
    ResumeAgentSession, StartAgentSession,
    node::{
        agent::{CheckoutPlan, Effort},
        harness::HarnessConfig,
        mesh_state::StateValue,
    },
};
use serde::Serialize;
use std::{sync::Arc, time::Instant};
use tauri::{AppHandle, Emitter, State, WebviewWindow};
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
    /// The session whose agent started this one.
    parent: Option<SessionParent>,
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
            parent: None,
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
pub async fn agent_hosts(
    app: AppHandle,
    fleet: State<'_, Arc<Fleet>>,
    held: State<'_, Outside>,
) -> Result<Vec<HostRow>> {
    let mesh = fleet.mesh();
    // A harness the desktops switched off is not offered.
    let is_off = |agent: &str| {
        mesh.as_ref()
            .and_then(|mesh| mesh.get(&Key::Harness(agent.to_owned())))
            .and_then(|bytes| HarnessConfig::from_bytes(&bytes).ok())
            .is_some_and(|config| !config.enabled)
    };
    let hosts = fleet.status().agent_hosts(is_off);
    let hosts = held.hosts.merged(
        &app,
        hosts,
        ListAgentHosts {},
        |host| &host.machine_id,
        |old, new| old == new,
    );
    Ok(hosts.into_iter().map(HostRow::from).collect())
}

#[tauri::command]
pub async fn agent_sessions(
    app: AppHandle,
    beep: Service<'_>,
    fleet: Machines<'_>,
    held: State<'_, Outside>,
) -> Result<Vec<SessionRow>> {
    let sessions = machine_sessions(&app, &beep, &fleet, &held).await;
    Ok(sessions
        .into_iter()
        .map(|session| {
            let parent = fleet.parent_of(&session.machine_id, &session.id);
            SessionRow {
                parent,
                ..SessionRow::from(session)
            }
        })
        .collect())
}

/// Whether two listings of a session differ in what the windows draw, which
/// leaves out what moves on every read.
fn same_session(old: &MachineSession, new: &MachineSession) -> bool {
    old.machine_id == new.machine_id
        && old.id == new.id
        && old.status == new.status
        && old.question.as_ref().map(|q| q.id) == new.question.as_ref().map(|q| q.id)
}

/// The sessions of every machine, newest activity first, at once: Beep's on
/// this Mac, read in this process, and the rest as last held. The other
/// machines of the mesh, the server and the scans of what agents ran
/// outside Beep refresh in the background, each machine on its own limit,
/// and the windows redraw when one changes.
pub(crate) async fn machine_sessions(
    app: &AppHandle,
    beep: &Arc<Beep>,
    fleet: &Arc<Fleet>,
    held: &Outside,
) -> Vec<MachineSession> {
    let mut sessions = direct::local_sessions(fleet).await;
    let (b, f) = (beep.clone(), fleet.clone());
    sessions.extend(held.sessions.refreshed(
        app,
        async move {
            let (peers, server) =
                tokio::join!(direct::peer_sessions(&f), b.call(ListAgentSessions {}));
            let mut sessions = peers;
            sessions.extend(
                extra(server)
                    .into_iter()
                    .filter(|session| f.member(&session.machine_id).is_none()),
            );
            Ok(sessions)
        },
        same_session,
    ));
    let f = fleet.clone();
    sessions.extend(held.scans.refreshed(
        app,
        async move { Ok(direct::scanned_sessions(&f).await) },
        same_session,
    ));
    sessions.sort_by_key(|session| std::cmp::Reverse(session.last_activity));
    sessions
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
    fleet: Machines<'_>,
    machine_id: Option<String>,
    agent: Option<String>,
    cwd: Option<String>,
    prompt: String,
    chat: Option<Uuid>,
    model: Option<String>,
    effort: Option<Effort>,
    checkout: Option<CheckoutPlan>,
) -> Result<Started> {
    let request = StartAgentSession {
        machine_id,
        agent,
        cwd,
        prompt,
        needs: Vec::new(),
        chat,
        model,
        effort: effort.map(Into::into),
        checkout: checkout.map(Into::into),
    };
    // A chat that hears the session is the server's: it follows the node.
    let chat_on_server = request.chat.is_some() && beep.has_server();
    let started = if !chat_on_server && goes_direct(&fleet, request.machine_id.as_deref()).await {
        direct::start(&fleet, request).await?
    } else {
        beep.call(request).await?
    };
    Ok(Started {
        session: started.session.into(),
        why: started.why,
    })
}

/// Whether a call about `machine` goes to the mesh: a member of it, or,
/// when none is named, any member that runs agents. Otherwise the server
/// has it.
async fn goes_direct(fleet: &Fleet, machine: Option<&str>) -> bool {
    match machine {
        Some(machine) => fleet.member(machine).is_some(),
        None => !direct::hosts(fleet).is_empty(),
    }
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
    fleet: Machines<'_>,
    agent: Option<String>,
    cwd: Option<String>,
    prompt: String,
) -> Result<Option<PickView>> {
    let request = PreviewAgentMachine {
        agent,
        cwd,
        prompt,
        needs: Vec::new(),
    };
    let asked = if goes_direct(&fleet, None).await {
        direct::preview_machine(&fleet, request).await
    } else {
        beep.call(request).await
    };
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
    fleet: Machines<'_>,
    machine_id: String,
    agent: String,
) -> Result<Option<ModelsView>> {
    let request = ListAgentModels {
        machine_id: machine_id.clone(),
        agent,
    };
    let asked = match fleet.member(&machine_id) {
        Some(machine) => direct::models(&fleet, machine, &request).await,
        None => beep.call(request).await,
    };
    match asked {
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
    fleet: Machines<'_>,
    machine_id: String,
    agent: String,
    session: String,
    prompt: Option<String>,
) -> Result<Started> {
    let request = ResumeAgentSession {
        machine_id,
        agent,
        session,
        prompt,
    };
    let started = match fleet.member(&request.machine_id) {
        Some(machine) => direct::resume(&fleet, machine, request).await?,
        None => beep.call(request).await?,
    };
    Ok(Started {
        session: started.session.into(),
        why: started.why,
    })
}

/// Whether this Mac can ask the machine to delete sessions: it is in the mesh.
#[tauri::command]
pub fn agent_deletable(fleet: Machines<'_>, machine_id: String) -> bool {
    fleet.member(&machine_id).is_some()
}

/// Deletes an ended session's conversation from the machine for good. Only a
/// machine in the mesh can: the node does it, and refuses one that still
/// runs, so the page offers Close first.
#[tauri::command]
pub async fn agent_delete(
    app: AppHandle,
    fleet: Machines<'_>,
    machine_id: String,
    agent: String,
    session: String,
) -> Result<()> {
    let machine = fleet.member(&machine_id).ok_or_else(|| {
        DesktopError::BadRequest("Only a machine in the mesh can delete sessions.".into())
    })?;
    direct::delete_session(&fleet, machine, &agent, &session).await?;
    // The windows read their session lists again.
    if let Err(e) = app.emit("sessions-changed", session) {
        log::warn!("could not tell the windows a session was deleted: {e}");
    }
    Ok(())
}

/// One session in full. The calling webview's session view shows it, so
/// its notices stay quiet while the view keeps reading it. `before` reads
/// the `last` entries before that entry, to load older ones.
// The arguments are the fields of the window's `invoke` call.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn agent_view(
    window: WebviewWindow,
    beep: Service<'_>,
    fleet: Machines<'_>,
    shown: State<'_, AgentsShown>,
    machine_id: String,
    agent: String,
    session: String,
    last: u32,
    before: Option<u64>,
) -> Result<SessionPage> {
    shown.read(
        window.label(),
        &machine_id,
        &agent,
        &session,
        Instant::now(),
    );
    let request = GetAgentSession {
        machine_id,
        agent,
        session,
        last,
        before,
    };
    let view = match fleet.member(&request.machine_id) {
        Some(machine) => direct::session(&fleet, machine, &request).await?,
        None => beep.call(request).await?,
    };
    Ok(view.into())
}

/// The folder on this Mac for a session's folder, to open in an editor; `None`
/// when this Mac does not have it.
#[tauri::command]
pub async fn session_folder_here(
    beep: Service<'_>,
    fleet: Machines<'_>,
    machine_id: String,
    cwd: String,
) -> Result<Option<String>> {
    let folder = work_copy::folder_here(&beep, &fleet, &machine_id, &cwd).await;
    Ok(folder.map(|f| f.to_string_lossy().into_owned()))
}

/// How full a Beep session's context is and what its finished turns
/// used; `None` from a server too old to say.
#[tauri::command]
pub async fn agent_usage(
    beep: Service<'_>,
    fleet: Machines<'_>,
    machine_id: String,
    agent: String,
    session: String,
) -> Result<Option<AgentSessionUsage>> {
    let request = GetAgentSessionUsage {
        machine_id,
        agent,
        session,
    };
    known(match fleet.member(&request.machine_id) {
        Some(machine) => direct::usage(&fleet, machine, &request).await,
        None => beep.call(request).await,
    })
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
    fleet: Machines<'_>,
    machine_id: String,
    agent: String,
    session: String,
) -> Result<Option<ModesRow>> {
    let request = GetAgentSessionModes {
        machine_id,
        agent,
        session,
    };
    let modes = known(match fleet.member(&request.machine_id) {
        Some(machine) => direct::modes(&fleet, machine, &request).await,
        None => beep.call(request).await,
    })?;
    Ok(modes.map(Into::into))
}

/// A background command of a session for the page.
#[derive(Debug, Serialize)]
pub struct TaskRow {
    id: String,
    name: String,
    description: String,
    /// `running`, `paused`, `completed`, `failed` or `stopped`.
    state: String,
    can_stop: bool,
    output_file: Option<String>,
    summary: Option<String>,
    /// The log entry of the call that started it.
    entry: Option<u64>,
}

impl From<AgentTask> for TaskRow {
    fn from(task: AgentTask) -> Self {
        Self {
            id: task.id,
            name: task.name,
            description: task.description,
            state: task.state,
            can_stop: task.can_stop,
            output_file: task.output_file,
            summary: task.summary,
            entry: task.entry,
        }
    }
}

/// The commands a Beep session's agent has in the background, oldest
/// first; none from a server or node too old to say.
#[tauri::command]
pub async fn agent_tasks(
    beep: Service<'_>,
    fleet: Machines<'_>,
    machine_id: String,
    agent: String,
    session: String,
) -> Result<Vec<TaskRow>> {
    let request = GetAgentSessionTasks {
        machine_id,
        agent,
        session,
    };
    let tasks = known(match fleet.member(&request.machine_id) {
        Some(machine) => direct::tasks(&fleet, machine, &request).await,
        None => beep.call(request).await,
    })?;
    Ok(tasks
        .map(|tasks| tasks.tasks.into_iter().map(TaskRow::from).collect())
        .unwrap_or_default())
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
