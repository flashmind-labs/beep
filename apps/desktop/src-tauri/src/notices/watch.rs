//! What the bell learns by looking at the mesh: a machine offline for
//! `AWAY` and back, a temporary machine removed once gone for a day,
//! sync conflicts on a machine, and a session move that failed. Each becomes
//! a note in `notes`.

use crate::notes::{self, Note, NoteKind, now_ms};
use crate::{
    beep::epoch_ms,
    fleet::Fleet,
    targets::{AgentTarget, CenterTarget},
};
use beep_fleet::{
    Desk, MoveState, Session,
    temporary::{Leftovers, sweep},
};
use beep_proto::{
    AgentNoticeKey,
    node::{
        agent::{MoveEntry, MovePhase, SessionsReply},
        events::{MOVE_PROGRESS_TOPIC, Value},
        mesh_state::SessionMove,
        skills, sync,
    },
};
use futures::future::join_all;
use iroh::EndpointId;
use std::{
    collections::{BTreeSet, HashMap, HashSet},
    hash::Hash,
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant, SystemTime},
};
use tauri::{AppHandle, Manager};
use tokio::{
    select,
    sync::broadcast::error::RecvError,
    time::{Instant as TokioInstant, sleep_until},
};

/// Sync conflicts, skill conflicts and moves are read after a change, at
/// most this often, since each read asks the machines.
const SLOW_GAP: Duration = Duration::from_secs(60);
/// How long a temporary machine's removal waits after it failed.
const RETRY: Duration = Duration::from_secs(60);
/// How long a machine is offline before the bell says so, so a short drop
/// says nothing.
const AWAY: Duration = Duration::from_secs(120);
/// How old a move may be and still be asked about.
const MOVES_FOR_MS: u64 = 24 * 3600 * 1000;
/// How long after a move started its machine, which pushes its moves, may leave it out before
/// the machine is asked: a node that restarted, or kept the outcome for a short while only,
/// does not say how the move ended.
const LOST_AFTER_MS: u64 = 5 * 60 * 1000;

/// The conflicts of each project on one machine, as its node pushed them.
type Said = Vec<(String, BTreeSet<String>)>;

/// What the machines pushed that the bell takes news from: each machine's latest sync, whose
/// conflicts it tells without asking anyone, how the moves to each machine stand, and whether
/// a move of a machine that does not push them changed.
pub(crate) struct Looks {
    said: Mutex<HashMap<EndpointId, Said>>,
    /// A machine's latest `move-progress`; a node from before it has none.
    moves: Mutex<HashMap<EndpointId, Vec<MoveEntry>>>,
    moves_pushed: AtomicBool,
    moved: AtomicBool,
}

impl Looks {
    pub(crate) fn new() -> Self {
        Self {
            said: Mutex::default(),
            moves: Mutex::default(),
            moves_pushed: AtomicBool::new(false),
            moved: AtomicBool::new(false),
        }
    }

    fn hear(&self, machine: EndpointId, topic: &str, value: &Value) {
        match (topic, value) {
            ("sync", Value::Sync(reply)) => {
                let sync::Reply::Status { projects } = reply.as_ref() else {
                    return;
                };
                let said = projects
                    .iter()
                    .map(|project| {
                        (
                            project.name.clone(),
                            project.conflicts.iter().cloned().collect(),
                        )
                    })
                    .collect();
                self.said
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .insert(machine, said);
            }
            (MOVE_PROGRESS_TOPIC, Value::Sessions(reply)) => {
                let SessionsReply::Moves { moves } = reply.as_ref() else {
                    return;
                };
                self.moves
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .insert(machine, moves.clone());
                self.moves_pushed.store(true, Ordering::Relaxed);
            }
            // A machine that pushes its moves says what changed itself.
            ("moves", _) => {
                if self
                    .moves
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .contains_key(&machine)
                {
                    return;
                }

                self.moved.store(true, Ordering::Relaxed);
            }
            _ => {}
        }
    }

