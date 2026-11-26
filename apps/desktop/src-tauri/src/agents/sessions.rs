//! Coding-agent sessions on the user's machines, for the bar's and the
//! window's agent views: which machines can run one, starting one and
//! reading one in full. `steer` steers it, `media` reads its files.

use crate::{
    agents::{
        kept_folder::PastedImage,
        overlay::{overlaid, overlaid_children},
        shown::AgentsShown,
        starts::{Start, Started, Starts},
        steering::{DevicePick, uploads},
    },
    error::{DesktopError, Result},
    fleet::{
        Fleet, Machines, agents as direct,
        asked::{BEFORE_SET_DEVICE, ORDER_LIMIT, READ_LIMIT, SMALL_LIMIT, newer_than},
        node_actor::{FAST_JOB_WAIT, JobKind},
        projects::{FleetState, desk},
        session_lists::{SessionLists, SessionRow},
        session_mirror::{SessionMirror, StartId},
        work_copy,
    },
    markdown,
    projects::known,
};
use beep_node::agents::tags::own_words;
use beep_proto::SessionId;
use beep_proto::node::mesh_state::SessionMove;
use beep_proto::{
    AccountName, AgentEntry, AgentEntryKind, AgentMode, AgentModels, AgentPlanStep,
    AgentSessionModes, AgentSessionUsage, AgentSessionView, AgentTask, AgentToolCall, ApiError,
    GetAgentSession, GetAgentSessionModes, GetAgentSessionTasks, ListAgentModels, ModelId,
    ResumeAgentSession, StartAgentSession,
    node::agent::{CheckoutPlan, Effort, StopReason},
};
use iroh::EndpointId;
use serde::Serialize;
use std::{str::FromStr, sync::Arc, time::Instant};
use tauri::{AppHandle, Emitter, Manager, State, WebviewWindow};
use tokio::sync::broadcast;

#[derive(Debug, Serialize)]
struct EntryRow {
    kind: &'static str,
    text: String,
    /// What the agent said or thought, as the chat shows Beep's replies:
    /// its Markdown as cleaned HTML. On a user entry, the user's own words
    /// as Markdown with raw HTML left as text. Not on tools and plans.
    #[serde(skip_serializing_if = "Option::is_none")]
    html: Option<String>,
    /// Cleaned Markdown for child parts of a user entry, in source order.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    child_html: Vec<String>,
    /// How long a thought took, in milliseconds, once it ended.
    #[serde(skip_serializing_if = "Option::is_none")]
    ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool: Option<AgentToolCall>,
    sent_at: Option<u64>,
}

