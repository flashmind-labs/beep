//! Coding-agent sessions on the user's machines, for the bar's and the
//! window's agent views: which machines can run one, starting one and
//! reading one in full. `steer` steers it, `media` reads its files.

use crate::{
    agents::{
        steer::{PastedImage, uploads},
        work_copy,
    },
    beep::{Beep, Service, epoch_ms},
    center::{AgentsShown, home_sessions::AgentSessions},
    chat::markdown,
    error::{DesktopError, Result},
    fleet::{
        Fleet, Machines, SessionParent, agents as direct,
        asked::{ORDER_LIMIT, READ_LIMIT, SMALL_LIMIT},
        extra,
    },
    machines::list::Outside,
    projects::{known, unknown_op},
};
use beep_mesh::Key;
use beep_node::agents::tags::own_words;
use beep_proto::{
    AgentEntry, AgentEntryKind, AgentHost, AgentMode, AgentModel, AgentModels, AgentQuestion,
    AgentSessionModes, AgentSessionUsage, AgentSessionView, AgentStatus, AgentTask, AgentToolCall,
    ApiError, GetAgentSession, GetAgentSessionModes, GetAgentSessionTasks, GetAgentSessionUsage,
    ListAgentHosts, ListAgentModels, ListAgentSessions, MachinePick, MachineSession,
    PreviewAgentMachine, ResumeAgentSession, StartAgentSession,
    node::{
        agent::{CheckoutPlan, Effort},
        harness::HarnessConfig,
        mesh_state::StateValue,
    },
};
use iroh::EndpointId;
use serde::Serialize;
use std::{
    sync::{Arc, PoisonError},
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter, Manager, State, WebviewWindow};
use uuid::Uuid;

/// How long one scan of the sessions started outside Beep serves every
/// caller (the tray, Home, the palette) before the next runs: each scan
/// starts `ps`, `lsof` and git on every machine.
const SCAN_FRESH: Duration = Duration::from_secs(30);
/// How long the held session lists stand before a read lists every machine
/// again. Until then each machine's change replaces only its own rows.
const SESSIONS_FRESH: Duration = Duration::from_secs(30);

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
    /// A temporary reviewer of its parent's changes.
    reviewer: bool,
    /// Its subagents that still run.
    subagents: u32,
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
            reviewer: false,
            subagents: session.subagents,
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
    plan: Vec<StepRow>,
    files: Vec<String>,
    last_message: String,
    error: Option<String>,
    queued: u32,
    queued_messages: Vec<String>,
    turns: u32,
    entries: Vec<EntryRow>,
    earlier: u32,
}