    fn pushed_moves(&self) -> HashMap<EndpointId, Vec<MoveEntry>> {
        self.moves
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// The syncs pushed since the last call, the latest of each machine.
    fn take_said(&self) -> HashMap<EndpointId, Said> {
        std::mem::take(&mut *self.said.lock().unwrap_or_else(PoisonError::into_inner))
    }
}

/// The moves each machine last pushed, by machine; a machine from before `move-progress` has none.
pub(crate) fn pushed_moves(app: &AppHandle) -> HashMap<EndpointId, Vec<MoveEntry>> {
    app.state::<Looks>().pushed_moves()
}

/// Looks at the mesh while the app runs: again on each change to it, to
/// this Mac's sync or to what the machines push of theirs, and when a machine away or a
/// temporary machine is due. A desktop keeps the machines pushing their sync and moves.
pub(crate) async fn follow(app: AppHandle, fleet: Arc<Fleet>, session: Arc<Session>) {
    let mut away = Away::default();
    let mut leftovers = Leftovers::default();
    let mut conflicts = Conflicts::default();
    let mut moves = HashSet::new();
    let mut changes = fleet.changes();
    let mut local = beep_node::events::system().subscribe();
    let mut slow = Slow::default();
    let mut heard = fleet.heard();
    #[cfg(desktop)]
    let _standing = hold_pushes(&app, &fleet);

    loop {
        machines(&app, &fleet, &mut away);
        temporaries(&app, &fleet, &mut leftovers).await;
        let now = Instant::now();
        let looks = app.state::<Looks>();

        for (machine, said) in looks.take_said() {
            sync(&app, &fleet, machine, said, &mut conflicts);
        }

        let moves_pushed = looks.moves_pushed.swap(false, Ordering::Relaxed);
        slow.wanted |= looks.moved.swap(false, Ordering::Relaxed);
        let reads = slow.due(now);

        if moves_pushed || reads {
            match session.desk().await {
                Ok(desk) => {
                    if reads {
                        skill_conflicts(&app, &fleet, &mut conflicts).await;
                    }

                    let pushed = looks.pushed_moves();
                    failed_moves(&app, &fleet, &desk, &mut moves, &pushed, reads).await;
                }
                Err(e) => log::debug!("the bell does not read the mesh: {e}"),
            }
        }

        if reads {
            slow.read(now);
        }

        // A removal that failed stays due: it is tried again after `RETRY`, not in a loop.
        let removal = leftovers.due().map(|due| due.max(now + RETRY));
        let wake = [away.due(), removal, slow.wake()]
            .into_iter()
            .flatten()
            .min();

        select! {
            changed = changes.changed() => {
                if !changed {
                    return;
                }

                slow.wanted = true;
            }
            heard = local.recv() => match heard {
                Ok(_) | Err(RecvError::Closed) => {}
                Err(RecvError::Lagged(_)) => slow.wanted = true,
            },
            said = heard.recv() => match said {
                Ok(said) => {
                    if let Some(value) = said.value() {
                        looks.hear(said.machine, said.topic.name(), value);
                    }
                }
                Err(RecvError::Closed) => return,
                // Some pushes were skipped: take the current ones again.
                Err(RecvError::Lagged(_)) => {
                    for said in fleet.heard_latest() {
                        if let Some(value) = said.value() {
                            looks.hear(said.machine, said.topic.name(), value);
                        }
                    }

                    slow.wanted = true;
                }
            },
            () = async {
                match wake {
                    Some(wake) => sleep_until(TokioInstant::from_std(wake)).await,
                    None => std::future::pending().await,
                }
            } => {}
        }
    }
}

/// When the reads that ask the machines run: soon after a change, but at
/// most once per `SLOW_GAP`.
#[derive(Debug)]
struct Slow {
    last: Option<Instant>,
    wanted: bool,
}

impl Default for Slow {
    fn default() -> Self {
        Self {
            last: None,
            wanted: true,
        }
    }
}

impl Slow {
    fn due(&self, now: Instant) -> bool {
        self.wanted && self.wake().is_none_or(|wake| wake <= now)
    }

    /// When the read a change asked for is due; `None` when nothing waits, or at once.
    fn wake(&self) -> Option<Instant> {
        self.wanted
            .then(|| self.last.map(|last| last + SLOW_GAP))
            .flatten()
    }

