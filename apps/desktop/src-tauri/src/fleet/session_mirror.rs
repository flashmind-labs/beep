//! The session mirror: one actor owns every machine's session rows. Readers
//! take the rows it publishes; a machine's push, a listing's answer and each
//! local edit reach it as one message. Asking a machine runs off its loop in
//! a `JoinSet` and comes back as a message.

use super::{Fleet, agents as direct, event_views};
use crate::{
    agents::starts::StartId,
    center::{
        cache_view,
        home::HOME_CHANGED,
        view_cache::{View, ViewCache},
    },
    palette::lists::PaletteCache,
};
use beep_proto::{
    AgentStatus, MachineSession, SessionId,
    node::{
        agent::{ExternalSession, Presence},
        events::Topic,
    },
    watched,
};
use iroh::EndpointId;
use std::{
    collections::{HashMap, HashSet},
    future::Future,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, UNIX_EPOCH},
};
use tauri::{AppHandle, Emitter, Manager};
use tokio::{
    sync::{mpsc, watch},
    task::JoinSet,
    time::{Instant, sleep, timeout},
};
use tokio_util::task::AbortOnDropHandle;

/// How long a machine's rows stand before a read lists it again. Until then
/// its pushes replace its rows alone. Nothing runs while nobody reads.
const FRESH: Duration = Duration::from_secs(30);
/// The first wait before a machine that did not list its sessions is asked
/// again; it doubles up to `RELIST_LAST`.
const RELIST_FIRST: Duration = Duration::from_secs(1);
const RELIST_LAST: Duration = Duration::from_secs(16);
/// How long the first read of a launch waits for this Mac's node.
const LOCAL_WAIT: Duration = Duration::from_secs(6);
/// How long a changed set of rows waits before the views file takes it, so a
/// burst of live reports writes once.
const KEEP_AFTER: Duration = Duration::from_secs(2);
const WAITING: usize = 256;

/// One machine's rows.
#[derive(Clone, Default)]
pub(crate) struct Machine {
    /// Beep's own sessions on it.
    beep: Vec<MachineSession>,
    /// The sessions started outside Beep, and the assistant's ended
    /// conversations, found by scanning it.
    scans: Vec<MachineSession>,
    source: Source,
}

/// Where a machine's Beep rows come from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Source {
    /// Kept from the last run, or from before the machine stopped answering.
    #[default]
    Kept,
    /// Listed by the machine in this run.
    Live,
}

/// Every machine's rows, as the mirror publishes them.
#[derive(Clone, Default)]
pub(crate) struct Rows {
    /// Counts the changes to the rows.
    pub(crate) revision: u64,
    machines: HashMap<EndpointId, Machine>,
}

impl Rows {
    /// The rows a previous run kept: a scan's rows are those of an outside session.
    fn kept(kept: Vec<MachineSession>) -> Self {
        let mut rows = Self::default();

        for session in kept {
            let Ok(machine) = session.machine_id.parse::<EndpointId>() else {
                continue;
            };
            let held = rows.machines.entry(machine).or_default();

            if session.external {
                held.scans.push(session);
            } else {
                held.beep.push(session);
            }
        }

        rows
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = &MachineSession> {
        self.machines
            .values()
            .flat_map(|machine| machine.beep.iter().chain(&machine.scans))
    }

    /// No machine was read or kept yet.
    pub(crate) fn is_empty(&self) -> bool {
        self.machines.is_empty()
    }

    /// Whether `machine` listed its Beep sessions in this run.
    fn live(&self, machine: EndpointId) -> bool {
        self.machines
            .get(&machine)
            .is_some_and(|machine| machine.source == Source::Live)
    }
}

/// What the mirror needs of the machines and the windows.
pub(crate) trait Outside: Send + Sync + 'static {
    /// The machines of the mesh that are online now, this Mac among them.
    fn online(&self) -> Vec<EndpointId>;
    /// Beep's sessions on `machine`; `None` when it did not answer.
    fn list(&self, machine: EndpointId)
    -> impl Future<Output = Option<Vec<MachineSession>>> + Send;
    /// What a scan of `machine` finds; `None` when it did not answer.
    fn scan(&self, machine: EndpointId)
    -> impl Future<Output = Option<Vec<MachineSession>>> + Send;
    /// Tells the windows, and the lists that follow, what changed.
    fn drawn(&self, drawn: Drawn);
}