impl From<AgentSessionView> for SessionPage {
    fn from(view: AgentSessionView) -> Self {
        Self {
            model: view.model,
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
            queued_messages: view.queued_messages,
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
    let reviewers = fleet.reviewers().into_iter().map(|session| (session, true));
    Ok(sessions
        .into_iter()
        .map(|session| (session, false))
        .chain(reviewers)
        .map(|(session, reviewer)| {
            let parent = fleet.parent_of(&session.machine_id, &session.id);
            SessionRow {
                parent,
                reviewer,
                ..SessionRow::from(session)
            }
        })
        .collect())
}

/// Saves the user's title independently of the agent's generated title.
#[tauri::command]
pub async fn agent_title_save(
    app: AppHandle,
    fleet: Machines<'_>,
    machine_id: String,
    agent: String,
    session: String,
    title: String,
) -> Result<()> {
    let mesh = fleet.ready().await?;
    beep_fleet::session_titles::save(&mesh, &machine_id, &agent, &session, &title).await?;
    app.state::<Arc<AgentSessions>>().stale();

    if let Err(e) = app.emit("agent-title-changed", ()) {
        log::warn!("could not tell the windows a session title changed: {e}");
    }

    Ok(())
}

/// Shows a session its machine said ended as ended in the lists held here, so
/// Home and the session lists drop it from Running at once rather than on a
/// later read, and has the windows read them again.
pub(crate) fn closed(app: &AppHandle, machine_id: &str, session: &str) {
    let end = |list: &mut [MachineSession]| {
        for listed in list
            .iter_mut()
            .filter(|listed| listed.machine_id == machine_id && listed.id == session)
        {
            listed.status = AgentStatus::Ended {};
            listed.doing = None;
            listed.question = None;
        }
    };
    let held = app.state::<Outside>();
    held.sessions.edit(end);
    // The scan runs again on the next read, which may find it gone.
    held.scans.stale();
    app.state::<Arc<AgentSessions>>().edit(end);

    if let Err(e) = app.emit("sessions-changed", session) {
        log::warn!("could not tell the windows a session closed: {e}");
    }
}

/// Whether two listings of a session differ in what the windows draw, which
/// leaves out what moves on every read.
fn same_session(old: &MachineSession, new: &MachineSession) -> bool {
    old.machine_id == new.machine_id
        && old.id == new.id
        && old.status == new.status
        && old.question.as_ref().map(|q| q.id) == new.question.as_ref().map(|q| q.id)
}

/// `machine` said its sessions changed: they are listed again, from its
/// subscription, and replace its rows alone, so the other machines are not
/// asked. A change that comes while it is listed has it listed once more.
pub(crate) async fn machine_changed(app: &AppHandle, machine: EndpointId) {
    let held = app.state::<Outside>();
    let id = machine.to_string();

    {
        let mut relisting = held
            .relisting
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some(again) = relisting.get_mut(&id) {
            *again = true;
            return;
        }
        relisting.insert(id.clone(), false);
    }

    let fleet = app.state::<Arc<Fleet>>().inner().clone();
    loop {
        if let Some(rows) = direct::sessions_of(&fleet, machine).await {
            let list = if machine == fleet.id() {
                &held.local
            } else {
                &held.sessions
            };
            list.replace(rows, |row| row.machine_id == id);
        }

        let mut relisting = held
            .relisting
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if relisting.get(&id) == Some(&true) {
            relisting.insert(id.clone(), false);
            continue;
        }
        relisting.remove(&id);
        break;
    }

    app.state::<Arc<AgentSessions>>().stale();
    app.state::<crate::palette::lists::PaletteCache>()
        .sessions_changed();
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
    let mut sessions = if held.local.was_read() {
        let f = fleet.clone();
        held.local.refreshed_after(
            app,
            async move { Ok(direct::local_sessions(&f).await) },
            same_session,
            SESSIONS_FRESH,
        )
    } else {
        let listed = direct::local_sessions(fleet).await;
        held.local.keep(listed.clone());
        listed
    };
    let (b, f) = (beep.clone(), fleet.clone());
    sessions.extend(held.sessions.refreshed_after(
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
        SESSIONS_FRESH,
    ));
    let f = fleet.clone();
    sessions.extend(held.scans.refreshed_after(
        app,
        async move { Ok(direct::scanned_sessions(&f).await) },
        same_session,
        SCAN_FRESH,
    ));
    fleet.apply_presence(&mut sessions);

    if let Some(mesh) = fleet.mesh() {
        for session in &mut sessions {
            beep_fleet::session_titles::apply(&mesh, session);
        }
    }

    sessions.sort_by_key(|session| std::cmp::Reverse(session.last_activity));
    sessions
}

/// Starts a session; no `machine_id` lets the server pick. `chat` hears
/// its turn ends and questions; `model`, from `agent_models`, picks the
/// agent's model, `effort` how hard it thinks and `checkout` the new
/// branch or worktree it starts on. `images` go with the prompt, only to a
/// machine in the mesh.
// The arguments are the fields of the window's `invoke` call.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn agent_start(
    app: AppHandle,
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
    images: Option<Vec<PastedImage>>,
) -> Result<Started> {
    let images = uploads(images.unwrap_or_default())?;
    let indexed_prompt = prompt.clone();
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
        direct::start(&fleet, request, images).await?
    } else if !images.is_empty() {
        return Err(DesktopError::BadRequest(
            "This Beep server does not take images yet. Start without them.".into(),
        ));
    } else {
        beep.call(request).await?
    };
    crate::palette::prompts::sent(&app, started.session.clone(), &indexed_prompt).await;
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
    context_window: Option<u64>,
    max_context_window: Option<u64>,
    provider: Option<String>,
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
            context_window: model.context_window,
            max_context_window: model.max_context_window,
            provider: model.provider,
        }
    }
}