    fn read(&mut self, now: Instant) {
        self.last = Some(now);
        self.wanted = false;
    }
}

/// Keeps every machine pushing its sync and its moves while the app runs, so a conflict or a
/// failed move on one is told as it comes.
#[cfg(desktop)]
fn hold_pushes(app: &AppHandle, fleet: &Arc<Fleet>) -> Option<()> {
    use crate::fleet::{events::Events, outbox::WindowOutbox, view_signal::Interest};
    use beep_proto::node::events::Topic;

    let events = app.state::<Events>();
    let interests = ["sync", "moves", MOVE_PROGRESS_TOPIC]
        .map(|name| Interest {
            machine: None,
            topic: Topic::new(name),
        })
        .to_vec();

    match app.state::<WindowOutbox>().hold_set("notices", interests) {
        Ok(()) => {
            events.start(app.clone(), fleet.clone());
            Some(())
        }
        Err(error) => {
            log::warn!("the bell cannot follow the machines' sync: {error}");
            None
        }
    }
}

fn machines(app: &AppHandle, fleet: &Fleet, away: &mut Away<EndpointId>) {
    let me = fleet.id();
    // A temporary machine going offline is no news: it stopped, or it is removed in time.
    let mesh = fleet.mesh();
    let temporary = mesh
        .as_ref()
        .map(|mesh| mesh.temporaries())
        .unwrap_or_default();
    let online: HashMap<EndpointId, bool> = fleet
        .machines()
        .into_iter()
        .filter(|status| {
            status.device.id != me
                && !temporary.contains(&status.device.id)
                && !mesh
                    .as_ref()
                    .is_some_and(|mesh| mesh.is_observer(status.device.id))
        })
        .map(|status| (status.device.id, status.online))
        .collect();
    for change in away.step(&online, Instant::now()) {
        match change {
            Change::Gone { id, since } => {
                let name = fleet.name_of(id);
                let went = SystemTime::now()
                    .checked_sub(since.elapsed())
                    .unwrap_or_else(SystemTime::now);
                let note = Note {
                    at: epoch_ms(went),
                    ..Note::new(
                        NoteKind::Offline,
                        machine_note(id),
                        format!("{name} went offline"),
                        name,
                    )
                };
                notes::record(app, note);
            }
            Change::Back(id) => {
                let note = machine_note(id);
                notes::finish(app, |n| n.id == note);
            }
        }
    }
}

/// Removes the temporary machines offline for a day, with a note for each this Mac
/// removed. Another desktop may have removed one first: then it says nothing.
async fn temporaries(app: &AppHandle, fleet: &Fleet, leftovers: &mut Leftovers<EndpointId>) {
    let Some(mesh) = fleet.mesh() else {
        return;
    };
    let removed = match sweep(&mesh, leftovers, Instant::now()).await {
        Ok(removed) => removed,
        Err(e) => {
            log::warn!("temporary machines not removed: {e}");
            return;
        }
    };

    for device in removed {
        let note = Note::new(
            NoteKind::Removed,
            format!("removed:{}", device.id),
            format!("{} was removed after a day offline", device.name),
            device.name,
        );
        notes::record(app, note);
    }
}

fn machine_note(id: EndpointId) -> String {
    format!("machine:{id}")
}

/// The conflicts the open note `id` holds, one per line.
fn kept_lines(app: &AppHandle, id: &str) -> BTreeSet<String> {
    notes::get(app, id)
        .filter(|note| !note.done)
        .map(|note| note.text.lines().map(str::to_owned).collect())
        .unwrap_or_default()
}

/// Tells the conflicts `machine` pushed of its projects. A machine that says nothing of a
/// project, as an offline one does, leaves its note as it is.
fn sync(
    app: &AppHandle,
    fleet: &Fleet,
    machine: EndpointId,
    said: Said,
    conflicts: &mut Conflicts,
) {
    for (project, paths) in said {
        let id = format!("conflict:{project}:{machine}");
        match conflicts.step(&id, &paths, || kept_lines(app, &id)) {
            Settle::Same => {}
            Settle::Settled => notes::finish(app, |note| note.id == id),
            Settle::Raise => {
                let title = match paths.len() {
                    1 => format!("Sync conflict in {project}"),
                    n => format!("{n} sync conflicts in {project}"),
                };
                let text = paths.into_iter().collect::<Vec<_>>().join("\n");
                let note = Note {
                    text,
                    ..Note::new(NoteKind::Conflict, id, title, fleet.name_of(machine))
                };
                notes::record(app, note);
            }
        }
    }
}

/// The skills this Mac's node found differing across the mesh: one note per
/// name, told again only when the copies change.
async fn skill_conflicts(app: &AppHandle, fleet: &Fleet, conflicts: &mut Conflicts) {
    let asked = match fleet.client(fleet.id()).await {
        Ok(client) => {
            client
                .call::<_, skills::Reply>(skills::PROTOCOL, &skills::Request::Conflicts {})
                .await
        }
        Err(e) => {
            log::debug!("skill conflicts not read: {e}");
            return;
        }
    };
    let now = match asked {
        Ok(skills::Reply::Conflicts { conflicts }) => conflicts,
        Ok(_) => return,
        Err(e) => {
            log::debug!("skill conflicts not read: {e}");
            return;
        }
    };
    let mut ids: HashSet<String> = now.iter().map(|c| skill_note(&c.name)).collect();
    // A note still open for a name that no longer differs is settled.
    ids.extend(
        notes::open_of(app, NoteKind::SkillConflict)
            .into_iter()
            .map(|note| note.id),
    );

    for id in ids {
        let conflict = now.iter().find(|c| skill_note(&c.name) == id);
        let copies: BTreeSet<String> = conflict
            .map(|c| c.copies.iter().cloned().collect())
            .unwrap_or_default();
        match conflicts.step(&id, &copies, || kept_lines(app, &id)) {
            Settle::Same => {}
            Settle::Settled => notes::finish(app, |note| note.id == id),
            Settle::Raise => {
                if let Some(conflict) = conflict {
                    let note = Note {
                        text: conflict.copies.join("\n"),
                        here: conflict.here.clone(),
                        held: conflict.held.clone(),
                        ..Note::new(
                            NoteKind::SkillConflict,
                            id,
                            conflict.title(),
                            "Skills".to_owned(),
                        )
                    };
                    notes::record(app, note);
                }
            }
        }
    }
}

fn skill_note(name: &str) -> String {
    format!("skill:{name}")
}

/// What a machine pushed of a move to it.
#[derive(Debug, PartialEq, Eq)]
enum Heard {
    Failed(String),
    Over,
    Going,
    /// The machine pushes no moves: it is from before that.
    Silent,
    /// The machine pushes its moves but not this one, long after it began: it is asked once.
    Lost,
}

fn heard_of(record: &SessionMove, pushed: &HashMap<EndpointId, Vec<MoveEntry>>, now: u64) -> Heard {
    let Some(entries) = pushed.get(&record.to) else {
        return Heard::Silent;
    };
    let Some(entry) = entries
        .iter()
        .find(|entry| entry.session == record.id && entry.at == record.at)
    else {
        return if now.saturating_sub(record.at) > LOST_AFTER_MS {
            Heard::Lost
        } else {
            Heard::Going
        };
    };

    match entry.progress.phase {
        MovePhase::Failed => Heard::Failed(entry.progress.reason.clone().unwrap_or_default()),
        MovePhase::Done | MovePhase::Cancelled => Heard::Over,
        _ => Heard::Going,
    }
}

/// Tells the moves that failed, from what the target machines pushed. A target from before
/// that is asked, when `ask` allows it.
async fn failed_moves(
    app: &AppHandle,
    fleet: &Fleet,
    desk: &Desk,
    told: &mut HashSet<String>,
    pushed: &HashMap<EndpointId, Vec<MoveEntry>>,
    ask: bool,
) {
    let now = now_ms();
    let open: Vec<(SessionMove, String)> = desk
        .moves()
        .into_iter()
        .filter(|record| now.saturating_sub(record.at) <= MOVES_FOR_MS)
        .map(|record| {
            let id = format!("move:{}:{}", record.id, record.at);
            (record, id)
        })
        .filter(|(_, id)| !told.contains(id) && notes::get(app, id).is_none())
        .collect();
    let mut silent = Vec::new();

    for (record, id) in open {
        match heard_of(&record, pushed, now) {
            Heard::Failed(reason) => {
                tell_failed(app, fleet, &record, id.clone(), reason);
                told.insert(id);
            }
            Heard::Over => {
                told.insert(id);
            }
            Heard::Going => {}
            Heard::Silent => silent.push((record, id, false)),
            Heard::Lost => silent.push((record, id, true)),
        }
    }

    if !ask {
        return;
    }

    // Each target is asked at once: a slow one holds up no other.
    let states = join_all(
        silent
            .iter()
            .map(|(record, _, _)| desk.move_state(&record.id)),
    )
    .await;

    for ((record, id, lost), state) in silent.into_iter().zip(states) {
        let progress = match state {
            Ok(MoveState::Known(progress)) => progress,
            Ok(MoveState::NeedsUpdate) => {
                told.insert(id);
                continue;
            }
            // A move the machine does not know long after it began has no outcome to tell.
            Ok(MoveState::Unknown) if lost => {
                told.insert(id);
                continue;
            }
            // Not heard of yet, or the target is away: asked again later.
            Ok(MoveState::Unknown) | Err(_) => continue,
        };

        match progress.phase {
            MovePhase::Failed => {
                tell_failed(
                    app,
                    fleet,
                    &record,
                    id.clone(),
                    progress.reason.unwrap_or_default(),
                );
                told.insert(id);
            }
            MovePhase::Done | MovePhase::Cancelled => {
                told.insert(id);
            }
            _ => {}
        }
    }
}

fn tell_failed(app: &AppHandle, fleet: &Fleet, record: &SessionMove, id: String, reason: String) {
    notes::record(
        app,
        Note {
            text: reason,
            ..moved(fleet, record, id)
        },
    );
}

/// The note of `record`'s failed move, which opens the session where it
/// stayed.
fn moved(fleet: &Fleet, record: &SessionMove, id: String) -> Note {
    let key = AgentNoticeKey {
        machine_id: record.from.to_string(),
        agent: record.agent.to_string(),
        session: record.id.clone(),
        question: None,
        machine_name: fleet.name_of(record.from),
    };
    Note {
        target: Some(CenterTarget::Agent(AgentTarget::notified(&key))),
        ..Note::new(
            NoteKind::MoveFailed,
            id,
            format!("Moving a session to {} failed", fleet.name_of(record.to)),
            record.project.clone(),
        )
    }
}

/// What a machine did since the last look.
#[derive(Debug, PartialEq)]
enum Change<K> {
    /// Offline since `since`, for `AWAY`.
    Gone { id: K, since: Instant },
    /// Back after a `Gone`.
    Back(K),
}

/// Which machines are away. Only a machine seen online in this run can go
/// away, so one that was off when the app started says nothing.
struct Away<K> {
    seen: HashSet<K>,
    since: HashMap<K, Instant>,
    told: HashSet<K>,
}

impl<K> Default for Away<K> {
    fn default() -> Self {
        Self {
            seen: HashSet::new(),
            since: HashMap::new(),
            told: HashSet::new(),
        }
    }
}

impl<K: Eq + Hash + Copy> Away<K> {
    /// When the next machine away is due to be told.
    fn due(&self) -> Option<Instant> {
        self.since
            .iter()
            .filter(|(id, _)| !self.told.contains(*id))
            .map(|(_, since)| *since + AWAY)
            .min()
    }

