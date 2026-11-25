//! Sessions started on this device before their machine names them. A
//! start is kept here, with its prompt and images, and the windows see it
//! as a session `start:<uuid>` that is starting. Each start has one runner,
//! which waits until the fleet sees its machine online and sends it, again
//! after each try that could not reach the machine. Once the machine names
//! the session the start goes and the windows hear `agent-started`; one
//! cancelled meanwhile has its session closed instead. A link that drops
//! after the start went, or a machine that refuses it, leaves it failed for
//! the user to retry or close. The starts are a folder in the app data
//! folder, written by its own thread as the outbox's is.

use crate::{
    agents::{
        kept_folder::{KeptFolder, KeptImage, Write, forget_images, image_writes},
        sessions::{SessionRow, Started, closed},
        steer::{PastedImage, Steer, SteerError, uploads},
    },
    beep::epoch_ms,
    error::{DesktopError, Result},
    fleet::{
        Fleet, Machines,
        agents::{self as direct, StartFailed},
    },
};
use beep_mesh::RETRY;
use beep_node::agents::tags::title;
use beep_proto::SessionId;
use beep_proto::{
    AccountName, AgentEntry, AgentEntryKind, AgentSessionView, AgentStatus, AgentSteer, ApiError,
    MachineSession, ModelId, StartAgentSession, StartedAgent, SteerAgentSession, agent_name,
    node::agent::{CheckoutPlan, Effort, Request},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    fmt,
    path::PathBuf,
    str::FromStr,
    sync::{Arc, Mutex, PoisonError},
    time::SystemTime,
};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::{task::spawn_blocking, time::sleep};
use uuid::Uuid;

/// The event the windows hear when a start's machine named its session.
const STARTED: &str = "agent-started";
/// The event the windows hear when their session lists change.
const SESSIONS_CHANGED: &str = "sessions-changed";
/// What a start answers what only a session can do.
const NOT_YET: &str = "It has not started yet.";
const STARTS: &str = "starts.json";

/// A start's id, as the windows know it until its machine names the
/// session: `start:<uuid>`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub(crate) struct StartId(Uuid);

const PREFIX: &str = "start:";

impl StartId {
    fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// What its images' files are named by: no colon, which a Windows file
    /// name cannot hold.
    pub(crate) fn images(self) -> Uuid {
        self.0
    }
}

impl fmt::Display for StartId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{PREFIX}{}", self.0)
    }
}

/// A session id that is not a start's.
#[derive(Debug, thiserror::Error)]
#[error("{0:?} is not a start")]
pub(crate) struct NotAStart(String);

impl FromStr for StartId {
    type Err = NotAStart;

    fn from_str(id: &str) -> std::result::Result<Self, NotAStart> {
        id.strip_prefix(PREFIX)
            .and_then(|uuid| Uuid::parse_str(uuid).ok())
            .map(Self)
            .ok_or_else(|| NotAStart(id.to_owned()))
    }
}

impl TryFrom<String> for StartId {
    type Error = NotAStart;

    fn try_from(id: String) -> std::result::Result<Self, NotAStart> {
        id.parse()
    }
}

impl From<StartId> for String {
    fn from(id: StartId) -> Self {
        id.to_string()
    }
}

/// Where a kept start is.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum Stage {
    /// Not sent yet: just kept, or its machine is off or could not be reached.
    Waiting,
    /// Sent to its machine, which has not named the session yet.
    Starting,
    /// It stays until the user retries or closes it.
    Failed { reason: String },
}

