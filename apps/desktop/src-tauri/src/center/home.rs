//! Home: machines, what runs, what needs the user and projects, gathered
//! from the mesh in one go. Entry point: `center_home`; `section` and
//! `is_unsupported` read any list.

use super::view_cache::{View, ViewCache, cache_view};
use crate::chat::markdown::inline_html;
use crate::{
    agents::sessions::{MoveView, agent_sessions, moves, overlaid},
    beep::epoch_ms,
    error::{DesktopError, Result},
    fleet::{
        Fleet,
        projects::{FleetState, desk},
        session_lists::SessionRow,
        session_mirror::{Rows, SessionMirror},
    },
    projects::ProjectRow,
};
use beep_proto::node::agent::LineChanges;
use beep_proto::{
    AgentNoticeKey, AgentQuestion, AgentStatus, ApiError, ImportedProject, Machine, MachineApp,
    MachineDetails, MachineDisk, MachineEnv, MachineGpu, MachineSession, MachineStats,
};
use beep_proto::{SessionId, watched};
use futures::{
    FutureExt,
    future::{BoxFuture, Shared},
};
use iroh::EndpointId;
use serde::Serialize;
use std::{
    cmp::Reverse,
    collections::HashMap,
    sync::{Arc, Mutex, MutexGuard},
    time::Duration,
};
use tauri::{AppHandle, Manager, State, WebviewWindow};
use tokio::{join, sync::watch, time::timeout};

/// How long Home waits for the first machine's rows when it has none.
const WAIT: Duration = Duration::from_millis(1500);

/// Answers a question an agent session asks. Its notice is answered too, as
/// is one the session no longer asks, and the tray looks again.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn center_answer_agent(
    window: WebviewWindow,
    fleet: State<'_, Arc<Fleet>>,
    machine_id: String,
    agent: String,
    session: SessionId,
    question: u32,
    reply: String,
) -> Result<()> {
    let key = AgentNoticeKey {
        machine_id,
        agent,
        session,
        question: Some(question),
        machine_name: String::new(),
    };

    crate::agents::outbox::answer(window.app_handle(), &fleet, window.label(), &key, reply).await
}

/// Answers the question ask note `id` is about, as Allow and Deny on its
/// macOS notification do. One answered elsewhere counts as answered.
#[tauri::command]
pub async fn notes_answer(app: AppHandle, id: String, allow: bool) -> Result<()> {
    let key: AgentNoticeKey = id
        .parse()
        .map_err(|_| DesktopError::BadRequest("not a question".into()))?;
    match crate::notices::answer_agent(&app, key.clone(), allow).await {
        Ok(()) | Err(DesktopError::Api(ApiError::Conflict { .. } | ApiError::NotFound {})) => {
            crate::notices::history::answered(&app, &key);
            Ok(())
        }
        Err(e) => Err(e),
    }
}

/// What Home shows. A section is `None` when it cannot be listed.
#[derive(Debug, Serialize)]
pub struct HomeView {
    machines: Option<Vec<MachineCard>>,
    running: Option<Vec<RunningRow>>,
    needs_you: Vec<NeedsYou>,
    /// The projects imported onto the user's machines.
    projects: Option<Vec<ProjectRow>>,
}

/// A machine with what it reported and what runs on it.
#[derive(Debug, Serialize)]
struct MachineCard {
    can_host: bool,
    id: String,
    name: String,
    online: bool,
    /// Not on yet, and the mesh is still trying to reach it after it opened
    /// or the link went.
    connecting: bool,
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
    /// or from a node from before them.
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
        id: SessionId,
        agent: String,
        title: Option<String>,
        cwd: String,
        status: &'static str,
        doing: Option<String>,
        /// The first line of its last reply, shown while it is idle, as inline HTML.
        reply_html: Option<String>,
        /// The lines changed in its checkout when its last turn ended.
        changes: Option<LineChanges>,
        started_at: u64,
        last_activity: u64,
        external: bool,
        /// Its subagents that still run.
        subagents: u32,
    },
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum NeedsYou {
    Agent {
        machine_id: String,
        machine_name: String,
        agent: String,
        session: SessionId,
        title: Option<String>,
        question: AgentQuestion,
    },
}