impl From<AgentEntry> for EntryRow {
    fn from(entry: AgentEntry) -> Self {
        let child_html = if matches!(entry.kind, AgentEntryKind::User {}) {
            super::child_reply::replies(&entry.text)
        } else {
            Vec::new()
        };
        // The bar draws the pictures of image lines after the text.
        let (html, ms) = match entry.kind {
            AgentEntryKind::Agent {} => (
                Some(markdown::to_html(&without_image_lines(&entry.text))),
                None,
            ),
            AgentEntryKind::Thought { ms } => (Some(markdown::to_html(&entry.text)), ms),
            AgentEntryKind::User {} => (
                Some(markdown::user_html(&without_image_lines(&own_words(
                    &entry.text,
                )))),
                None,
            ),
            _ => (None, None),
        };
        Self {
            kind: entry.kind.into(),
            text: entry.text,
            html,
            child_html,
            ms,
            tool: entry.tool,
            sent_at: entry.sent_at,
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
    model: Option<String>,
    session: SessionRow,
    plan: Vec<AgentPlanStep>,
    error: Option<String>,
    queued: u32,
    queued_messages: Vec<String>,
    turns: u32,
    /// Why its last turn ended; none before the first.
    last_stop: Option<StopReason>,
    entries: Vec<EntryRow>,
    earlier: u32,
    usage: Option<AgentSessionUsage>,
}

impl From<AgentSessionView> for SessionPage {
    fn from(view: AgentSessionView) -> Self {
        Self {
            model: view.model,
            session: SessionRow::from(view.session),
            plan: view.plan,
            error: view.error,
            queued: view.queued,
            queued_messages: view.queued_messages,
            turns: view.turns,
            last_stop: view.last_stop,
            entries: view.entries.into_iter().map(EntryRow::from).collect(),
            earlier: view.earlier,
            usage: view.usage,
        }
    }
}

#[tauri::command]
pub async fn agent_sessions(app: AppHandle, fleet: Machines<'_>) -> Result<Vec<SessionRow>> {
    let rows = app
        .state::<Arc<SessionMirror>>()
        .rows_of_launch(&fleet)
        .await;
    let sessions = overlaid(&app, &fleet, &rows);
    let children = overlaid_children(&fleet, &rows);

    Ok(sessions
        .into_iter()
        .map(SessionRow::from)
        .chain(children.into_iter().map(|(session, link)| SessionRow {
            parent: link.parent(&session.machine_id),
            reviewer: link.reviewer,
            background: link.background(),
            ..SessionRow::from(session)
        }))
        .collect())
}

/// Saves the user's title independently of the agent's generated title.
#[tauri::command]
pub async fn agent_title_save(
    fleet: Machines<'_>,
    machine_id: String,
    agent: String,
    session: SessionId,
    title: String,
) -> Result<()> {
    let mesh = fleet.ready().await?;
    // `follow_titles` hears the write and tells the windows.
    beep_fleet::session_titles::save(&mesh, &machine_id, &agent, &session, &title).await?;
    Ok(())
}

/// Tells the windows of each title saved here or on another app, with the
/// session and the title it shows (the agent's own once the user's is cleared), so an open
/// session retitles in place.
pub(crate) async fn follow_titles(app: AppHandle, fleet: Arc<Fleet>) {
    let mesh = match fleet.ready().await {
        Ok(mesh) => mesh,
        Err(e) => {
            log::warn!("session titles saved elsewhere are not followed: {e}");
            return;
        }
    };
    let mut changes = mesh.changes();

    loop {
        let key = match changes.recv().await {
            Ok(key) => key,
            Err(broadcast::error::RecvError::Lagged(_)) => continue,
            Err(broadcast::error::RecvError::Closed) => return,
        };
        let Some(mut retitled) = beep_fleet::session_titles::changed(&mesh, &key) else {
            continue;
        };

        // A cleared title shows the agent's own, which the mirror's rows hold.
        if retitled.title.is_none() {
            let rows = app.state::<Arc<SessionMirror>>().current();
            retitled.title = rows
                .iter_of(None)
                .find(|row| {
                    row.machine_id == retitled.machine_id
                        && row.agent == retitled.agent
                        && row.id == retitled.session
                })
                .and_then(|row| row.title.clone());
        }

        if let Err(e) = app.emit("agent-title-changed", retitled) {
            log::warn!("could not tell the windows a session title changed: {e}");
        }
    }
}

/// Starts a session; no `machine_id` lets the mesh pick. `model`, from
/// `agent_models`, picks the agent's model, `effort` how hard it thinks and
/// `checkout` the new branch or worktree it starts on. `images` go with the
/// prompt. `assistant` starts the home assistant.
// The arguments are the fields of the window's `invoke` call.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn agent_start(
    fleet: Machines<'_>,
    machine_id: Option<String>,
    agent: Option<String>,
    cwd: Option<String>,
    prompt: String,
    model: Option<String>,
    effort: Option<Effort>,
    fast: Option<bool>,
    checkout: Option<CheckoutPlan>,
    images: Option<Vec<PastedImage>>,
    assistant: Option<bool>,
    account: Option<String>,
) -> Result<Started> {
    let images = uploads(images.unwrap_or_default())?;
    let account = account.as_deref().map(AccountName::from_str).transpose()?;

    if let Some(machine) = &machine_id {
        fleet.machine(machine)?;
    }

    let request = StartAgentSession {
        machine_id,
        agent,
        cwd,
        prompt,
        needs: Vec::new(),
        model,
        effort,
        fast,
        checkout,
        assistant: assistant.unwrap_or(false),
        account,
        continues: None,
    };
    let started = direct::start(&fleet, request, images).await?;
    Ok(Started {
        session: started.session.into(),
        why: started.why,
    })
}

/// The models `agent` can run on `machine_id`; failures retain their context
/// for the picker and the local log.
#[tauri::command]
pub async fn agent_models(
    fleet: Machines<'_>,
    machine_id: String,
    agent: String,
) -> Result<Option<AgentModels>> {
    let request = ListAgentModels {
        machine_id: machine_id.clone(),
        agent: agent.clone(),
    };
    let machine = fleet.machine(&machine_id)?;
    let fleet_arc = fleet.inner().clone();
    let key = (machine, agent.clone());
    let fetched = fleet
        .projections()
        .models
        .read(
            key,
            PartialEq::eq,
            || {},
            move || async move { direct::models(&fleet_arc, machine, &request).await },
        )
        .await;

    match fetched {
        Ok(list) => Ok(Some(list)),
        Err(e) => {
            log::warn!("model lookup failed: machine={machine_id} harness={agent}: {e:#}");
            Err(e)
        }
    }
}

/// Resumes a session that ran outside Beep, as a project lists it; it then
/// runs as one Beep started.
#[tauri::command]
pub async fn agent_resume(
    fleet: Machines<'_>,
    machine_id: String,
    agent: String,
    session: SessionId,
    prompt: Option<String>,
    model: Option<ModelId>,
) -> Result<Started> {
    let request = ResumeAgentSession {
        machine_id,
        agent,
        session,
        prompt,
        model,
    };
    let machine = fleet.machine(&request.machine_id)?;
    resume_session(&fleet, machine, request).await
}

/// Resumes the session `request` names on `machine`, which answers with the
/// session that goes on from it.
pub(crate) async fn resume_session(
    fleet: &Fleet,
    machine: EndpointId,
    request: ResumeAgentSession,
) -> Result<Started> {
    let session = request.session.clone();
    let resume = direct::resume(fleet, machine, request);
    let started = fleet
        .asked(
            machine,
            &session,
            "resuming the session",
            ORDER_LIMIT,
            resume,
        )
        .await?;

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
    session: SessionId,
) -> Result<()> {
    let machine = fleet.member(&machine_id).ok_or_else(|| {
        DesktopError::BadRequest("Only a machine in the mesh can delete sessions.".into())
    })?;
    let fleet_arc = fleet.inner().clone();
    fleet
        .node_ops()
        .submit(
            Some(app.clone()),
            fleet_arc,
            machine,
            JobKind::DeleteSession {
                agent,
                session: session.clone(),
            },
            FAST_JOB_WAIT,
        )
        .await?;

    app.state::<Arc<SessionMirror>>()
        .deleted(&machine_id, &session);
    fleet.projections().forget_session(machine, &session);
    app.state::<SessionLists>().forget(machine, session);

    Ok(())
}

/// Why a session could not be read, which the session view tells apart:
/// `{ kind: "lost" }`, `{ kind: "gone" }` or `{ kind: "failed", message }`.
#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ViewError {
    /// The machine could not be reached or the link dropped: read again.
    Lost,
    /// The machine has no such session.
    Gone,
    /// What the machine or the app said.
    Failed { message: String },
}