    /// The changes as of `now`, given each member and whether it is online.
    fn step(&mut self, online: &HashMap<K, bool>, now: Instant) -> Vec<Change<K>> {
        // A machine that left the mesh is forgotten.
        self.seen.retain(|id| online.contains_key(id));
        self.since.retain(|id, _| online.contains_key(id));
        self.told.retain(|id| online.contains_key(id));
        let mut changes = Vec::new();

        for (&id, &on) in online {
            if on {
                self.seen.insert(id);
                self.since.remove(&id);
                if self.told.remove(&id) {
                    changes.push(Change::Back(id));
                }
            } else if self.seen.contains(&id) {
                let since = *self.since.entry(id).or_insert(now);
                if now.saturating_duration_since(since) >= AWAY && self.told.insert(id) {
                    changes.push(Change::Gone { id, since });
                }
            }
        }

        changes
    }
}

/// What a machine's conflicts mean for its note.
#[derive(Debug, PartialEq)]
enum Settle {
    Same,
    /// A path came that was not there before.
    Raise,
    /// Every conflict is gone.
    Settled,
}

/// The conflict paths last seen, by note id.
#[derive(Default)]
struct Conflicts(HashMap<String, BTreeSet<String>>);

impl Conflicts {
    /// `paths` are the conflicts now; `kept` gives the ones the note holds,
    /// the first time an id is seen in this run.
    fn step(
        &mut self,
        id: &str,
        paths: &BTreeSet<String>,
        kept: impl FnOnce() -> BTreeSet<String>,
    ) -> Settle {
        let before = self.0.entry(id.to_owned()).or_insert_with(kept);
        let settle = if paths.is_empty() {
            if before.is_empty() {
                Settle::Same
            } else {
                Settle::Settled
            }
        } else if paths.iter().any(|path| !before.contains(path)) {
            Settle::Raise
        } else {
            Settle::Same
        };
        before.clone_from(paths);
        settle
    }
}

#[cfg(test)]
#[path = "../tests/notices_watch.rs"]
mod tests;