/// One start kept on this device: what its machine is sent, and how it
/// stands.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Start {
    pub(crate) id: StartId,
    pub(crate) machine_id: String,
    pub(crate) machine_name: String,
    pub(crate) agent: String,
    pub(crate) cwd: Option<String>,
    pub(crate) prompt: String,
    pub(crate) model: Option<String>,
    pub(crate) effort: Option<Effort>,
    pub(crate) fast: Option<bool>,
    pub(crate) checkout: Option<CheckoutPlan>,
    pub(crate) images: Vec<KeptImage>,
    pub(crate) assistant: bool,
    /// The named account it runs as; `None` for the agent's default one.
    #[serde(default)]
    pub(crate) account: Option<AccountName>,
    /// The session on the same machine it carries on.
    #[serde(default)]
    pub(crate) continues: Option<SessionId>,
    /// Why Auto picked the machine; empty when the start named it.
    pub(crate) why: String,
    pub(crate) at: SystemTime,
    pub(crate) stage: Stage,
}

impl Start {
    /// The session as the lists show it.
    pub(crate) fn row(&self) -> MachineSession {
        let (status, doing) = match &self.stage {
            Stage::Waiting => (
                AgentStatus::Starting {},
                Some(format!("Waiting for {}", self.machine_name)),
            ),
            Stage::Starting => (
                AgentStatus::Starting {},
                Some(format!(
                    "Starting {} on {}",
                    agent_name(&self.agent),
                    self.machine_name
                )),
            ),
            Stage::Failed { .. } => (AgentStatus::Idle {}, None),
        };
        MachineSession {
            machine_id: self.machine_id.clone(),
            machine_name: self.machine_name.clone(),
            id: Into::<SessionId>::into(self.id.to_string()),
            agent: self.agent.clone(),
            title: title(&self.prompt),
            cwd: self.cwd.clone().unwrap_or_default(),
            status,
            doing,
            started_at: self.at,
            last_activity: self.at,
            external: false,
            question: None,
            subagents: 0,
            model: self.model.as_deref().map(ModelId::new),
            assistant: self.assistant,
            reply: None,
            changes: None,
            account: self.account.clone(),
            persona: None,
            place: None,
            continues: self.continues.clone(),
        }
    }

    /// The session as its page shows it: the prompt as its one entry.
    pub(crate) fn page(&self) -> AgentSessionView {
        let error = match &self.stage {
            Stage::Failed { reason } => Some(reason.clone()),
            Stage::Waiting | Stage::Starting => None,
        };
        AgentSessionView {
            session: self.row(),
            plan: Vec::new(),
            files: Vec::new(),
            last_message: String::new(),
            error,
            queued: 0,
            turns: 0,
            entries: vec![AgentEntry {
                kind: AgentEntryKind::User {},
                text: self.first_message(),
                tool: None,
                sent_at: Some(epoch_ms(self.at)),
            }],
            earlier: 0,
            queued_messages: Vec::new(),
            model: self.model.clone(),
            last_stop: None,
            usage: None,
        }
    }

    /// The prompt, then a line per image, as the page's entry holds it.
    pub(crate) fn first_message(&self) -> String {
        std::iter::once(self.prompt.clone())
            .chain(
                self.images
                    .iter()
                    .map(|image| format!("[image: {}]", image.name)),
            )
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// What its machine is asked, named so it is not picked again.
    fn request(&self) -> StartAgentSession {
        StartAgentSession {
            machine_id: Some(self.machine_id.clone()),
            agent: Some(self.agent.clone()),
            cwd: self.cwd.clone(),
            prompt: self.prompt.clone(),
            needs: Vec::new(),
            model: self.model.clone(),
            effort: self.effort,
            fast: self.fast,
            checkout: self.checkout.clone(),
            assistant: self.assistant,
            account: self.account.clone(),
            continues: self.continues.clone(),
        }
    }
}

/// Why a start that was sent may have started a session anyway.
fn may_be_running(machine_name: &str) -> String {
    format!(
        "The link to {machine_name} dropped while it started. It may be running: check the list, or retry."
    )
}

/// The kept starts, and which have a runner.
#[derive(Default)]
pub(crate) struct Held {
    starts: Vec<Start>,
    running: HashSet<StartId>,
    /// Folder changes not yet handed to the writer, and whether the
    /// starts' file changed.
    writes: Vec<Write>,
    dirty: bool,
}

impl Held {
    fn from_starts(mut starts: Vec<Start>) -> Self {
        // One sent when the app quit may have started on its machine.
        for start in &mut starts {
            if start.stage == Stage::Starting {
                start.stage = Stage::Failed {
                    reason: may_be_running(&start.machine_name),
                };
            }
        }

        Self {
            starts,
            ..Self::default()
        }
    }