impl From<DesktopError> for ViewError {
    fn from(e: DesktopError) -> Self {
        if e.is_transport() {
            Self::Lost
        } else if matches!(e, DesktopError::Api(ApiError::NotFound {})) {
            Self::Gone
        } else {
            Self::Failed {
                message: e.to_string(),
            }
        }
    }
}

/// One session in full. The calling webview's session view shows it, so
/// its notices stay quiet while the view keeps reading it. `before` reads
/// the `last` entries before that entry, to load older ones.
// The arguments are the fields of the window's `invoke` call.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn agent_view(
    app: AppHandle,
    window: WebviewWindow,
    fleet: Machines<'_>,
    shown: State<'_, AgentsShown>,
    machine_id: String,
    agent: String,
    session: SessionId,
    last: u32,
    before: Option<u64>,
) -> Result<SessionPage, ViewError> {
    // A start is read from this device, before its machine has the session.
    if let Ok(start) = session.as_ref().parse::<StartId>() {
        return app
            .state::<Arc<Starts>>()
            .read(|held| held.get(start).map(Start::page))
            .map(SessionPage::from)
            .ok_or(ViewError::Gone);
    }

    shown.read(
        window.label(),
        &machine_id,
        &agent,
        &session,
        Instant::now(),
    );
    crate::notices::history::agent_viewed(&app, &machine_id, &agent, &session);
    let request = GetAgentSession {
        machine_id,
        agent,
        session,
        last,
        before,
    };
    let machine = fleet.machine(&request.machine_id)?;
    let read = direct::session(&fleet, machine, &request);
    let mut view = fleet
        .asked(
            machine,
            &request.session,
            "reading the session",
            READ_LIMIT,
            read,
        )
        .await?;

    if let Some(mesh) = fleet.mesh() {
        beep_fleet::session_titles::apply(&mesh, &mut view.session);
    }

    app.state::<crate::palette::PaletteCache>()
        .found_session(&app, view.session.clone());
    Ok(view.into())
}

