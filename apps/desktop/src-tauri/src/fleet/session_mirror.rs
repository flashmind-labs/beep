//! The session mirror: one actor owns every machine's session rows. Readers
//! take the rows it publishes; a machine's push, a listing's answer and each
//! local edit reach it as one message. Asking a machine runs off its loop in
//! a `JoinSet` and comes back as a message.

use super::{Fleet, agents as direct};
use crate::center::{
    cache_view,
    view_cache::{View, ViewCache},
};
use beep_mesh::machine::listing::{Host, IDLE_SHOWN, Linked, apply_presence};
use beep_proto::{
    AgentStatus, MachineSession, SessionId,
    node::agent::{ExternalSession, ParentLink, Presence},
    watched,
};
use futures::FutureExt;
use iroh::EndpointId;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    fmt,
    future::Future,
    panic::AssertUnwindSafe,
    str::FromStr,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tauri::{AppHandle, Emitter, Manager};
use tokio::{
    sync::{OwnedSemaphorePermit, Semaphore, mpsc, watch},
    task::{AbortHandle, JoinSet},
    time::{Instant, sleep, timeout},
};
use tokio_util::task::AbortOnDropHandle;
use uuid::Uuid;

/// How long a machine's rows stand before a read lists it again, when it has
/// pushed none. Until then its pushes replace its rows alone. Nothing runs while nobody reads.
/// Scans never age: a push of what they read scans again.
const FRESH: Duration = Duration::from_secs(30);
/// The first wait before a machine that did not list its sessions is asked
/// again; it doubles up to `RELIST_LAST`.
const RELIST_FIRST: Duration = Duration::from_secs(1);
const RELIST_LAST: Duration = Duration::from_secs(16);
/// How long the first read of a launch waits for this Mac's node.
const LOCAL_WAIT: Duration = Duration::from_secs(6);
/// The most machines listed at once, and scanned at once.
const IN_FLIGHT: usize = 4;
/// How often at most the views file takes the rows.
const KEEP_EVERY: Duration = Duration::from_secs(30);
const WAITING: usize = 256;

/// One machine's rows.
#[derive(Clone, Default)]
pub(crate) struct Machine {
    /// Beep's own sessions on it.
    beep: Vec<MachineSession>,
    /// The sessions its agents started and the temporary reviewers, which the
    /// parent agent panel shows and the lists leave out.
    children: Vec<MachineSession>,
    /// How each of `children` hangs under its parent, by session.
    links: HashMap<SessionId, Link>,
    /// The sessions started outside Beep, and the assistant's ended
    /// conversations, found by scanning it.
    scans: Vec<MachineSession>,
    /// What its native session reports say last, which win over its scans.
    presence: Option<Reported>,
    source: Source,
}

/// A machine's native session reports.
#[derive(Clone)]
struct Reported {
    host: Host,
    sessions: Vec<ExternalSession>,
}

/// How a child session hangs under its parent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Link {
    parent: Option<ParentLink>,
    /// A temporary reviewer of its parent's changes.
    pub(crate) reviewer: bool,
}

/// The session that started another: where it runs and the ids it may be
/// listed under.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct SessionParent {
    pub(crate) machine_id: String,
    /// The agent's own id for the parent.
    pub(crate) id: SessionId,
    /// The id its node listed it under before the agent's own replaced it.
    pub(crate) key: SessionId,
}

impl Link {
    /// The session that started it, when an agent did; `machine_id` is the
    /// machine the child runs on, where a parent without a machine runs too.
    pub(crate) fn parent(&self, machine_id: &str) -> Option<SessionParent> {
        let parent = self.parent.as_ref()?;

        Some(SessionParent {
            machine_id: parent
                .machine
                .map_or_else(|| machine_id.to_owned(), |id| id.to_string()),
            id: parent.session.clone(),
            key: parent.key.clone(),
        })
    }

    /// An agent started it.
    pub(crate) fn has_parent(&self) -> bool {
        self.parent.is_some()
    }

    /// Started in the background by its parent, so no thread of its chat.
    pub(crate) fn background(&self) -> bool {
        self.parent.as_ref().is_some_and(|parent| parent.background)
    }
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
    pub(super) fn kept(kept: Vec<MachineSession>) -> Self {
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
        self.iter_of(None)
    }