    /// Keeps `start`, with a runner the caller starts.
    pub(crate) fn keep(&mut self, start: Start) {
        self.writes
            .extend(image_writes(start.id.images(), &start.images));
        self.running.insert(start.id);
        self.starts.push(start);
        self.dirty = true;
    }

    fn find(&mut self, id: StartId) -> Option<&mut Start> {
        self.starts.iter_mut().find(|start| start.id == id)
    }

    /// Start `id` for its runner to wait on its machine; `None`, which ends
    /// the runner, when it is gone or failed.
    pub(crate) fn runnable(&mut self, id: StartId) -> Option<Start> {
        let start = self
            .starts
            .iter()
            .find(|start| start.id == id && !matches!(start.stage, Stage::Failed { .. }))
            .cloned();

        if start.is_none() {
            self.running.remove(&id);
        }

        start
    }

    /// Start `id`, marked as starting; `None`, which ends the runner, when
    /// it went or failed meanwhile.
    pub(crate) fn begin(&mut self, id: StartId) -> Option<Start> {
        let start = self.runnable(id)?;
        self.find(id)?.stage = Stage::Starting;
        self.dirty = true;
        Some(Start {
            stage: Stage::Starting,
            ..start
        })
    }

    /// Start `id` could not reach its machine and waits for it; false when
    /// it went meanwhile.
    pub(crate) fn wait(&mut self, id: StartId) -> bool {
        let Some(start) = self.find(id) else {
            self.running.remove(&id);
            return false;
        };
        start.stage = Stage::Waiting;
        self.dirty = true;
        true
    }

    /// Start `id` failed for `reason`, and its runner ends; false when it
    /// went meanwhile.
    pub(crate) fn fail(&mut self, id: StartId, reason: String) -> bool {
        self.running.remove(&id);
        let Some(start) = self.find(id) else {
            return false;
        };
        start.stage = Stage::Failed { reason };
        self.dirty = true;
        true
    }

    /// A failed start waits again; true when the caller starts its runner.
    pub(crate) fn retry(&mut self, id: StartId) -> Result<bool> {
        let start = self
            .find(id)
            .ok_or_else(|| DesktopError::from(ApiError::NotFound {}))?;

        if !matches!(start.stage, Stage::Failed { .. }) {
            return Err(DesktopError::BadRequest("It is starting already.".into()));
        }

        start.stage = Stage::Waiting;
        self.dirty = true;
        Ok(self.running.insert(id))
    }

    /// Takes start `id` out with its images.
    pub(crate) fn remove(&mut self, id: StartId) -> Option<Start> {
        let at = self.starts.iter().position(|start| start.id == id)?;
        let start = self.starts.remove(at);
        self.dirty = true;
        self.writes
            .extend(forget_images(start.id.images(), start.images.len()));
        Some(start)
    }

    /// The starts waiting for their machine, for runners to start.
    pub(crate) fn waiting(&mut self) -> Vec<StartId> {
        let ids: Vec<StartId> = self
            .starts
            .iter()
            .filter(|start| start.stage == Stage::Waiting)
            .map(|start| start.id)
            .collect();
        ids.into_iter()
            .filter(|id| self.running.insert(*id))
            .collect()
    }

    pub(crate) fn get(&self, id: StartId) -> Option<&Start> {
        self.starts.iter().find(|start| start.id == id)
    }

