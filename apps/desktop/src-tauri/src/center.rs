//! The command center: one window beside the bar that shows what the
//! account is doing, starting with Home. This file opens the window and
//! gathers Home's data from the server.

use crate::{
    bar,
    beep::{Beep, Pick, Sent, epoch_ms},
    error::{DesktopError, Result},
};
use beep_proto::{
    AgentQuestion, AgentStatus, AnswerAgentQuestion, ApiError, ChatApproval, Cron,
    ListAgentSessions, ListCrons, ListMachineDetails, ListMachines, ListPendingApprovals,
    ListResearches, Machine, MachineApp, MachineDetails, MachineEnv, MachineGpu, MachineSession,
    PendingApproval, ResearchStatus, ResearchSummary, Session,
};
use serde::Serialize;
use std::{cmp::Reverse, collections::HashMap, sync::Arc};
use tauri::{
    ActivationPolicy, AppHandle, LogicalPosition, LogicalSize, Manager, Runtime, State,
    TitleBarStyle, WebviewUrl, WebviewWindowBuilder, ipc::Channel,
};
use tokio::join;
use uuid::Uuid;

type Service<'a> = State<'a, Arc<Beep>>;

pub const LABEL: &str = "center";
/// The window never shrinks below this.
const MIN_WIDTH: f64 = 1000.0;
const MIN_HEIGHT: f64 = 640.0;
/// Share of the screen the window opens at.
const SHARE: f64 = 0.82;

/// Where the window opens: most of the screen under the mouse, centered.
fn frame<R: Runtime>(app: &AppHandle<R>) -> Option<(LogicalPosition<f64>, LogicalSize<f64>)> {
    let cursor = app.cursor_position().ok()?;
    let monitor = app
        .monitor_from_point(cursor.x, cursor.y)
        .ok()
        .flatten()
        .or_else(|| app.primary_monitor().ok().flatten())?;
    let scale = monitor.scale_factor();
    let area = monitor.work_area();
    let left = f64::from(area.position.x) / scale;
    let top = f64::from(area.position.y) / scale;
    let full_w = f64::from(area.size.width) / scale;
    let full_h = f64::from(area.size.height) / scale;
    let width = (full_w * SHARE).max(MIN_WIDTH.min(full_w));
    let height = (full_h * SHARE).max(MIN_HEIGHT.min(full_h));
    Some((
        LogicalPosition::new(left + (full_w - width) / 2.0, top + (full_h - height) / 2.0),
        LogicalSize::new(width, height),
    ))
}

/// Shows the window, opening it if needed. While it is open Beep sits in
/// the Dock, as with research windows.
pub fn open<R: Runtime>(app: &AppHandle<R>) -> Result<()> {
    bar::hide(app);
    if let Some(window) = app.get_webview_window(LABEL) {
        window.show()?;
        window.set_focus()?;
        return Ok(());
    }
    if let Err(e) = app.set_activation_policy(ActivationPolicy::Regular) {
        log::warn!("joining the Dock: {e}");
    }
    let mut builder = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("center.html".into()))
        .title("Beep")
        .min_inner_size(MIN_WIDTH, MIN_HEIGHT)
        .resizable(true)
        .maximizable(true)
        .title_bar_style(TitleBarStyle::Overlay)
        .hidden_title(true);
    builder = match frame(app) {
        Some((position, size)) => builder
            .position(position.x, position.y)
            .inner_size(size.width, size.height),
        None => builder.inner_size(MIN_WIDTH, MIN_HEIGHT).center(),
    };
    builder.build()?.set_focus()?;
    Ok(())
}

#[tauri::command]
pub fn open_center(app: AppHandle) -> Result<()> {
    open(&app)
}

/// Opens the bar on a new chat that holds `prompt`, and sends it.
#[tauri::command]
pub fn center_compose(app: AppHandle, beep: Service<'_>, prompt: String) -> Result<()> {
    let prompt = prompt.trim();
    if prompt.is_empty() {
        return Err(DesktopError::BadRequest("say what to ask".into()));
    }
    let session = Uuid::new_v4();
    beep.set_session(Some(session.to_string()));
    let sent = Sent {
        message: prompt.to_owned(),
        images: Vec::new(),
        files: Vec::new(),
        research: false,
        voice: None,
        spoken: false,
    };
    // The bar follows the run itself, so nothing reads this channel.
    let events = Channel::new(|_| Ok(()));
    let beep = beep.inner().clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = beep
            .send(Some(session.to_string()), sent, Pick::default(), events)
            .await
        {
            log::warn!("sending a prompt from the command center: {e}");
        }
    });
    bar::show(&app, Some("open"));
    Ok(())
}