/// A change in what the windows draw.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Drawn {
    /// A machine's push was listed again.
    Pushed(EndpointId),
    /// A read listed a machine again and its rows differ.
    Listed,
    /// A local edit of this session.
    Session(SessionId),
}

enum Message {
    /// Someone reads the rows: what is older than `FRESH` is listed again.
    Read,
    /// The next read lists everything again.
    Stale,
    /// A machine's push: its rows are listed again.
    Changed(EndpointId),
    /// A listing of one machine came back.
    Listed {
        machine: EndpointId,
        rows: Option<Vec<MachineSession>>,
    },
    /// A scan of one machine came back.
    Scanned {
        machine: EndpointId,
        rows: Option<Vec<MachineSession>>,
    },
    Deleted {
        machine_id: String,
        session: SessionId,
    },
    /// The machine refused a delete: it is listed again.
    DeleteFailed(EndpointId),
    Closed {
        machine_id: String,
        session: SessionId,
    },
    Answered {
        machine_id: String,
        session: SessionId,
        question: Option<u32>,
    },
    /// A session moved to another folder: its machine is listed again.
    Moved(EndpointId),
    /// One external session's live state.
    Reported {
        machine_id: String,
        report: Box<ExternalSession>,
    },
}

/// The handle readers and edits use.
pub(crate) struct SessionMirror {
    inbox: mpsc::Sender<Message>,
    rows: watch::Receiver<Arc<Rows>>,
    asked: AtomicBool,
    _task: AbortOnDropHandle<()>,
}

impl SessionMirror {
    /// Starts the mirror on the running runtime, with the rows `kept` from
    /// the last run.
    pub(crate) fn start(outside: impl Outside, kept: Vec<MachineSession>) -> Self {
        let (inbox, messages) = mpsc::channel(WAITING);
        let (rows, follow) = watch::channel(Arc::new(Rows::kept(kept)));
        let actor = Actor {
            outside: Arc::new(outside),
            rows,
            reads: HashMap::new(),
            relisting: HashMap::new(),
            scanning: HashSet::new(),
            tasks: JoinSet::new(),
        };

        Self {
            inbox,
            rows: follow,
            asked: AtomicBool::new(false),
            _task: AbortOnDropHandle::new(tokio::spawn(actor.run(messages))),
        }
    }

    /// The rows now, and a listing of what is older than `FRESH`.
    pub(crate) fn rows(&self) -> Arc<Rows> {
        // A full inbox holds a read already.
        let _ = self.inbox.try_send(Message::Read);
        self.current()
    }

    fn current(&self) -> Arc<Rows> {
        watched::read(&self.rows, Arc::clone)
    }

    /// As `rows`, once `ready` holds of them or `wait` has passed.
    pub(crate) async fn rows_when(
        &self,
        ready: impl Fn(&Rows) -> bool,
        wait: Duration,
    ) -> Arc<Rows> {
        let _ = self.inbox.try_send(Message::Read);
        let mut rows = self.rows.clone();

        // Either way the rows now are the answer.
        let _ = timeout(wait, watched::wait_for(&mut rows, |rows| ready(rows))).await;
        self.current()
    }

    /// The rows for a reader that cannot wait long: the first call of a launch
    /// waits for this Mac's node to list, then they are as they stand.
    pub(crate) async fn rows_of_launch(&self, fleet: &Fleet) -> Arc<Rows> {
        if self.asked.swap(true, Ordering::AcqRel) {
            return self.rows();
        }

        let me = fleet.id();
        // Without the mesh open this Mac is not online yet.
        let _ = fleet.ready().await;

        if fleet.online().contains(&me) {
            return self.rows_when(|rows| rows.live(me), LOCAL_WAIT).await;
        }

        self.rows()
    }

    /// A subscription to the rows, for what follows them.
    pub(crate) fn watch(&self) -> watch::Receiver<Arc<Rows>> {
        self.rows.clone()
    }

    /// `machine` pushed a change: its rows are listed again.
    pub(crate) fn changed(&self, machine: EndpointId) {
        self.send(Message::Changed(machine));
    }

    /// The next read lists every machine again.
    pub(crate) fn stale(&self) {
        self.send(Message::Stale);
    }

    /// The session is deleted on its machine.
    pub(crate) fn deleted(&self, machine_id: &str, session: &SessionId) {
        self.send(Message::Deleted {
            machine_id: machine_id.to_owned(),
            session: session.clone(),
        });
    }

    pub(crate) fn delete_failed(&self, machine: EndpointId) {
        self.send(Message::DeleteFailed(machine));
    }