    pub(crate) fn rows(&self) -> Vec<MachineSession> {
        self.starts.iter().map(Start::row).collect()
    }
}

/// The kept starts and the thread that writes them.
pub struct Starts {
    held: Mutex<Held>,
    folder: KeptFolder,
}

impl Starts {
    /// The starts kept in `dir`, or none.
    pub fn load(dir: PathBuf, cache: PathBuf) -> Self {
        let folder = KeptFolder::open(dir, cache, STARTS, "starts");
        let mut starts: Vec<Start> = folder.read();

        for start in &mut starts {
            if !folder.read_images(start.id.images(), &mut start.images) {
                start.stage = Stage::Failed {
                    reason: "An image of this start could not be read.".into(),
                };
            }
        }

        Self {
            held: Mutex::new(Held::from_starts(starts)),
            folder,
        }
    }

    /// Changes the held starts with `change`, then has what changed
    /// written: the starts' file, which leaves out image bytes, serialized
    /// and queued under the lock so no waiting image is copied and the
    /// files land in the order of the changes.
    pub(crate) fn with<T>(&self, change: impl FnOnce(&mut Held) -> T) -> T {
        let mut held = self.held.lock().unwrap_or_else(PoisonError::into_inner);
        let out = change(&mut held);
        let file = std::mem::take(&mut held.dirty)
            .then(|| self.folder.records(&held.starts))
            .flatten();
        let writes: Vec<Write> = std::mem::take(&mut held.writes)
            .into_iter()
            .chain(file)
            .collect();

        // Queued before the lock goes, so a later change cannot be queued first.
        self.folder.write(writes);

        out
    }

    pub(crate) fn read<T>(&self, read: impl FnOnce(&Held) -> T) -> T {
        read(&self.held.lock().unwrap_or_else(PoisonError::into_inner))
    }

    /// The kept starts as the session lists show them.
    pub(crate) fn rows(&self) -> Vec<MachineSession> {
        self.read(Held::rows)
    }

    /// Returns once what changed so far is on disk.
    pub fn sync(&self) {
        self.folder.sync();
    }
}

/// What became of one try at a start.
#[derive(Debug)]
pub(crate) enum Tried {
    Started(Box<StartedAgent>),
    /// The machine could not be reached: it never had the start.
    Unreached,
    /// The link dropped after the start went.
    Dropped,
    Refused(String),
}

/// What a runner needs of the machine and the windows.
pub(crate) trait Link {
    /// Returns once the fleet sees the start's machine online.
    async fn online(&mut self, start: &Start);
    /// Sends the start to its machine.
    async fn open(&mut self, start: &Start) -> Tried;
    /// Waits before the next try, longer each time.
    async fn pause(&mut self);
    /// Tells the windows the start changed.
    fn changed(&mut self, id: StartId);
    /// The machine named the session; the start is gone.
    async fn started(&mut self, start: Start, started: StartedAgent);
    /// The machine named the session of a start cancelled meanwhile: it is
    /// closed there, and the windows never hear of it.
    async fn close(&mut self, started: StartedAgent);
}

/// Runs start `id` until its machine names the session or it fails: once
/// the machine is online it is sent, and one that could not reach the
/// machine waits for it again. One the link dropped on after it went is not
/// sent again, since the session may exist.
pub(crate) async fn run(starts: &Arc<Starts>, id: StartId, link: &mut impl Link) {
    loop {
        let Some(start) = starts.with(|held| held.runnable(id)) else {
            return;
        };
        link.online(&start).await;

        let Some(start) = starts.with(|held| held.begin(id)) else {
            return;
        };
        link.changed(id);
        // On disk as starting before it leaves: the app cut short after
        // this does not send it a second time.
        let kept = starts.clone();

        if let Err(e) = spawn_blocking(move || kept.sync()).await {
            log::warn!("keeping a start before it is sent: {e}");
        }

        let reason = match link.open(&start).await {
            Tried::Started(started) => {
                let kept = starts.with(|held| {
                    held.running.remove(&id);
                    held.remove(id)
                });

                match kept {
                    Some(_) => link.started(start, *started).await,
                    None => link.close(*started).await,
                }

                return;
            }
            Tried::Unreached => {
                if !starts.with(|held| held.wait(id)) {
                    return;
                }

                link.changed(id);
                link.pause().await;
                continue;
            }
            Tried::Dropped => may_be_running(&start.machine_name),
            Tried::Refused(reason) => reason,
        };

        if starts.with(|held| held.fail(id, reason)) {
            link.changed(id);
        }

        return;
    }
}

/// The started event's payload.
#[derive(Clone, Debug, Serialize)]
struct AgentStarted {
    start: String,
    session: SessionRow,
    first_message: String,
}

/// A start's machine, as its runner reaches it.
struct MeshLink {
    app: AppHandle,
}

impl Link for MeshLink {
    async fn online(&mut self, start: &Start) {
        let fleet = self.app.state::<Arc<Fleet>>().inner().clone();

        // One no longer in the mesh is never online; `open` fails it.
        if let Some(machine) = fleet.member(&start.machine_id) {
            crate::fleet::alerts::until_online(&fleet, machine).await;
        }
    }