/// What Home is made of, as the mesh answered.
struct HomeParts {
    machines: Result<Vec<Machine>>,
    details: Result<Vec<MachineDetails>>,
    stats: Result<Vec<MachineStats>>,
    sessions: Result<Vec<MachineSession>>,
    projects: Result<Vec<ImportedProject>>,
}

/// Home, from the mesh. The agent sessions wait only as long as `WAIT`, and not at all with rows kept. A call
/// while a read runs shares it.
#[tauri::command]
pub async fn center_home(app: AppHandle, reads: State<'_, HomeReads>) -> Result<SharedHome> {
    reads.read(&app).await
}

/// A Home read, shared by every caller that came while it ran.
type Reading = Shared<BoxFuture<'static, std::result::Result<Arc<HomeView>, Arc<str>>>>;

/// The Home read under way. At start the mesh view prime and the window both ask for Home;
/// they share one read, which lists every machine once.
pub(crate) struct HomeReads {
    flight: Mutex<Flight>,
    /// The mirror's rows, as it pushes them.
    rows: watch::Receiver<Arc<Rows>>,
}

#[derive(Default)]
struct Flight {
    next: u64,
    reading: Option<(u64, Reading)>,
}

impl HomeReads {
    pub(crate) fn new(rows: watch::Receiver<Arc<Rows>>) -> Self {
        Self {
            flight: Mutex::default(),
            rows,
        }
    }

    fn flight(&self) -> MutexGuard<'_, Flight> {
        super::lock(&self.flight)
    }

    /// The all-sessions list pushed beside Home. It lives here so the view layer
    /// reaches the session code only through Home, which already imports it.
    pub(crate) async fn sessions(&self, app: &AppHandle) -> Result<Vec<SessionRow>> {
        agent_sessions(app.clone(), app.state()).await
    }

    /// The moves in the mesh, pushed beside the sessions.
    pub(crate) async fn moves(&self, app: &AppHandle) -> Result<Vec<MoveView>> {
        moves(app.state()).await
    }

    /// Home, from the read under way or a new one.
    pub(crate) async fn read(&self, app: &AppHandle) -> Result<SharedHome> {
        let reading = {
            let mut flight = self.flight();

            match &flight.reading {
                Some((_, reading)) => reading.clone(),
                None => {
                    let id = flight.next;
                    flight.next += 1;
                    let app = app.clone();
                    let rows = self.rows.clone();
                    let reading = tauri::async_runtime::spawn(async move {
                        let home = read_home(&app, rows).await;
                        app.state::<HomeReads>().finished(id);
                        home.map(Arc::new)
                            .map_err(|e| Arc::<str>::from(e.to_string()))
                    })
                    .map(|joined| joined.unwrap_or_else(|e| Err(Arc::from(e.to_string()))))
                    .boxed()
                    .shared();
                    flight.reading = Some((id, reading.clone()));
                    reading
                }
            }
        };

        reading
            .await
            .map(SharedHome)
            .map_err(|e| DesktopError::Other(anyhow::anyhow!("{e}")))
    }

    fn finished(&self, id: u64) {
        let mut flight = self.flight();

        if flight.reading.as_ref().is_some_and(|(held, _)| *held == id) {
            flight.reading = None;
        }
    }

    /// Home changed: the next caller reads again rather than share a read from before.
    pub(crate) fn forget(&self) {
        self.flight().reading = None;
    }
}

/// One Home read, as each of its callers hands it on.
#[derive(Debug)]
pub struct SharedHome(Arc<HomeView>);

impl Serialize for SharedHome {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}

