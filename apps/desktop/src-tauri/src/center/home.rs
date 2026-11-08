//! Home: machines, what runs, what needs the user, routines coming up,
//! app updates and projects, gathered from the server in one go. Entry
//! point: `center_home`; `section` and `is_unsupported` read any list.

use super::{
    home_sessions::AgentSessions,
    view_cache::{View, ViewCache, cache_view},
};
use crate::{
    beep::{Service, epoch_ms},
    error::{DesktopError, Result},
    fleet::{
        Fleet, alerts,
        projects::{FleetState, desk},
    },
    machines::list::{self as machine_list, Outside},
    projects::{ProjectRow, known},
};
use beep_proto::{
    AgentNoticeKey, AgentQuestion, AgentStatus, AnswerAgentQuestion, ApiError, AppUpdate,
    ChatApproval, Cron, DismissUpdates, ImportedProject, ListCrons, ListImportedProjects,
    ListPendingApprovals, ListResearches, ListUpdates, Machine, MachineApp, MachineDetails,
    MachineDisk, MachineEnv, MachineGpu, MachineSession, MachineStats, OpenUpdateChat,
    PendingApproval, ResearchStatus, ResearchSummary, Session,
};
use iroh::EndpointId;
use serde::Serialize;
use std::{cmp::Reverse, collections::HashMap, sync::Arc};
use tauri::{AppHandle, State, WebviewWindow};
use tokio::join;
use uuid::Uuid;

/// Answers a question an agent session asks.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn center_answer_agent(
    window: WebviewWindow,
    events: State<'_, crate::fleet::events::Events>,
    beep: Service<'_>,
    fleet: State<'_, Arc<Fleet>>,
    machine_id: String,
    agent: String,
    session: String,
    question: u32,
    reply: String,
) -> Result<()> {
    // A machine of the mesh is asked directly; any other through the server.
    if alerts::reaches(&fleet, &machine_id) {
        if let Some(machine) = fleet.member(&machine_id) {
            let operation = beep_proto::node::events::Operation::Agent {
                agent: agent.clone(),
                session: session.clone(),
                request: beep_proto::node::agent::Request::Answer {
                    question,
                    reply: reply.clone(),
                },
            };
            if events
                .call(window.label(), machine, operation)
                .await?
                .is_some()
            {
                return Ok(());
            }
        }
        let key = AgentNoticeKey {
            machine_id,
            agent,
            session,
            question: Some(question),
            machine_name: String::new(),
        };
        return alerts::answer(&fleet, &key, &reply).await;
    }
    beep.call(AnswerAgentQuestion {
        machine_id,
        agent,
        session,
        question,
        reply,
    })
    .await
}

/// Dismisses updates from the user's apps, by `AppUpdate::id`.
#[tauri::command]
pub async fn center_dismiss_updates(beep: Service<'_>, ids: Vec<i64>) -> Result<()> {
    beep.call(DismissUpdates { ids }).await
}

/// The chat about an update, which opens with Beep telling it; made on the
/// first ask. `None` from a server too old to make one.
#[tauri::command]
pub async fn center_open_update_chat(beep: Service<'_>, id: i64) -> Result<Option<String>> {
    Ok(known(beep.call(OpenUpdateChat { id }).await)?.map(|session| session.id))
}

/// What Home shows. A section is `None` when the server cannot list it for
/// this account.
#[derive(Debug, Serialize)]
pub struct HomeView {
    machines: Option<Vec<MachineCard>>,
    running: Option<Vec<RunningRow>>,
    needs_you: Vec<NeedsYou>,
    coming_up: Option<Vec<CronRow>>,
    updates: Option<Vec<UpdateRow>>,
    /// The projects imported onto the user's machines.
    projects: Option<Vec<ProjectRow>>,
}

/// Something the event triage agent found in one of the user's apps.
#[derive(Debug, Serialize)]
struct UpdateRow {
    id: i64,
    summary: String,
    /// The app's slug, such as `gmail`; empty when unknown.
    app: String,
    /// How the user knows the app, such as "Gmail"; empty when unknown.
    app_name: String,
    /// In milliseconds since the Unix epoch.
    at: u64,
}

impl From<AppUpdate> for UpdateRow {
    fn from(update: AppUpdate) -> Self {
        Self {
            id: update.id,
            summary: update.summary,
            app: update.app,
            app_name: update.app_name,
            at: epoch_ms(update.at),
        }
    }
}