    /// The session ended on its machine.
    pub(crate) fn closed(&self, machine_id: &str, session: &SessionId) {
        self.send(Message::Closed {
            machine_id: machine_id.to_owned(),
            session: session.clone(),
        });
    }

    /// The question is answered, or every question of the session when `None`.
    pub(crate) fn answered(&self, machine_id: &str, session: &SessionId, question: Option<u32>) {
        self.send(Message::Answered {
            machine_id: machine_id.to_owned(),
            session: session.clone(),
            question,
        });
    }

    pub(crate) fn moved(&self, machine: EndpointId) {
        self.send(Message::Moved(machine));
    }

    pub(crate) fn reported(&self, machine_id: &str, report: &ExternalSession) {
        self.send(Message::Reported {
            machine_id: machine_id.to_owned(),
            report: Box::new(report.clone()),
        });
    }

    fn send(&self, message: Message) {
        if let Err(e) = self.inbox.try_send(message) {
            log::warn!("the session mirror did not take a change: {e}");
        }
    }
}

/// When a machine's rows were last read, whatever the answer.
#[derive(Default)]
struct Reads {
    beep: Option<Instant>,
    scans: Option<Instant>,
}

/// A machine whose sessions are listed now.
struct Relist {
    /// Another change came meanwhile: list once more.
    again: bool,
    /// A push, not a read, asked for it: the lists that follow hear of it.
    loud: bool,
    /// The wait before the next try, should the machine not answer.
    pause: Duration,
}

struct Actor<O> {
    outside: Arc<O>,
    rows: watch::Sender<Arc<Rows>>,
    reads: HashMap<EndpointId, Reads>,
    relisting: HashMap<EndpointId, Relist>,
    scanning: HashSet<EndpointId>,
    tasks: JoinSet<Message>,
}

impl<O: Outside> Actor<O> {
    async fn run(mut self, mut inbox: mpsc::Receiver<Message>) {
        loop {
            let message = tokio::select! {
                message = inbox.recv() => match message {
                    Some(message) => message,
                    None => return,
                },
                Some(done) = self.tasks.join_next() => match done {
                    Ok(message) => message,
                    Err(e) => {
                        // Its machine is asked again by the next read.
                        log::warn!("asking a machine for its sessions stopped: {e}");
                        self.relisting.clear();
                        self.scanning.clear();
                        continue;
                    }
                },
            };

            self.handle(message);
        }
    }

    fn handle(&mut self, message: Message) {
        match message {
            Message::Read => self.read(),
            Message::Stale => self.reads.clear(),
            Message::Changed(machine) => self.relist(machine, true),
            Message::Listed { machine, rows } => self.listed(machine, rows),
            Message::Scanned { machine, rows } => self.scanned(machine, rows),
            Message::DeleteFailed(machine) | Message::Moved(machine) => {
                self.relist(machine, true);
                self.stale_scans();
            }
            Message::Deleted {
                machine_id,
                session,
            } => {
                self.edit(|rows| {
                    let before = rows.len();
                    rows.retain(|row| !is(row, &machine_id, &session));
                    rows.len() != before
                });
                self.outside.drawn(Drawn::Session(session));
            }
            Message::Closed {
                machine_id,
                session,
            } => {
                self.edit(|rows| {
                    let mut changed = false;

                    for row in rows.iter_mut().filter(|row| is(row, &machine_id, &session)) {
                        changed |= row.status != AgentStatus::Ended {}
                            || row.doing.is_some()
                            || row.question.is_some();
                        row.status = AgentStatus::Ended {};
                        row.doing = None;
                        row.question = None;
                    }

                    changed
                });
                // The scan runs again on the next read, which may find it gone.
                self.stale_scans();
                self.outside.drawn(Drawn::Session(session));
            }
            Message::Answered {
                machine_id,
                session,
                question,
            } => {
                self.edit(|rows| answer(rows, &machine_id, &session, question));
                self.outside.drawn(Drawn::Session(session));
            }
            Message::Reported { machine_id, report } => {
                self.edit(|rows| report_in(rows, &machine_id, &report));
            }
        }
    }

    /// Lists and scans each online machine whose rows are older than `FRESH`.
    fn read(&mut self) {
        let now = Instant::now();

        for machine in self.outside.online() {
            let reads = self.reads.entry(machine).or_default();
            let list = reads.beep.is_none_or(|at| now - at >= FRESH);
            let scan = reads.scans.is_none_or(|at| now - at >= FRESH);

            if list {
                self.relist(machine, false);
            }

            if scan {
                self.scan(machine);
            }
        }
    }