/// The models `agent` can run on `machine_id`; failures retain their context
/// for the picker and the local log.
#[tauri::command]
pub async fn agent_models(
    beep: Service<'_>,
    fleet: Machines<'_>,
    machine_id: String,
    agent: String,
) -> Result<Option<ModelsView>> {
    let request = ListAgentModels {
        machine_id: machine_id.clone(),
        agent: agent.clone(),
    };
    let asked = match fleet.member(&machine_id) {
        Some(machine) => direct::models(&fleet, machine, &request).await,
        None => beep.call(request).await,
    };

    match asked {
        Ok(list) => Ok(Some(list.into())),
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
    app: AppHandle,
    beep: Service<'_>,
    fleet: Machines<'_>,
    machine_id: String,
    agent: String,
    session: String,
    prompt: Option<String>,
) -> Result<Started> {
    let indexed_prompt = prompt.clone();
    let request = ResumeAgentSession {
        machine_id,
        agent,
        session,
        prompt,
    };
    let started = match fleet.member(&request.machine_id) {
        Some(machine) => {
            let session = request.session.clone();
            let resume = direct::resume(&fleet, machine, request);
            fleet
                .asked(
                    machine,
                    &session,
                    "resuming the session",
                    ORDER_LIMIT,
                    resume,
                )
                .await?
        }
        None => beep.call(request).await?,
    };

    if let Some(prompt) = indexed_prompt {
        crate::palette::prompts::sent(&app, started.session.clone(), &prompt).await;
    }

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
    app.state::<Arc<crate::palette::prompts::PromptIndex>>()
        .remove(format!("{machine_id} {session}"))
        .await;
    // The windows read their session lists again.
    if let Err(e) = app.emit("sessions-changed", session) {
        log::warn!("could not tell the windows a session was deleted: {e}");
    }

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
    beep: Service<'_>,
    fleet: Machines<'_>,
    shown: State<'_, AgentsShown>,
    machine_id: String,
    agent: String,
    session: String,
    last: u32,
    before: Option<u64>,
) -> Result<SessionPage, ViewError> {
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
    let mut view = match fleet.member(&request.machine_id) {
        Some(machine) => {
            let read = direct::session(&fleet, machine, &request);
            fleet
                .asked(
                    machine,
                    &request.session,
                    "reading the session",
                    READ_LIMIT,
                    read,
                )
                .await?
        }
        None => beep.call(request).await?,
    };

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

/// The `last` entries before entry `before` of a session, for scrolling back. A machine of the
/// mesh reads only the transcript, not the session list `agent_view` reads too.
#[tauri::command]
pub async fn agent_older(
    beep: Service<'_>,
    fleet: Machines<'_>,
    machine_id: String,
    agent: String,
    session: String,
    last: u32,
    before: u64,
) -> Result<OlderPage, ViewError> {
    let request = GetAgentSession {
        machine_id,
        agent,
        session,
        last,
        before: Some(before),
    };
    let (entries, earlier) = match fleet.member(&request.machine_id) {
        Some(machine) => {
            let read = direct::older(&fleet, machine, &request);
            fleet
                .asked(
                    machine,
                    &request.session,
                    "reading older entries",
                    READ_LIMIT,
                    read,
                )
                .await?
        }
        None => {
            let view = beep.call(request).await?;
            (view.entries, view.earlier)
        }
    };

    Ok(OlderPage {
        entries: entries.into_iter().map(EntryRow::from).collect(),
        earlier,
    })
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
        Some(machine) => {
            let read = direct::usage(&fleet, machine, &request);
            let step = "reading the session's usage";
            fleet
                .asked(machine, &request.session, step, SMALL_LIMIT, read)
                .await
        }
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
        Some(machine) => {
            let read = direct::modes(&fleet, machine, &request);
            let step = "reading the session's modes";
            fleet
                .asked(machine, &request.session, step, SMALL_LIMIT, read)
                .await
        }
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
        Some(machine) => {
            let read = direct::tasks(&fleet, machine, &request);
            let step = "reading the session's background commands";
            fleet
                .asked(machine, &request.session, step, SMALL_LIMIT, read)
                .await
        }
        None => beep.call(request).await,
    })?;
    Ok(tasks
        .map(|tasks| tasks.tasks.into_iter().map(TaskRow::from).collect())
        .unwrap_or_default())
}

#[cfg(test)]
#[path = "../tests/agents_sessions.rs"]
mod tests;

/// Moves a closed conversation to a project folder on its current machine.
#[tauri::command]
pub async fn agent_relocate(
    app: AppHandle,
    fleet: Machines<'_>,
    machine_id: String,
    agent: String,
    session: String,
    folder: String,
) -> Result<()> {
    let machine = fleet
        .member(&machine_id)
        .ok_or_else(|| DesktopError::BadRequest("The machine must be in the mesh.".into()))?;
    let client = fleet.client(machine).await?;
    let done =
        beep_mesh::machine::sessions::relocate_session(&client, &agent, &session, &folder).await?;
    match done {
        beep_mesh::machine::sessions::NodeWork::Done(()) => {}
        beep_mesh::machine::sessions::NodeWork::Refused(why) => {
            return Err(DesktopError::BadRequest(why));
        }
    }
    app.emit("sessions-changed", session)
        .map_err(anyhow::Error::from)?;
    Ok(())
}