/// A machine with what it reported and what runs on it.
#[derive(Debug, Serialize)]
struct MachineCard {
    can_host: bool,
    id: String,
    name: String,
    online: bool,
    /// In the mesh, or one only the account lists, reached through the server.
    joined: bool,
    /// In milliseconds since the Unix epoch.
    seen_at: u64,
    version: String,
    env: Option<MachineEnv>,
    apps: Vec<MachineApp>,
    gpus: Vec<MachineGpu>,
    capabilities: Vec<&'static str>,
    /// Agent sessions on it that have not ended.
    agents: u32,
    /// Its live resources; `None` while offline, before its first sample,
    /// or from a server or node from before them.
    stats: Option<CardStats>,
}

/// What a machine last said it has free.
#[derive(Debug, Serialize)]
struct CardStats {
    /// In milliseconds since the Unix epoch.
    sampled_at: u64,
    /// The workspace's volume first, then home's when it is another.
    disks: Vec<MachineDisk>,
    memory_used_mb: u64,
    memory_total_mb: u64,
    load: Option<f64>,
}

impl From<MachineStats> for CardStats {
    fn from(stats: MachineStats) -> Self {
        Self {
            sampled_at: epoch_ms(stats.sampled_at),
            disks: stats.disks,
            memory_used_mb: stats.memory_used_mb,
            memory_total_mb: stats.memory_total_mb,
            load: stats.load,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum RunningRow {
    Agent {
        machine_id: String,
        machine_name: String,
        id: String,
        agent: String,
        title: Option<String>,
        cwd: String,
        status: &'static str,
        doing: Option<String>,
        started_at: u64,
        last_activity: u64,
        external: bool,
    },
    Research {
        session: Uuid,
        topic: String,
        status: &'static str,
        round: u32,
        sources: u32,
        started_at: i64,
    },
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum NeedsYou {
    Chat {
        session_id: Uuid,
        session_title: String,
        pending: PendingApproval,
    },
    Agent {
        machine_id: String,
        machine_name: String,
        agent: String,
        session: String,
        title: Option<String>,
        question: AgentQuestion,
    },
}

#[derive(Debug, Serialize)]
struct CronRow {
    id: String,
    label: String,
    /// What it asks Beep, the title when it has no label.
    prompt: String,
    schedule: String,
    /// RFC 3339, in UTC.
    next_run_at: String,
}

/// The server's answers, as they came back.
struct HomeParts {
    machines: Result<Vec<Machine>>,
    details: Result<Vec<MachineDetails>>,
    stats: Result<Vec<MachineStats>>,
    sessions: Result<Vec<MachineSession>>,
    researches: Result<Vec<ResearchSummary>>,
    approvals: Result<Vec<ChatApproval>>,
    chats: Result<Vec<Session>>,
    crons: Result<Vec<Cron>>,
    updates: Result<Vec<AppUpdate>>,
    projects: Result<Vec<ImportedProject>>,
}

/// Home: the machines from the mesh, the rest from the server. Every section
/// but the machines and their agent sessions waits for the server; those wait only as long as `AgentSessions` allows.
#[tauri::command]
pub async fn center_home(
    app: AppHandle,
    beep: Service<'_>,
    fleet: State<'_, Arc<Fleet>>,
    held: State<'_, Outside>,
    cache: State<'_, Arc<ViewCache>>,
    agents: State<'_, Arc<AgentSessions>>,
    session: FleetState<'_>,
) -> Result<HomeView> {
    let (sessions, researches, approvals, chats, crons, updates, listed, imported) = join!(
        agents.list(&app, beep.inner(), cache.inner()),
        beep.call(ListResearches {}),
        beep.call(ListPendingApprovals {}),
        beep.sessions(),
        beep.call(ListCrons {}),
        beep.call(ListUpdates {}),
        beep.call(ListImportedProjects {}),
        mesh_imports(&fleet, &session),
    );
    let projects = with_server(imported, listed);
    let mut view = home_view(HomeParts {
        machines: Ok(machine_list::machines(&app, &fleet, &held)),
        details: Ok(machine_list::details(&app, &fleet, &held)),
        stats: Ok(machine_list::stats(&app, &fleet, &held)),
        sessions,
        researches,
        approvals,
        chats,
        crons,
        updates,
        projects,
    })?;
    for card in view.machines.iter_mut().flatten() {
        card.joined = fleet.member(&card.id).is_some();
        card.can_host = card
            .id
            .parse::<EndpointId>()
            .ok()
            .is_none_or(|id| !fleet.mesh().is_some_and(|mesh| mesh.is_observer(id)));
    }
    cache_view(&cache, View::Home, &view);
    Ok(view)
}

/// The projects the mesh holds, one row per machine of it that has one; none while the mesh is
/// not open, so Home never waits for it.
pub(crate) async fn mesh_imports(fleet: &Fleet, session: &FleetState<'_>) -> Vec<ImportedProject> {
    if fleet.mesh().is_none() {
        return Vec::new();
    }
    match desk(session).await {
        Ok(desk) => desk.imported().await,
        Err(e) => {
            log::info!("no projects from the mesh: {e}");
            Vec::new()
        }
    }
}

/// The mesh's projects, then the server's on machines the mesh does not hold. With no server, or
/// one that does not answer, the mesh's alone; the server's failure shows only when the mesh has
/// nothing, so a cached Home is kept rather than shown empty.
pub(crate) fn with_server(
    mut mesh: Vec<ImportedProject>,
    server: Result<Vec<ImportedProject>>,
) -> Result<Vec<ImportedProject>> {
    match server {
        Ok(listed) => {
            let more: Vec<ImportedProject> = listed
                .into_iter()
                .filter(|row| {
                    !mesh.iter().any(|held| {
                        held.project == row.project && held.machine_id == row.machine_id
                    })
                })
                .collect();
            mesh.extend(more);
            Ok(mesh)
        }
        Err(DesktopError::NoServer | DesktopError::Unlinked) => Ok(mesh),
        Err(e) if mesh.is_empty() => Err(e),
        Err(_) => Ok(mesh),
    }
}

/// `None` for a request the server does not know or this account may not
/// make; any other failure goes to the log. A server that is not there is no failure.
pub(crate) fn section<T>(what: &str, result: Result<T>) -> Option<T> {
    match result {
        Ok(listed) => Some(listed),
        Err(DesktopError::NoServer | DesktopError::Unlinked) => None,
        Err(e) if is_unsupported(&e) => None,
        Err(e) => {
            log::warn!("listing {what} for the command center: {e}");
            None
        }
    }
}

/// Home from the server's answers. A failed machine list that is not a
/// plain "not here" means the link itself is down, so the whole Home is
/// stale and the caller keeps what it showed.
fn home_view(parts: HomeParts) -> Result<HomeView> {
    let machines = match parts.machines {
        Err(e) if !is_unsupported(&e) => return Err(e),
        other => other,
    };
    let sessions = section("agent sessions", parts.sessions);
    let live: Vec<&MachineSession> = sessions
        .iter()
        .flatten()
        .filter(|s| !matches!(s.status, AgentStatus::Ended {}))
        .collect();
    let researches = section("researches", parts.researches);
    let running = sessions.as_ref().map(|_| {
        let agents = live.iter().map(|s| RunningRow::from(*s));
        let researching = researches
            .iter()
            .flatten()
            .filter(|r| is_running(r.status))
            .map(RunningRow::from);
        agents.chain(researching).collect()
    });
    let chats = section("chats", parts.chats).unwrap_or_default();
    Ok(HomeView {
        machines: section("machines", machines).map(|machines| {
            let details = section("machine details", parts.details).unwrap_or_default();
            let stats = section("machine stats", parts.stats).unwrap_or_default();
            machine_cards(machines, details, stats, &live)
        }),
        running,
        needs_you: needs_you(
            sessions.iter().flatten(),
            section("approvals", parts.approvals).unwrap_or_default(),
            &chats,
        ),
        coming_up: section("routines", parts.crons).map(coming_up),
        updates: section("updates", parts.updates)
            .map(|updates| updates.into_iter().map(UpdateRow::from).collect()),
        projects: section("projects", parts.projects)
            .map(|projects| projects.into_iter().map(ProjectRow::from).collect()),
    })
}

/// Whether the server does not know the request or this account may not
/// make it, as opposed to a failure on the way.
pub(crate) fn is_unsupported(e: &DesktopError) -> bool {
    matches!(
        e,
        DesktopError::Api(ApiError::BadRequest { .. } | ApiError::Unauthorized {})
    )
}

/// Online machines first, then the busiest, then by name.
fn machine_cards(
    machines: Vec<Machine>,
    details: Vec<MachineDetails>,
    stats: Vec<MachineStats>,
    live: &[&MachineSession],
) -> Vec<MachineCard> {
    let mut agents: HashMap<&str, u32> = HashMap::new();
    for session in live {
        *agents.entry(session.machine_id.as_str()).or_default() += 1;
    }
    let mut details: HashMap<String, MachineDetails> =
        details.into_iter().map(|d| (d.id.clone(), d)).collect();
    let mut stats: HashMap<String, MachineStats> =
        stats.into_iter().map(|s| (s.id.clone(), s)).collect();
    let mut cards: Vec<MachineCard> = machines
        .into_iter()
        .map(|machine| {
            let detail = details.remove(&machine.id);
            MachineCard {
                can_host: true,
                stats: stats.remove(&machine.id).map(CardStats::from),
                agents: agents.get(machine.id.as_str()).copied().unwrap_or(0),
                seen_at: epoch_ms(machine.seen_at),
                apps: detail.as_ref().map(|d| d.apps.clone()).unwrap_or_default(),
                gpus: detail.as_ref().map(|d| d.gpus.clone()).unwrap_or_default(),
                capabilities: detail
                    .iter()
                    .flat_map(|d| &d.capabilities)
                    .map(|c| <&'static str>::from(*c))
                    .collect(),
                id: machine.id,
                name: machine.name,
                online: machine.online,
                joined: true,
                version: machine.version,
                env: machine.env,
            }
        })
        .collect();
    cards.sort_by(|a, b| {
        (Reverse(a.online), Reverse(a.agents), &a.name).cmp(&(
            Reverse(b.online),
            Reverse(b.agents),
            &b.name,
        ))
    });
    cards
}

impl From<&MachineSession> for RunningRow {
    fn from(session: &MachineSession) -> Self {
        Self::Agent {
            machine_id: session.machine_id.clone(),
            machine_name: session.machine_name.clone(),
            id: session.id.clone(),
            agent: session.agent.clone(),
            title: session.title.clone(),
            cwd: session.cwd.clone(),
            status: session.status.into(),
            doing: session.doing.clone(),
            started_at: epoch_ms(session.started_at),
            last_activity: epoch_ms(session.last_activity),
            external: session.external,
        }
    }
}

impl From<&ResearchSummary> for RunningRow {
    fn from(research: &ResearchSummary) -> Self {
        Self::Research {
            session: research.session_id,
            topic: research.topic.clone(),
            status: Into::<&'static str>::into(research.status),
            round: research.round,
            sources: research.sources,
            started_at: research.started_at,
        }
    }
}

/// Everything waiting on an answer, oldest first. An agent question counts
/// from its session's last activity. A chat approval carries no time of its
/// own, so it counts from its chat's last change; a chat the list lacks
/// counts as the newest, so it comes last.
pub(crate) fn needs_you<'a>(
    sessions: impl Iterator<Item = &'a MachineSession>,
    approvals: Vec<ChatApproval>,
    chats: &[Session],
) -> Vec<NeedsYou> {
    let changed: HashMap<&str, i64> = chats
        .iter()
        .map(|c| (c.id.as_str(), c.updated_at))
        .collect();
    let mut waiting: Vec<(u64, NeedsYou)> = Vec::new();
    for session in sessions {
        if let Some(question) = &session.question {
            waiting.push((
                epoch_ms(session.last_activity),
                NeedsYou::Agent {
                    machine_id: session.machine_id.clone(),
                    machine_name: session.machine_name.clone(),
                    agent: session.agent.clone(),
                    session: session.id.clone(),
                    title: session.title.clone(),
                    question: question.clone(),
                },
            ));
        }
    }
    for approval in approvals {
        let since = changed
            .get(approval.session_id.to_string().as_str())
            .map_or(u64::MAX, |&at| u64::try_from(at).unwrap_or(0));
        waiting.push((
            since,
            NeedsYou::Chat {
                session_id: approval.session_id,
                session_title: approval.session_title,
                pending: approval.pending,
            },
        ));
    }
    waiting.sort_by_key(|(since, _)| *since);
    waiting.into_iter().map(|(_, item)| item).collect()
}

/// Whether a research is still at work.
fn is_running(status: ResearchStatus) -> bool {
    matches!(
        status,
        ResearchStatus::Planning {}
            | ResearchStatus::Researching {}
            | ResearchStatus::Reviewing {}
            | ResearchStatus::Writing {}
    )
}

/// Enabled routines by when they run next.
fn coming_up(crons: Vec<Cron>) -> Vec<CronRow> {
    let mut rows: Vec<CronRow> = crons
        .into_iter()
        .filter(|c| c.enabled)
        .filter_map(|c| {
            Some(CronRow {
                next_run_at: c.next_run_at?,
                id: c.id,
                label: c.label,
                prompt: c.prompt,
                schedule: c.schedule,
            })
        })
        .collect();
    rows.sort_by(|a, b| a.next_run_at.cmp(&b.next_run_at));
    rows
}

#[cfg(test)]
#[path = "../tests/center_home.rs"]
mod tests;