/// Older entries of a session, as `agent_view` with `before` reads them.
#[derive(Debug, Serialize)]
pub struct OlderPage {
    entries: Vec<EntryRow>,
    earlier: u32,
}

/// The newest `last` entries of a session, or the entries before `before`.
/// Reads only the transcript, without marking the session as viewed.
#[tauri::command]
pub async fn agent_older(
    fleet: Machines<'_>,
    machine_id: String,
    agent: String,
    session: SessionId,
    last: u32,
    before: Option<u64>,
) -> Result<OlderPage, ViewError> {
    let request = GetAgentSession {
        machine_id,
        agent,
        session,
        last,
        before,
    };
    let machine = fleet.machine(&request.machine_id)?;
    let read = direct::older(&fleet, machine, &request);
    let (entries, earlier) = fleet
        .asked(
            machine,
            &request.session,
            "reading older entries",
            READ_LIMIT,
            read,
        )
        .await?;

    Ok(OlderPage {
        entries: entries.into_iter().map(EntryRow::from).collect(),
        earlier,
    })
}

/// The folder on this Mac for a session's folder, to open in an editor; `None`
/// when this Mac does not have it.
#[tauri::command]
pub async fn session_folder_here(
    fleet: Machines<'_>,
    machine_id: String,
    cwd: String,
) -> Result<Option<String>> {
    let folder = work_copy::folder_here(&fleet, &machine_id, &cwd).await;
    Ok(folder.map(|f| f.to_string_lossy().into_owned()))
}

/// The device a session uses, as its machine keeps it.
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum KeptDevice {
    Kept {
        pick: Option<DevicePick>,
    },
    /// The machine's beep-node is too old to keep one.
    Unsupported,
}

#[tauri::command]
pub async fn agent_device(
    fleet: Machines<'_>,
    machine_id: String,
    agent: String,
    session: SessionId,
) -> Result<KeptDevice> {
    let machine = fleet.machine(&machine_id)?;
    let version = fleet.status().version(machine);

    if !newer_than(version.as_deref(), &BEFORE_SET_DEVICE) {
        return Ok(KeptDevice::Unsupported);
    }

    let read = direct::device(&fleet, machine, &agent, &session);
    let step = "reading the session's device";
    let device = fleet
        .asked(machine, &session, step, SMALL_LIMIT, read)
        .await?;
    let pick = device.map(|used| DevicePick {
        machine: used.machine.to_string(),
        device: used.device.0,
    });
    Ok(KeptDevice::Kept { pick })
}