    async fn open(&mut self, start: &Start) -> Tried {
        let fleet = self.app.state::<Arc<Fleet>>().inner().clone();

        if fleet.member(&start.machine_id).is_none() {
            return Tried::Refused(format!("{} is no longer in the mesh.", start.machine_name));
        }

        let images = start
            .images
            .iter()
            .cloned()
            .map(PastedImage::from)
            .collect();
        let planned = match uploads(images) {
            Ok(images) => direct::plan(&fleet, start.request(), images).await,
            Err(e) => Err(e),
        };
        let planned = match planned {
            Ok(planned) => planned,
            Err(e) => return Tried::Refused(e.to_string()),
        };

        match direct::open(&fleet, planned).await {
            Ok(started) => Tried::Started(Box::new(started)),
            Err(StartFailed::Unreached(_)) => Tried::Unreached,
            Err(StartFailed::Dropped(_)) => Tried::Dropped,
            Err(StartFailed::Refused(e)) => Tried::Refused(e.to_string()),
        }
    }

    async fn pause(&mut self) {
        sleep(RETRY).await;
    }

    fn changed(&mut self, id: StartId) {
        sessions_changed(&self.app, id);
    }

    async fn started(&mut self, start: Start, started: StartedAgent) {
        // The windows hear first: until then a read of the start finds it gone.
        let event = AgentStarted {
            start: start.id.to_string(),
            first_message: start.first_message(),
            session: started.session.clone().into(),
        };

        if let Err(e) = self.app.emit(STARTED, event) {
            log::warn!("could not tell the windows a session started: {e}");
        }

        sessions_changed(&self.app, start.id);
    }

    async fn close(&mut self, started: StartedAgent) {
        let fleet = self.app.state::<Arc<Fleet>>().inner().clone();
        let session = started.session;
        let Some(machine) = fleet.member(&session.machine_id) else {
            return;
        };
        let close = SteerAgentSession {
            machine_id: session.machine_id.clone(),
            agent: session.agent,
            session: session.id.clone(),
            steer: AgentSteer::Close {},
        };

        match direct::steer(&fleet, machine, close).await {
            Ok(_) => closed(&self.app, &session.machine_id, &session.id),
            Err(e) => log::warn!("closing the session of a cancelled start: {e}"),
        }
    }
}

/// Has the windows read their session lists again.
fn sessions_changed(app: &AppHandle, id: StartId) {
    if let Err(e) = app.emit(SESSIONS_CHANGED, id.to_string()) {
        log::warn!("could not tell the windows a start changed: {e}");
    }

    crate::fleet::home_signal::poke(app);
}

/// Runs the runner of start `id`, which the caller marked as running.
fn spawn(app: &AppHandle, id: StartId) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let starts = app.state::<Arc<Starts>>().inner().clone();
        let mut link = MeshLink { app: app.clone() };
        run(&starts, id, &mut link).await;
    });
}