async fn read_home(app: &AppHandle, mut rows: watch::Receiver<Arc<Rows>>) -> Result<HomeView> {
    let fleet = app.state::<Arc<Fleet>>();
    let cache = app.state::<Arc<ViewCache>>();
    let session: FleetState<'_> = app.state();

    // The mesh opens from disk in a moment. Reading before it would replace the Home the
    // window drew from disk with an empty one.
    if fleet.mesh().is_none()
        && let Err(e) = fleet.ready().await
    {
        log::debug!("Home without the mesh: {e}");
    }

    app.state::<Arc<SessionMirror>>().list_now();
    let (_, projects) = join!(
        timeout(WAIT, watched::wait_for(&mut rows, |rows| !rows.is_empty())),
        mesh_imports(&fleet, &session),
    );
    let rows = watched::read(&rows, Arc::clone);
    let sessions = Ok(overlaid(app, &fleet, &rows));
    let status = fleet.status();
    let mut view = home_view(HomeParts {
        machines: Ok(status.machines()),
        details: Ok(status.details()),
        stats: Ok(status.stats()),
        sessions,
        projects: Ok(projects),
    })?;

    let connecting = status.connecting();

    for card in view.machines.iter_mut().flatten() {
        let id = card.id.parse::<EndpointId>().ok();
        card.connecting = id.is_some_and(|id| connecting.contains(&id));
        card.can_host = id.is_none_or(|id| !fleet.mesh().is_some_and(|mesh| mesh.is_observer(id)));
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

/// `None` for a request a machine does not know; any other failure goes to
/// the log.
pub(crate) fn section<T>(what: &str, result: Result<T>) -> Option<T> {
    match result {
        Ok(listed) => Some(listed),
        Err(e) if is_unsupported(&e) => None,
        Err(e) => {
            log::warn!("listing {what} for the command center: {e}");
            None
        }
    }
}

/// Home from the mesh's answers. A failed machine list that is not a plain
/// "not here" means the link itself is down, so the whole Home is stale and
/// the caller keeps what it showed.
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
    let running = sessions
        .as_ref()
        .map(|_| live.iter().map(|s| RunningRow::from(*s)).collect());
    Ok(HomeView {
        machines: section("machines", machines).map(|machines| {
            let details = section("machine details", parts.details).unwrap_or_default();
            let stats = section("machine stats", parts.stats).unwrap_or_default();
            machine_cards(machines, details, stats, &live)
        }),
        running,
        needs_you: needs_you(sessions.iter().flatten()),
        projects: section("projects", parts.projects)
            .map(|projects| projects.into_iter().map(ProjectRow::from).collect()),
    })
}

/// Whether a machine does not know the request or may not take it, as
/// opposed to a failure on the way.
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
                connecting: false,
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
            reply_html: session.reply.as_deref().map(inline_html),
            changes: session.changes,
            started_at: epoch_ms(session.started_at),
            last_activity: epoch_ms(session.last_activity),
            external: session.external,
            subagents: session.subagents,
        }
    }
}

/// Every question an agent session waits on, oldest first, counted from
/// its session's last activity.
pub(crate) fn needs_you<'a>(sessions: impl Iterator<Item = &'a MachineSession>) -> Vec<NeedsYou> {
    let mut waiting: Vec<(u64, NeedsYou)> = sessions
        .filter_map(|session| {
            let question = session.question.as_ref()?;
            Some((
                epoch_ms(session.last_activity),
                NeedsYou::Agent {
                    machine_id: session.machine_id.clone(),
                    machine_name: session.machine_name.clone(),
                    agent: session.agent.clone(),
                    session: session.id.clone(),
                    title: session.title.clone(),
                    question: question.clone(),
                },
            ))
        })
        .collect();
    waiting.sort_by_key(|(since, _)| *since);
    waiting.into_iter().map(|(_, item)| item).collect()
}

#[cfg(test)]
#[path = "../tests/center_home.rs"]
mod tests;