    /// Every row, for the views file.
    pub(crate) fn sessions(&self) -> Vec<MachineSession> {
        self.iter().cloned().collect()
    }

    /// The rows of `shown` machines only; of every machine when `None`.
    pub(crate) fn iter_of<'a>(
        &'a self,
        shown: Option<&'a HashSet<EndpointId>>,
    ) -> impl Iterator<Item = &'a MachineSession> {
        self.machines
            .iter()
            .filter(move |(machine, _)| shown.is_none_or(|shown| shown.contains(*machine)))
            .flat_map(|(_, machine)| machine.beep.iter().chain(&machine.scans))
    }

    /// The machines whose Beep rows, children or links differ from `old`'s.
    pub(crate) fn changed_since(&self, old: &Rows) -> HashSet<EndpointId> {
        self.machines
            .keys()
            .chain(old.machines.keys())
            .filter(|id| match (self.machines.get(*id), old.machines.get(*id)) {
                (Some(new), Some(old)) => {
                    new.beep != old.beep || new.children != old.children || new.links != old.links
                }
                (None, None) => false,
                _ => true,
            })
            .copied()
            .collect()
    }

    /// No machine was read or kept yet.
    pub(crate) fn is_empty(&self) -> bool {
        self.machines.is_empty()
    }

    /// The child and reviewer sessions of the `shown` machines, of every
    /// machine when `None`, with how each hangs under its parent.
    pub(crate) fn children_of<'a>(
        &'a self,
        shown: Option<&'a HashSet<EndpointId>>,
    ) -> impl Iterator<Item = (&'a MachineSession, &'a Link)> {
        self.machines
            .iter()
            .filter(move |(machine, _)| shown.is_none_or(|shown| shown.contains(*machine)))
            .flat_map(|(_, machine)| {
                machine
                    .children
                    .iter()
                    .filter_map(|child| Some((child, machine.links.get(&child.id)?)))
            })
    }

    /// How `session` on `machine_id` hangs under its parent, when it is a
    /// child or a reviewer.
    pub(crate) fn link(&self, machine_id: &str, session: &SessionId) -> Option<&Link> {
        let machine = self.machines.get(&machine_id.parse::<EndpointId>().ok()?)?;

        machine
            .children
            .iter()
            .any(|child| child.id == *session)
            .then(|| machine.links.get(session))?
    }

    /// Every Beep session the mirror holds, children and reviewers with how they hang.
    pub(crate) fn linked(&self) -> Vec<Linked> {
        let top = |session: &MachineSession| Linked {
            session: session.clone(),
            parent: None,
            reviewer: false,
        };

        self.machines
            .values()
            .flat_map(|machine| {
                let children = machine.children.iter().filter_map(|session| {
                    let link = machine.links.get(&session.id)?;

                    Some(Linked {
                        session: session.clone(),
                        parent: link.parent.clone(),
                        reviewer: link.reviewer,
                    })
                });

                machine.beep.iter().map(top).chain(children)
            })
            .collect()
    }

    /// Beep's own top-level sessions on every machine.
    pub(crate) fn beep(&self) -> impl Iterator<Item = &MachineSession> {
        self.machines.values().flat_map(|machine| &machine.beep)
    }

    /// `scan`, a fresh scan of `machine`, with its native reports on top.
    pub(crate) fn with_presence(&self, machine: EndpointId, scan: &mut Vec<MachineSession>) {
        if let Some(reported) = self
            .machines
            .get(&machine)
            .and_then(|machine| machine.presence.as_ref())
        {
            apply_presence(&reported.host, &reported.sessions, scan);
        }
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
    /// This Mac, whose listing and scan wait for no free slot.
    fn me(&self) -> EndpointId;
    /// The machines of the mesh, this Mac among them; `None` until the mesh opens.
    fn members(&self) -> Option<HashSet<EndpointId>>;
    /// Beep's sessions on `machine`; `None` when it did not answer.
    fn list(&self, machine: EndpointId) -> impl Future<Output = Option<Vec<Linked>>> + Send;
    /// What a scan of `machine` finds; `None` when it did not answer.
    fn scan(&self, machine: EndpointId)
    -> impl Future<Output = Option<Vec<MachineSession>>> + Send;
    /// Tells the windows, and the lists that follow, what changed.
    fn drawn(&self, drawn: Drawn);
    /// Holds what a scan of `machine` reads, so its node keeps pushing it, until the stream
    /// that carries it ends: true then, false when no stream was open to hold it on.
    fn hold(&self, machine: EndpointId) -> impl Future<Output = bool> + Send;
}