/// Answers a question an agent session asks.
#[tauri::command]
pub async fn center_answer_agent(
    beep: Service<'_>,
    machine_id: String,
    agent: String,
    session: String,
    question: u32,
    reply: String,
) -> Result<()> {
    beep.call(AnswerAgentQuestion {
        machine_id,
        agent,
        session,
        question,
        reply,
    })
    .await
}

/// What Home shows. A section is `None` when the server cannot list it for
/// this account.
#[derive(Debug, Serialize)]
pub struct HomeView {
    machines: Option<Vec<MachineCard>>,
    running: Option<Vec<RunningRow>>,
    needs_you: Vec<NeedsYou>,
    coming_up: Option<Vec<CronRow>>,
}

/// A machine with what it reported and what runs on it.
#[derive(Debug, Serialize)]
struct MachineCard {
    id: String,
    name: String,
    online: bool,
    /// In milliseconds since the Unix epoch.
    seen_at: u64,
    version: String,
    env: Option<MachineEnv>,
    apps: Vec<MachineApp>,
    gpus: Vec<MachineGpu>,
    capabilities: Vec<&'static str>,
    /// Agent sessions on it that have not ended.
    agents: u32,
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
enum NeedsYou {
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
    schedule: String,
    /// RFC 3339, in UTC.
    next_run_at: String,
}

/// The server's answers, as they came back.
struct HomeParts {
    machines: Result<Vec<Machine>>,
    details: Result<Vec<MachineDetails>>,
    sessions: Result<Vec<MachineSession>>,
    researches: Result<Vec<ResearchSummary>>,
    approvals: Result<Vec<ChatApproval>>,
    chats: Result<Vec<Session>>,
    crons: Result<Vec<Cron>>,
}

#[tauri::command]
pub async fn center_home(beep: Service<'_>) -> Result<HomeView> {
    let (machines, details, sessions, researches, approvals, chats, crons) = join!(
        beep.call(ListMachines {}),
        beep.call(ListMachineDetails {}),
        beep.call(ListAgentSessions {}),
        beep.call(ListResearches {}),
        beep.call(ListPendingApprovals {}),
        beep.sessions(),
        beep.call(ListCrons {}),
    );
    Ok(home_view(HomeParts {
        machines,
        details,
        sessions,
        researches,
        approvals,
        chats,
        crons,
    }))
}

/// `None` for a request the server does not know or this account may not
/// make; any other failure goes to the log.
fn section<T>(what: &str, result: Result<T>) -> Option<T> {
    match result {
        Ok(listed) => Some(listed),
        Err(DesktopError::Api(ApiError::BadRequest { .. } | ApiError::Unauthorized {})) => None,
        Err(e) => {
            log::warn!("listing {what} for the command center: {e}");
            None
        }
    }
}

fn home_view(parts: HomeParts) -> HomeView {
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
    HomeView {
        machines: section("machines", parts.machines).map(|machines| {
            let details = section("machine details", parts.details).unwrap_or_default();
            machine_cards(machines, details, &live)
        }),
        running,
        needs_you: needs_you(
            sessions.iter().flatten(),
            section("approvals", parts.approvals).unwrap_or_default(),
            &chats,
        ),
        coming_up: section("routines", parts.crons).map(coming_up),
    }
}

/// Online machines first, then the busiest, then by name.
fn machine_cards(
    machines: Vec<Machine>,
    details: Vec<MachineDetails>,
    live: &[&MachineSession],
) -> Vec<MachineCard> {
    let mut agents: HashMap<&str, u32> = HashMap::new();
    for session in live {
        *agents.entry(session.machine_id.as_str()).or_default() += 1;
    }
    let mut details: HashMap<String, MachineDetails> =
        details.into_iter().map(|d| (d.id.clone(), d)).collect();
    let mut cards: Vec<MachineCard> = machines
        .into_iter()
        .map(|machine| {
            let detail = details.remove(&machine.id);
            MachineCard {
                agents: agents.get(machine.id.as_str()).copied().unwrap_or(0),
                seen_at: epoch_ms(machine.seen_at),
                apps: detail.as_ref().map(|d| d.apps.clone()).unwrap_or_default(),
                gpus: detail.as_ref().map(|d| d.gpus.clone()).unwrap_or_default(),
                capabilities: detail
                    .iter()
                    .flat_map(|d| &d.capabilities)
                    .map(|c| c.as_str())
                    .collect(),
                id: machine.id,
                name: machine.name,
                online: machine.online,
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
            status: session.status.as_str(),
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
            status: research.status.as_str(),
            round: research.round,
            sources: research.sources,
            started_at: research.started_at,
        }
    }
}

/// Everything waiting on an answer, oldest first. An agent question counts
/// from its session's last activity. A chat approval carries no time of its
/// own, so it counts from its chat's last change; a chat the list lacks
/// keeps the place the server gave it, after the approval before it.
fn needs_you<'a>(
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
    let mut since = 0;
    for approval in approvals {
        if let Some(&at) = changed.get(approval.session_id.to_string().as_str()) {
            since = u64::try_from(at).unwrap_or(0);
        }
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
                schedule: c.schedule,
            })
        })
        .collect();
    rows.sort_by(|a, b| a.next_run_at.cmp(&b.next_run_at));
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use beep_proto::Approval;
    use std::time::{Duration, SystemTime};

    fn at(ms: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_millis(ms)
    }

    fn machine(id: &str, online: bool) -> Machine {
        Machine {
            id: id.into(),
            name: id.into(),
            online,
            seen_at: at(0),
            version: "0.1.0".into(),
            env: None,
        }
    }

    fn agent(machine: &str, id: &str, last_activity: u64) -> MachineSession {
        MachineSession {
            machine_id: machine.into(),
            machine_name: machine.into(),
            id: id.into(),
            agent: "claude".into(),
            title: None,
            cwd: "/srv/project".into(),
            status: AgentStatus::Working {},
            doing: None,
            started_at: at(0),
            last_activity: at(last_activity),
            external: false,
            question: None,
        }
    }

    fn asking(machine: &str, id: &str, last_activity: u64) -> MachineSession {
        MachineSession {
            question: Some(AgentQuestion {
                id: 1,
                text: "Run cargo test".into(),
                detail: None,
                options: Vec::new(),
            }),
            ..agent(machine, id, last_activity)
        }
    }

    fn approval(session: Uuid) -> ChatApproval {
        ChatApproval {
            session_id: session,
            session_title: "Chat".into(),
            pending: PendingApproval {
                run_id: "run".into(),
                query_id: "query".into(),
                approval: Approval {
                    app: "Fastmail".into(),
                    action: "send email".into(),
                    fields: Vec::new(),
                },
            },
        }
    }

    fn parts() -> HomeParts {
        HomeParts {
            machines: Ok(Vec::new()),
            details: Ok(Vec::new()),
            sessions: Ok(Vec::new()),
            researches: Ok(Vec::new()),
            approvals: Ok(Vec::new()),
            chats: Ok(Vec::new()),
            crons: Ok(Vec::new()),
        }
    }

    #[test]
    fn machines_count_their_agents() {
        let view = home_view(HomeParts {
            machines: Ok(vec![machine("build-box", true), machine("mac-box", true)]),
            sessions: Ok(vec![agent("build-box", "a", 1), agent("build-box", "b", 2)]),
            ..parts()
        });
        let cards = view.machines.expect("machines were listed");
        let build = cards.iter().find(|c| c.id == "build-box").expect("card");
        let mac = cards.iter().find(|c| c.id == "mac-box").expect("card");
        assert_eq!((build.agents, mac.agents), (2, 0));
    }

    #[test]
    fn unsupported_section_is_none() {
        let view = home_view(HomeParts {
            machines: Ok(vec![machine("build-box", true)]),
            sessions: Err(DesktopError::Api(ApiError::bad_request("unknown op 131"))),
            ..parts()
        });
        assert!(view.running.is_none());
        assert!(view.machines.is_some());
        assert!(view.coming_up.is_some());
    }

    #[test]
    fn machines_sort_online_then_by_agents() {
        let view = home_view(HomeParts {
            machines: Ok(vec![
                machine("off-busy", false),
                machine("on-idle", true),
                machine("on-busy", true),
                machine("off-idle", false),
            ]),
            sessions: Ok(vec![
                agent("off-busy", "a", 1),
                agent("off-busy", "b", 2),
                agent("on-busy", "c", 3),
            ]),
            ..parts()
        });
        let order: Vec<String> = view
            .machines
            .expect("machines were listed")
            .into_iter()
            .map(|c| c.id)
            .collect();
        assert_eq!(order, ["on-busy", "on-idle", "off-busy", "off-idle"]);
    }

    #[test]
    fn needs_you_puts_oldest_first() {
        let chat = Uuid::new_v4();
        let view = home_view(HomeParts {
            sessions: Ok(vec![
                asking("build-box", "late", 3000),
                asking("build-box", "early", 1000),
            ]),
            approvals: Ok(vec![approval(chat)]),
            chats: Ok(vec![Session {
                id: chat.to_string(),
                title: "Chat".into(),
                pinned: false,
                updated_at: 2000,
            }]),
            ..parts()
        });
        let order: Vec<String> = view
            .needs_you
            .iter()
            .map(|item| match item {
                NeedsYou::Chat { .. } => "chat".into(),
                NeedsYou::Agent { session, .. } => session.clone(),
            })
            .collect();
        assert_eq!(order, ["early", "chat", "late"]);
    }
}