    /// Lists `machine`, one listing at a time. A push that comes while it is
    /// listed has it listed once more; a read does not.
    fn relist(&mut self, machine: EndpointId, push: bool) {
        match self.relisting.get_mut(&machine) {
            Some(relist) => {
                relist.again |= push;
                relist.loud |= push;
            }
            None => {
                self.relisting.insert(
                    machine,
                    Relist {
                        again: false,
                        loud: push,
                        pause: RELIST_FIRST,
                    },
                );
                self.list(machine, Duration::ZERO);
            }
        }
    }

    fn list(&mut self, machine: EndpointId, after: Duration) {
        let outside = self.outside.clone();
        self.tasks.spawn(async move {
            if !after.is_zero() {
                sleep(after).await;
            }

            Message::Listed {
                machine,
                rows: outside.list(machine).await,
            }
        });
    }

    fn scan(&mut self, machine: EndpointId) {
        if !self.scanning.insert(machine) {
            return;
        }

        let outside = self.outside.clone();
        self.tasks.spawn(async move {
            Message::Scanned {
                machine,
                rows: outside.scan(machine).await,
            }
        });
    }

    fn listed(&mut self, machine: EndpointId, rows: Option<Vec<MachineSession>>) {
        let mut relist = self.relisting.remove(&machine).unwrap_or(Relist {
            again: false,
            loud: false,
            pause: RELIST_FIRST,
        });
        let mut drawn = false;

        match rows {
            Some(rows) => drawn = self.replace(machine, rows, false),
            // One that does not answer keeps its rows and is asked again while
            // it is online, waiting longer each time.
            None if relist.pause <= RELIST_LAST && self.outside.online().contains(&machine) => {
                self.list(machine, relist.pause);
                relist.pause *= 2;
                self.relisting.insert(machine, relist);
                return;
            }
            None => log::info!("the sessions of {machine} were not listed"),
        }

        self.reads.entry(machine).or_default().beep = Some(Instant::now());

        if relist.loud {
            relist.loud = false;
            self.outside.drawn(Drawn::Pushed(machine));
        } else if drawn {
            self.outside.drawn(Drawn::Listed);
        }

        if relist.again {
            relist.again = false;
            self.list(machine, Duration::ZERO);
            self.relisting.insert(machine, relist);
        }
    }

    fn scanned(&mut self, machine: EndpointId, rows: Option<Vec<MachineSession>>) {
        self.scanning.remove(&machine);
        self.reads.entry(machine).or_default().scans = Some(Instant::now());

        if let Some(rows) = rows
            && self.replace(machine, rows, true)
        {
            self.outside.drawn(Drawn::Listed);
        }
    }

    /// Takes `rows` as all of `machine`'s, for its Beep sessions or its scan.
    /// True when what the windows draw differs.
    fn replace(&self, machine: EndpointId, rows: Vec<MachineSession>, scans: bool) -> bool {
        let mut drawn = false;

        self.revise(|all| {
            let held = all.machines.entry(machine).or_default();
            let was = if scans {
                &mut held.scans
            } else {
                &mut held.beep
            };
            drawn = was.len() != rows.len() || was.iter().zip(&rows).any(|(a, b)| !same(a, b));
            let changed = *was != rows || (!scans && held.source != Source::Live);
            *was = rows;

            if !scans {
                held.source = Source::Live;
            }

            changed
        });

        drawn
    }

    /// Changes every machine's rows, as `change` says, and publishes them.
    fn edit(&self, change: impl Fn(&mut Vec<MachineSession>) -> bool) {
        self.revise(|rows| {
            let mut changed = false;

            for machine in rows.machines.values_mut() {
                changed |= change(&mut machine.beep);
                changed |= change(&mut machine.scans);
            }

            changed
        });
    }

    /// Publishes the rows when `change` says it changed them.
    fn revise(&self, change: impl FnOnce(&mut Rows) -> bool) {
        self.rows.send_if_modified(|rows| {
            let rows = Arc::make_mut(rows);
            let changed = change(rows);
            rows.revision += u64::from(changed);
            changed
        });
    }

    fn stale_scans(&mut self) {
        for reads in self.reads.values_mut() {
            reads.scans = None;
        }
    }
}

fn is(row: &MachineSession, machine_id: &str, session: &SessionId) -> bool {
    row.machine_id == machine_id && row.id == *session
}