/// A change in what the windows draw.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Drawn {
    /// A listing of a machine came back and its rows differ.
    Listed,
    /// A local edit of this session.
    Session(SessionId),
}

enum Message {
    /// Someone reads the rows: what is older than `FRESH` is listed again.
    Read,
    /// The next read lists this machine again.
    Stale(EndpointId),
    /// The hold on what a scan of this machine reads ended: `held` when its stream ended, not
    /// when none was open.
    Released { machine: EndpointId, held: bool },
    /// What a scan of this machine reads changed: it is scanned again.
    Rescan(EndpointId),
    /// A machine's `agents` subscription ended: its rows age again.
    Unpushed(EndpointId),
    /// A machine pushed its Beep rows.
    Pushed {
        machine: EndpointId,
        rows: Vec<Linked>,
    },
    /// A listing of one machine came back.
    Listed {
        machine: EndpointId,
        rows: Option<Vec<Linked>>,
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
    /// The native reports of the sessions started outside Beep on a machine.
    Presence {
        machine: EndpointId,
        host: Host,
        sessions: Vec<ExternalSession>,
    },
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
    started: Instant,
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
            scanning: HashMap::new(),
            holding: HashSet::new(),
            expiries: HashMap::new(),
            list_slots: Arc::new(Semaphore::new(IN_FLIGHT)),
            scan_slots: Arc::new(Semaphore::new(IN_FLIGHT)),
            tasks: JoinSet::new(),
        };