/// A session's modes for the page.
#[derive(Debug, Serialize)]
pub struct ModesRow {
    current: Option<String>,
    modes: Vec<AgentMode>,
    /// The plan waiting for approval, as cleaned HTML.
    plan: Option<String>,
}

impl From<AgentSessionModes> for ModesRow {
    fn from(modes: AgentSessionModes) -> Self {
        Self {
            current: modes.current,
            modes: modes.modes,
            plan: modes.plan.as_deref().map(markdown::to_html),
        }
    }
}

/// The modes of a Beep session and the plan it waits on approval of; `None`
/// from a machine too old to say.
#[tauri::command]
pub async fn agent_modes(
    fleet: Machines<'_>,
    machine_id: String,
    agent: String,
    session: SessionId,
) -> Result<Option<ModesRow>> {
    let request = GetAgentSessionModes {
        machine_id,
        agent,
        session,
    };
    let modes = {
        let machine = fleet.machine(&request.machine_id)?;
        let read = direct::modes(&fleet, machine, &request);
        let step = "reading the session's modes";
        known(
            fleet
                .asked(machine, &request.session, step, SMALL_LIMIT, read)
                .await,
        )
    }?;
    Ok(modes.map(Into::into))
}

/// The commands a Beep session's agent has in the background, oldest
/// first; none from a node too old to say.
#[tauri::command]
pub async fn agent_tasks(
    fleet: Machines<'_>,
    machine_id: String,
    agent: String,
    session: SessionId,
) -> Result<Vec<AgentTask>> {
    let request = GetAgentSessionTasks {
        machine_id,
        agent,
        session,
    };
    let tasks = {
        let machine = fleet.machine(&request.machine_id)?;
        let read = direct::tasks(&fleet, machine, &request);
        let step = "reading the session's background commands";
        known(
            fleet
                .asked(machine, &request.session, step, SMALL_LIMIT, read)
                .await,
        )
    }?;
    Ok(tasks.map(|tasks| tasks.tasks).unwrap_or_default())
}

/// Moves a closed conversation to a project folder on its current machine.
#[tauri::command]
pub async fn agent_relocate(
    app: AppHandle,
    fleet: Machines<'_>,
    machine_id: String,
    agent: String,
    session: SessionId,
    folder: String,
) -> Result<()> {
    let machine = fleet
        .member(&machine_id)
        .ok_or_else(|| DesktopError::BadRequest("The machine must be in the mesh.".into()))?;
    let fleet_arc = fleet.inner().clone();
    fleet
        .node_ops()
        .submit(
            Some(app.clone()),
            fleet_arc,
            machine,
            JobKind::RelocateSession {
                agent,
                session,
                folder,
            },
            FAST_JOB_WAIT,
        )
        .await?;
    // The settled job lists the machine again and has the lists ask it.
    crate::fleet::home_signal::poke(&app);
    Ok(())
}

/// A move record as the window shows it.
#[derive(Debug, Serialize, PartialEq)]
pub struct MoveView {
    /// The agent's own id for the session.
    pub session: SessionId,
    pub agent: String,
    /// The machine that ran it, and the one that takes it over.
    pub from: String,
    pub to: String,
    pub project: String,
    /// Milliseconds since the Unix epoch.
    pub at: u64,
}

impl From<SessionMove> for MoveView {
    fn from(record: SessionMove) -> Self {
        Self {
            session: record.id,
            agent: record.agent.to_string(),
            from: record.from.to_string(),
            to: record.to.to_string(),
            project: record.project,
            at: record.at,
        }
    }
}

/// The moves in the mesh: the latest record of each session.
pub(crate) async fn moves(fleet: FleetState<'_>) -> Result<Vec<MoveView>> {
    Ok(desk(&fleet)
        .await?
        .moves()
        .into_iter()
        .map(Into::into)
        .collect())
}

#[cfg(test)]
#[path = "../tests/agents_sessions.rs"]
mod tests;