/// Clears the question a session's answer settled, or every one, and puts
/// the session back to work.
fn answer(
    rows: &mut [MachineSession],
    machine_id: &str,
    session: &SessionId,
    question: Option<u32>,
) -> bool {
    let mut changed = false;

    for row in rows.iter_mut().filter(|row| {
        is(row, machine_id, session)
            && question.is_none_or(|id| row.question.as_ref().is_some_and(|q| q.id == id))
    }) {
        changed |= row.question.take().is_some();

        if matches!(row.status, AgentStatus::Asking {}) {
            row.status = AgentStatus::Working {};
            changed = true;
        }
    }

    changed
}

/// Takes one external session's live report into its row, so a change to one
/// session does not list every machine again.
fn report_in(rows: &mut [MachineSession], machine_id: &str, report: &ExternalSession) -> bool {
    let status = match report.presence {
        Presence::Working => AgentStatus::Working {},
        Presence::Waiting => AgentStatus::Waiting {},
        Presence::Running | Presence::Recent => AgentStatus::Running {},
        Presence::Stopped => AgentStatus::Ended {},
        _ => return false,
    };
    let at = UNIX_EPOCH + Duration::from_secs(report.last_activity);
    let Some(row) = rows.iter_mut().find(|row| {
        row.external
            && row.machine_id == machine_id
            && row.id == report.id
            && row.agent == report.agent
    }) else {
        return false;
    };
    let before = row.clone();
    row.status = status;
    row.last_activity = row.last_activity.max(at);

    if report.title.is_some() {
        row.title.clone_from(&report.title);
    }

    *row != before
}

/// Whether two listings of a session draw the same, which leaves out what
/// moves on every read.
fn same(old: &MachineSession, new: &MachineSession) -> bool {
    old.machine_id == new.machine_id
        && old.id == new.id
        && old.status == new.status
        && old.question.as_ref().map(|q| q.id) == new.question.as_ref().map(|q| q.id)
}

/// The mirror's machines and windows, in the app.
pub(crate) struct Desk {
    pub(crate) app: AppHandle,
    pub(crate) fleet: Arc<Fleet>,
}

impl Outside for Desk {
    fn online(&self) -> Vec<EndpointId> {
        self.fleet.online()
    }

    async fn list(&self, machine: EndpointId) -> Option<Vec<MachineSession>> {
        direct::sessions_of(&self.fleet, machine).await
    }

    async fn scan(&self, machine: EndpointId) -> Option<Vec<MachineSession>> {
        direct::scanned_sessions_of(&self.fleet, machine).await
    }

    fn drawn(&self, drawn: Drawn) {
        let app = &self.app;
        let told = match drawn {
            Drawn::Pushed(machine) => {
                app.state::<super::session_lists::SessionLists>()
                    .stale(machine);
                app.state::<PaletteCache>().sessions_changed();
                event_views::sessions_changed(app.clone(), Topic::new("agents"));
                Ok(())
            }
            Drawn::Listed => app.emit(HOME_CHANGED, ()),
            Drawn::Session(session) => app.emit("sessions-changed", session),
        };

        if let Err(e) = told {
            log::warn!("telling the windows the sessions changed: {e}");
        }
    }
}

/// The rows the last run kept in `cache`, leaving out the starts the app
/// holds itself.
pub(crate) fn kept(cache: &ViewCache) -> Vec<MachineSession> {
    match cache
        .get(View::AgentSessions)
        .map(serde_json::from_value::<Vec<MachineSession>>)
    {
        Some(Ok(mut rows)) => {
            rows.retain(|row| row.id.as_ref().parse::<StartId>().is_err());
            rows
        }
        Some(Err(e)) => {
            // Only a head start: the machines' listings replace it.
            log::info!("the saved agent sessions do not parse: {e}");
            Vec::new()
        }
        None => Vec::new(),
    }
}

/// Keeps the rows in `cache` for the next start, a moment after each change.
pub(crate) async fn keep(mut rows: watch::Receiver<Arc<Rows>>, cache: Arc<ViewCache>) {
    while rows.changed().await.is_ok() {
        sleep(KEEP_AFTER).await;
        let all: Vec<MachineSession> =
            watched::read_and_update(&mut rows, |rows| rows.iter().cloned().collect());
        cache_view(&cache, View::AgentSessions, &all);
    }
}

#[cfg(test)]
#[path = "../tests/fleet_session_mirror.rs"]
mod tests;