        Self {
            inbox,
            rows: follow,
            started: Instant::now(),
            _task: AbortOnDropHandle::new(tokio::spawn(actor.run(messages))),
        }
    }

    /// The rows now, and a listing of what is older than `FRESH`.
    pub(crate) fn rows(&self) -> Arc<Rows> {
        // A full inbox holds a read already.
        let _ = self.inbox.try_send(Message::Read);
        self.current()
    }

    /// Has the mirror list the machines that are due, as a reader that wants rows does.
    pub(crate) fn list_now(&self) {
        let _ = self.inbox.try_send(Message::Read);
    }

    /// As `rows`, once `ready` holds of them or `wait` has passed.
    pub(crate) async fn rows_when(
        &self,
        ready: impl Fn(&Rows) -> bool,
        wait: Duration,
    ) -> Arc<Rows> {
        self.list_now();
        let mut rows = self.rows.clone();

        // Either way the rows now are the answer.
        let _ = timeout(wait, watched::wait_for(&mut rows, |rows| ready(rows))).await;
        self.current()
    }

    /// The rows for a reader that cannot wait long: until `LOCAL_WAIT` after
    /// the launch each caller waits for this Mac's node to list, then they
    /// are as they stand.
    pub(crate) async fn rows_of_launch(&self, fleet: &Fleet) -> Arc<Rows> {
        let me = fleet.id();
        // Without the mesh open this Mac is not online yet; the mesh gets no
        // longer than the wait.
        let open = timeout(self.launch_left(), fleet.ready()).await.is_ok();

        self.launch_rows(me, open && fleet.online().contains(&me))
            .await
    }

    /// The rows once this Mac listed, if it is `online`, or `LOCAL_WAIT`
    /// after the launch.
    async fn launch_rows(&self, me: EndpointId, online: bool) -> Arc<Rows> {
        if !online {
            return self.rows();
        }

        self.rows_when(|rows| rows.live(me), self.launch_left())
            .await
    }

    fn launch_left(&self) -> Duration {
        LOCAL_WAIT.saturating_sub(self.started.elapsed())
    }

    /// The rows as they stand, without a read.
    pub(crate) fn current(&self) -> Arc<Rows> {
        watched::read(&self.rows, Arc::clone)
    }

    /// A subscription to the rows, for what follows them.
    pub(crate) fn watch(&self) -> watch::Receiver<Arc<Rows>> {
        self.rows.clone()
    }

    /// `machine` pushed its Beep rows: they replace the ones held.
    pub(crate) fn pushed(&self, machine: EndpointId, rows: Vec<Linked>) {
        self.send(Message::Pushed { machine, rows });
    }

    /// `machine` no longer pushes its rows: they age and are listed again.
    pub(crate) fn unpushed(&self, machine: EndpointId) {
        self.send(Message::Unpushed(machine));
    }

    /// The next read lists `machine` again.
    pub(crate) fn stale(&self, machine: EndpointId) {
        self.send(Message::Stale(machine));
    }

    /// `machine` pushed what a scan of it reads: it is scanned again, from the push.
    pub(crate) fn rescan(&self, machine: EndpointId) {
        self.send(Message::Rescan(machine));
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

    /// `machine` reported its sessions started outside Beep: they win over
    /// its scans, now and in the scans to come.
    pub(crate) fn presence(&self, machine: EndpointId, host: Host, sessions: Vec<ExternalSession>) {
        self.send(Message::Presence {
            machine,
            host,
            sessions,
        });
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
    /// Its scan answered. It stays current: what it reads is pushed.
    scanned: bool,
    /// The machine pushes its rows, so they never age: a listing would read them back.
    pushed: bool,
}

/// A machine whose sessions are listed now.
struct Relist {
    /// The wait before the next try, should the machine not answer.
    pause: Duration,
    /// A change came meanwhile (an edit, a push, a refused delete or a move):
    /// the answer may hold what it undid, so it is dropped and the machine
    /// listed again.
    edited: bool,
}

impl Relist {
    fn new() -> Self {
        Self {
            pause: RELIST_FIRST,
            edited: false,
        }
    }
}

struct Actor<O> {
    outside: Arc<O>,
    rows: watch::Sender<Arc<Rows>>,
    reads: HashMap<EndpointId, Reads>,
    relisting: HashMap<EndpointId, Relist>,
    /// The machines scanned now, and whether an edit came meanwhile.
    scanning: HashMap<EndpointId, bool>,
    /// The machines whose scanned reads are held, so their node pushes them.
    holding: HashSet<EndpointId>,
    /// The scan each machine has waiting for the hour a session it shows leaves the list.
    expiries: HashMap<EndpointId, AbortHandle>,
    list_slots: Arc<Semaphore>,
    scan_slots: Arc<Semaphore>,
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
                    Err(e) if e.is_cancelled() => continue,
                    Err(e) => {
                        log::warn!("asking a machine for its sessions stopped: {e}");
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
            Message::Stale(machine) => {
                self.reads.remove(&machine);
            }
            Message::Released { machine, held } => {
                self.holding.remove(&machine);

                // The node no longer pushes what the scan read: it is scanned again on the
                // next read, and when the machine pushes again.
                if held {
                    self.reads.entry(machine).or_default().scanned = false;
                }
            }
            Message::Rescan(machine) => self.rescan(machine),
            Message::Unpushed(machine) => {
                if let Some(reads) = self.reads.get_mut(&machine) {
                    reads.pushed = false;
                }
            }
            Message::Pushed { machine, rows } => self.pushed(machine, rows),
            Message::Listed { machine, rows } => self.listed(machine, rows),
            Message::Scanned { machine, rows } => self.scanned(machine, rows),
            Message::DeleteFailed(machine) | Message::Moved(machine) => {
                self.relist(machine);
                self.stale_scans();
            }
            Message::Deleted {
                machine_id,
                session,
            } => {
                self.edited(&machine_id);
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
                self.edited(&machine_id);
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
                self.edited(&machine_id);
                self.edit(|rows| answer(rows, &machine_id, &session, question));
                self.outside.drawn(Drawn::Session(session));
            }
            Message::Presence {
                machine,
                host,
                sessions,
            } => self.took_presence(machine, Reported { host, sessions }),
            Message::Reported { machine_id, report } => {
                self.edit(|rows| report_in(rows, &machine_id, &report));
            }
        }
    }

    /// Lists each online machine whose rows are older than `FRESH`, and scans one not scanned yet.
    fn read(&mut self) {
        let now = Instant::now();
        self.prune();

        for machine in self.outside.online() {
            let reads = self.reads.entry(machine).or_default();
            let list = !reads.pushed && reads.beep.is_none_or(|at| now - at >= FRESH);
            let scan = !reads.scanned;

            if list && !self.relisting.contains_key(&machine) {
                self.start_listing(machine);
            }

            if scan {
                self.scan(machine);
            }
        }
    }

    /// Drops the rows of machines that left the mesh.
    fn prune(&mut self) {
        let Some(members) = self.outside.members() else {
            return;
        };

        if !watched::read_sent(&self.rows, |rows| {
            rows.machines
                .keys()
                .any(|machine| !members.contains(machine))
        }) {
            return;
        }

        self.revise(|rows| {
            let before = rows.machines.len();
            rows.machines.retain(|machine, _| members.contains(machine));
            rows.machines.len() != before
        });
        self.reads.retain(|machine, _| members.contains(machine));
    }

    /// Marks the listings and scans of `machine_id` that run now, which an
    /// edit came in the middle of.
    fn edited(&mut self, machine_id: &str) {
        for (machine, relist) in &mut self.relisting {
            relist.edited |= machine.to_string() == machine_id;
        }

        for (machine, edited) in &mut self.scanning {
            *edited |= machine.to_string() == machine_id;
        }
    }

    /// Lists `machine` because what it listed may be out of date, one listing
    /// at a time: one under way is dropped and runs again.
    fn relist(&mut self, machine: EndpointId) {
        match self.relisting.get_mut(&machine) {
            Some(relist) => relist.edited = true,
            None => self.start_listing(machine),
        }
    }

    fn start_listing(&mut self, machine: EndpointId) {
        self.relisting.insert(machine, Relist::new());
        self.list(machine, Duration::ZERO);
    }

    /// Takes what `machine` pushed as its Beep rows. A listing under way may
    /// hold older rows, so it is dropped and runs again.
    fn pushed(&mut self, machine: EndpointId, rows: Vec<Linked>) {
        if let Some(relist) = self.relisting.get_mut(&machine) {
            relist.edited = true;
        }

        self.replace_listed(machine, rows);

        let reads = self.reads.entry(machine).or_default();
        reads.beep = Some(Instant::now());
        let streamed = !std::mem::replace(&mut reads.pushed, true);
        let scanned = reads.scanned;

        // Its stream just opened, so what a scan read before may have changed unheard.
        if streamed && scanned {
            self.rescan(machine);
        }
    }

    /// Scans `machine` again from a push of what a scan reads.
    fn rescan(&mut self, machine: EndpointId) {
        match self.scanning.get_mut(&machine) {
            // What the scan under way read may be older than the push.
            Some(edited) => *edited = true,
            None => self.scan(machine),
        }
    }

    fn list(&mut self, machine: EndpointId, after: Duration) {
        let (outside, slots) = (self.outside.clone(), self.list_slots.clone());
        self.tasks.spawn(async move {
            if !after.is_zero() {
                sleep(after).await;
            }

            let _slot = slot(&slots, outside.me() == machine).await;
            // A panic is an answer that did not come: the machine is asked again.
            let rows = AssertUnwindSafe(outside.list(machine)).catch_unwind().await;

            Message::Listed {
                machine,
                rows: rows.unwrap_or_else(|_| {
                    log::warn!("listing the sessions of {machine} panicked");
                    None
                }),
            }
        });
    }

    /// Holds what a scan of `machine` reads, once, so the node pushes it when it changes.
    fn hold(&mut self, machine: EndpointId) {
        if !self.holding.insert(machine) {
            return;
        }

        let outside = self.outside.clone();
        self.tasks.spawn(async move {
            let held = AssertUnwindSafe(outside.hold(machine))
                .catch_unwind()
                .await
                .unwrap_or(false);

            Message::Released { machine, held }
        });
    }

    fn scan(&mut self, machine: EndpointId) {
        if self.scanning.insert(machine, false).is_some() {
            return;
        }

        let (outside, slots) = (self.outside.clone(), self.scan_slots.clone());
        self.tasks.spawn(async move {
            let _slot = slot(&slots, outside.me() == machine).await;
            let rows = AssertUnwindSafe(outside.scan(machine)).catch_unwind().await;

            Message::Scanned {
                machine,
                rows: rows.unwrap_or_else(|_| {
                    log::warn!("scanning {machine} panicked");
                    None
                }),
            }
        });
    }

    fn listed(&mut self, machine: EndpointId, rows: Option<Vec<Linked>>) {
        let mut relist = self.relisting.remove(&machine).unwrap_or_else(Relist::new);
        let mut drawn = false;

        if relist.edited {
            relist.edited = false;
            self.list(machine, Duration::ZERO);
            self.relisting.insert(machine, relist);
            return;
        }

        match rows {
            Some(rows) => drawn = self.replace_listed(machine, rows),
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

        if drawn {
            self.outside.drawn(Drawn::Listed);
        }
    }

    fn took_presence(&mut self, machine: EndpointId, reported: Reported) {
        let mut drawn = false;

        self.revise(|all| {
            let held = all.machines.entry(machine).or_default();
            let mut scans = held.scans.clone();
            apply_presence(&reported.host, &reported.sessions, &mut scans);
            drawn = differ(&held.scans, &scans);
            let changed = held.scans != scans;
            held.scans = scans;
            held.presence = Some(reported);

            changed
        });

        if drawn {
            self.outside.drawn(Drawn::Listed);
        }

        self.expire_scans(machine);
    }

    fn scanned(&mut self, machine: EndpointId, rows: Option<Vec<MachineSession>>) {
        if self.scanning.remove(&machine) == Some(true) {
            // An edit came while it ran: what it found may undo the edit.
            self.scan(machine);
            return;
        }

        self.reads.entry(machine).or_default().scanned = rows.is_some();

        if let Some(rows) = rows {
            if self.replace_scans(machine, rows) {
                self.outside.drawn(Drawn::Listed);
            }

            self.hold(machine);
        }

        self.expire_scans(machine);
    }

    /// A session that waits for its user stays in the scan for `IDLE_SHOWN` after its last
    /// entry, and the node pushes nothing when that passes: the machine is scanned again then.
    fn expire_scans(&mut self, machine: EndpointId) {
        if let Some(earlier) = self.expiries.remove(&machine) {
            earlier.abort();
        }

        let now = SystemTime::now();
        let left = watched::read_sent(&self.rows, |rows| {
            rows.machines.get(&machine).and_then(|held| {
                held.scans
                    .iter()
                    .filter(|row| row.status == AgentStatus::Waiting {})
                    .filter_map(|row| (row.last_activity + IDLE_SHOWN).duration_since(now).ok())
                    .min()
            })
        });

        if let Some(left) = left {
            let wait = left + Duration::from_secs(1);
            let handle = self.tasks.spawn(async move {
                sleep(wait).await;

                Message::Rescan(machine)
            });
            self.expiries.insert(machine, handle);
        }
    }

    /// Takes `linked` as all of `machine`'s Beep sessions: the top-level
    /// ones are its rows, the children are kept beside them. True when what
    /// the windows draw differs.
    fn replace_listed(&self, machine: EndpointId, linked: Vec<Linked>) -> bool {
        let (children, rows): (Vec<Linked>, Vec<Linked>) = linked
            .into_iter()
            .partition(|linked| linked.parent.is_some() || linked.reviewer);
        let links: HashMap<SessionId, Link> = children
            .iter()
            .map(|child| {
                let link = Link {
                    parent: child.parent.clone(),
                    reviewer: child.reviewer,
                };

                (child.session.id.clone(), link)
            })
            .collect();
        let children: Vec<MachineSession> =
            children.into_iter().map(|child| child.session).collect();
        let rows: Vec<MachineSession> = rows.into_iter().map(|linked| linked.session).collect();
        let mut drawn = false;

        self.revise(|all| {
            let held = all.machines.entry(machine).or_default();
            drawn = differ(&held.beep, &rows);
            let changed = held.beep != rows
                || held.children != children
                || held.links != links
                || held.source != Source::Live;
            held.beep = rows;
            held.children = children;
            held.links = links;
            held.source = Source::Live;

            changed
        });

        drawn
    }

    /// Takes `rows` as all of `machine`'s found by a scan. True when what the
    /// windows draw differs.
    fn replace_scans(&self, machine: EndpointId, rows: Vec<MachineSession>) -> bool {
        let mut drawn = false;

        self.revise(|all| {
            let held = all.machines.entry(machine).or_default();
            let mut rows = rows;

            if let Some(reported) = &held.presence {
                apply_presence(&reported.host, &reported.sessions, &mut rows);
            }

            drawn = differ(&held.scans, &rows);
            let changed = held.scans != rows;
            held.scans = rows;

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
                changed |= change(&mut machine.children);
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
            reads.scanned = false;
        }
    }
}

/// A free slot among the machines asked at once; this Mac's node needs none.
async fn slot(slots: &Arc<Semaphore>, mine: bool) -> Option<OwnedSemaphorePermit> {
    if mine {
        return None;
    }

    // The semaphore lives as long as the actor, so this holds a slot or the task ends.
    slots.clone().acquire_owned().await.ok()
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

/// Whether the windows draw `new` other than `old`.
fn differ(old: &[MachineSession], new: &[MachineSession]) -> bool {
    old.len() != new.len() || old.iter().zip(new).any(|(a, b)| !same(a, b))
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

    fn me(&self) -> EndpointId {
        self.fleet.id()
    }

    fn members(&self) -> Option<HashSet<EndpointId>> {
        self.fleet.mesh()?;

        Some(
            self.fleet
                .machines()
                .into_iter()
                .map(|machine| machine.device.id)
                .chain([self.fleet.id()])
                .collect(),
        )
    }

    async fn list(&self, machine: EndpointId) -> Option<Vec<Linked>> {
        direct::sessions_of(&self.fleet, machine).await
    }

    async fn scan(&self, machine: EndpointId) -> Option<Vec<MachineSession>> {
        direct::scanned_sessions_of(&self.fleet, machine).await
    }

    async fn hold(&self, machine: EndpointId) -> bool {
        direct::hold_scanned_reads(&self.fleet, machine).await
    }

    fn drawn(&self, drawn: Drawn) {
        let app = &self.app;
        // The rows reach Home through the mirror's watch. A local edit tells the windows which
        // session, for the closing rows to settle.
        if let Drawn::Session(session) = drawn {
            crate::fleet::view_signal::ring(app, crate::fleet::view_signal::Change::Edited);

            if let Err(e) = app.emit("sessions-changed", session) {
                log::warn!("telling the windows the sessions changed: {e}");
            }
        }
    }
}

/// The machines whose rows show: those online, those being connected to
/// (their rows are kept while they connect or have not answered yet) and
/// this Mac. A machine that is none of these is off: its rows stay in the
/// mirror and show again when it is back.
pub(crate) fn showing(
    online: &[EndpointId],
    connecting: &[EndpointId],
    me: EndpointId,
) -> HashSet<EndpointId> {
    online
        .iter()
        .chain(connecting)
        .copied()
        .chain([me])
        .collect()
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

/// Keeps `rows` in `cache` for the next start.
pub(crate) fn save(cache: &Arc<ViewCache>, rows: &[MachineSession]) {
    cache_view(cache, View::AgentSessions, &rows);
}

/// Hands `save` the rows for the next start, when what they hold changed
/// and `KEEP_EVERY` has passed since the last time. The app saves them
/// again when it quits.
pub(crate) async fn keep(
    mut rows: watch::Receiver<Arc<Rows>>,
    mut save: impl FnMut(&[MachineSession]),
) {
    let mut saved = watched::read(&rows, |rows| rows.sessions());
    let mut at: Option<Instant> = None;

    while rows.changed().await.is_ok() {
        if let Some(at) = at {
            sleep((at + KEEP_EVERY).saturating_duration_since(Instant::now())).await;
        }

        let now = watched::read_and_update(&mut rows, |rows| rows.sessions());

        if kept_between(&saved, &now) {
            save(&now);
            saved = now;
            at = Some(Instant::now());
        }
    }
}

/// Whether two lists of rows differ in what the next start shows: every field
/// but the time of the last activity, which moves with every report.
fn kept_between(old: &[MachineSession], new: &[MachineSession]) -> bool {
    let old: HashMap<_, _> = old
        .iter()
        .map(|row| ((&row.machine_id, &row.id), row))
        .collect();

    old.len() != new.len()
        || new.iter().any(|row| {
            old.get(&(&row.machine_id, &row.id)).is_none_or(|old| {
                let mut row = row.clone();
                row.last_activity = old.last_activity;
                row != **old
            })
        })
}

/// Shows a session its machine said ended as ended in the mirror's rows, so
/// Home and the session lists drop it from Running at once rather than on a
/// later read, and has the windows read them again.
pub(crate) fn closed(app: &AppHandle, machine_id: &str, session: &SessionId) {
    app.state::<Arc<SessionMirror>>()
        .closed(machine_id, session);
}

/// A start's id, as the windows know it until its machine names the
/// session: `start:<uuid>`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub(crate) struct StartId(Uuid);

const PREFIX: &str = "start:";

impl StartId {
    pub(crate) fn new() -> Self {
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

#[cfg(test)]
#[path = "../tests/fleet_session_mirror.rs"]
mod tests;