/// Runs the starts kept from the last run that wait for their machine, once
/// the mesh is open.
pub fn resume(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let fleet = app.state::<Arc<Fleet>>().inner().clone();

        // Without a mesh no machine is reached: the starts wait, shown.
        if let Err(e) = fleet.ready().await {
            log::warn!("the kept starts wait: {e}");
            return;
        }

        let starts = app.state::<Arc<Starts>>().inner().clone();

        for id in starts.with(Held::waiting) {
            spawn(&app, id);
        }
    });
}

/// What `agent_steer` does to a start: closing, cancelling or stopping it
/// takes it out; nothing else can be done to it yet.
pub(crate) fn steer(
    app: &AppHandle,
    id: StartId,
    steer: &Steer,
) -> std::result::Result<Vec<String>, SteerError> {
    if !matches!(steer, Steer::Close | Steer::Cancel | Steer::StopAll) {
        return Err(SteerError::Failed {
            message: NOT_YET.into(),
        });
    }

    if app
        .state::<Arc<Starts>>()
        .with(|held| held.remove(id))
        .is_some()
    {
        sessions_changed(app, id);
    }

    Ok(Vec::new())
}

/// Fails a follow of a start at once: its page reads `agent_view`.
pub(crate) fn not_followed(session: &SessionId) -> Result<()> {
    match session.as_ref().parse::<StartId>() {
        Ok(_) => Err(DesktopError::BadRequest(NOT_YET.into())),
        Err(_) => Ok(()),
    }
}

/// Starts a session the way `agent_start` does, without waiting for its
/// machine: the start is checked and kept on this device, its runner sends
/// it once the machine is online, and the session `start:<uuid>` it answers
/// with stands in until the machine names it (`agent-started`).
// The arguments are the fields of the window's `invoke` call.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn agent_begin(
    app: AppHandle,
    fleet: Machines<'_>,
    starts: State<'_, Arc<Starts>>,
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
    continues: Option<SessionId>,
) -> Result<Started> {
    let account = account.as_deref().map(AccountName::from_str).transpose()?;
    let pasted = images.unwrap_or_default();
    let checked = uploads(pasted.clone())?;

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
        continues,
    };
    let planned = direct::plan_fast(&fleet, request, checked).await?;
    let Request::Start {
        cwd,
        prompt,
        model,
        effort,
        fast,
        checkout,
        assistant,
        continues,
        ..
    } = planned.first
    else {
        return Err(ApiError::Internal {}.into());
    };
    let start = Start {
        id: StartId::new(),
        machine_id: planned.machine.to_string(),
        machine_name: fleet.name_of(planned.machine),
        agent: planned.agent,
        cwd,
        prompt,
        model,
        effort,
        fast,
        checkout,
        images: pasted.into_iter().map(KeptImage::from).collect(),
        assistant,
        account: planned.account,
        continues,
        why: planned.why,
        at: SystemTime::now(),
        // Its runner marks it starting, once that is on disk.
        stage: Stage::Waiting,
    };
    let started = Started {
        session: start.row().into(),
        why: start.why.clone(),
    };
    let id = start.id;
    starts.with(|held| held.keep(start));
    spawn(&app, id);
    sessions_changed(&app, id);
    Ok(started)
}

/// A failed start waits for its machine again, and its runner runs.
#[tauri::command]
pub fn agent_begin_retry(
    app: AppHandle,
    starts: State<'_, Arc<Starts>>,
    session: SessionId,
) -> Result<()> {
    let id: StartId = session
        .as_ref()
        .parse()
        .map_err(|e: NotAStart| DesktopError::BadRequest(e.to_string()))?;

    if starts.with(|held| held.retry(id))? {
        spawn(&app, id);
    }

    sessions_changed(&app, id);
    Ok(())
}

#[cfg(test)]
#[path = "../tests/agents_starts.rs"]
mod tests;
