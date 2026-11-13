//! What the bell learns by looking at the mesh: a machine offline for
//! `AWAY` and back, a temporary machine removed once gone for a day,
//! sync conflicts on a machine, and a session move that failed. Each becomes
//! a note in `history`.

use super::history::{self, Note, NoteKind, now_ms};
use crate::{
    beep::epoch_ms,
    center::{AgentTarget, CenterTarget},
    fleet::Fleet,
};
use beep_fleet::{
    Desk, MoveState, Session,
    temporary::{Leftovers, sweep},
};
use beep_proto::{
    AgentNoticeKey, SyncStatus,
    node::{agent::MovePhase, mesh_state::SessionMove, skills},
};
use iroh::EndpointId;
use std::{
    collections::{BTreeSet, HashMap, HashSet},
    hash::Hash,
    sync::Arc,
    time::{Duration, Instant, SystemTime},
};
use tauri::AppHandle;
use tokio::{
    select,
    sync::broadcast::error::RecvError,
    time::{Instant as TokioInstant, sleep_until},
};

/// Machines are looked at on each change to the mesh, and this often besides.
const LOOK_EVERY: Duration = Duration::from_secs(600);
/// Sync conflicts, skill conflicts and moves are read after a change, at
/// most this often, since each read asks the machines.
const SLOW_GAP: Duration = Duration::from_secs(60);
/// They are read this often besides, for what says nothing when it changes.
const SLOW_EVERY: Duration = Duration::from_secs(300);
/// How long a machine is offline before the bell says so, so a short drop
/// says nothing.
const AWAY: Duration = Duration::from_secs(120);
/// How old a move may be and still be asked about.
const MOVES_FOR_MS: u64 = 24 * 3600 * 1000;

/// Looks at the mesh while the app runs: again on each change to it or to
/// this Mac's sync, and when a machine away is due to be told.
pub(crate) async fn follow(app: AppHandle, fleet: Arc<Fleet>, session: Arc<Session>) {
    let mut away = Away::default();
    let mut leftovers = Leftovers::default();
    let mut conflicts = Conflicts::default();
    let mut moves = HashSet::new();
    let mut changes = fleet.changes();
    let mut local = beep_node::events::system().subscribe();
    let mut slow = Slow::default();

    loop {
        machines(&app, &fleet, &mut away);
        temporaries(&app, &fleet, &mut leftovers).await;
        let now = Instant::now();

        if slow.due(now) {
            match session.desk().await {
                Ok(desk) => {
                    sync(&app, &desk, &mut conflicts).await;
                    skill_conflicts(&app, &fleet, &mut conflicts).await;
                    failed_moves(&app, &fleet, &desk, &mut moves).await;
                }
                Err(e) => log::debug!("the bell does not read the mesh: {e}"),
            }

            slow.read(now);
        }

        let wake = [Some(now + LOOK_EVERY), away.due(), slow.next()]
            .into_iter()
            .flatten()
            .min()
            .unwrap_or(now + LOOK_EVERY);

        select! {
            changed = changes.changed() => {
                if !changed {
                    return;
                }

                slow.wanted = true;
            }
            heard = local.recv() => match heard {
                Ok(change) if change.topic.name() == "sync" => slow.wanted = true,
                Ok(_) | Err(RecvError::Closed) => {}
                Err(RecvError::Lagged(_)) => slow.wanted = true,
            },
            () = sleep_until(TokioInstant::from_std(wake)) => {}
        }
    }
}

/// When the reads that ask the machines run: soon after a change, but at
/// most once per `SLOW_GAP`, and every `SLOW_EVERY` regardless.
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
        self.next().is_none_or(|next| next <= now)
    }

    /// When the next read is due; `None` for at once.
    fn next(&self) -> Option<Instant> {
        let last = self.last?;
        let gap = if self.wanted { SLOW_GAP } else { SLOW_EVERY };
        Some(last + gap)
    }

    fn read(&mut self, now: Instant) {
        self.last = Some(now);
        self.wanted = false;
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
                history::record(app, note);
            }
            Change::Back(id) => {
                let note = machine_note(id);
                history::finish(app, |n| n.id == note);
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
        history::record(app, note);
    }
}

fn machine_note(id: EndpointId) -> String {
    format!("machine:{id}")
}

/// The conflicts the open note `id` holds, one per line.
fn kept_lines(app: &AppHandle, id: &str) -> BTreeSet<String> {
    history::get(app, id)
        .filter(|note| !note.done)
        .map(|note| note.text.lines().map(str::to_owned).collect())
        .unwrap_or_default()
}

async fn sync(app: &AppHandle, desk: &Desk, conflicts: &mut Conflicts) {
    for project in desk.sync().await {
        for machine in project.machines {
            // An offline machine says nothing of its conflicts.
            if machine.status == (SyncStatus::Offline {}) {
                continue;
            }

            let id = format!("conflict:{}:{}", project.project, machine.machine_id);
            let paths: BTreeSet<String> = machine.conflicts.into_iter().collect();
            match conflicts.step(&id, &paths, || kept_lines(app, &id)) {
                Settle::Same => {}
                Settle::Settled => history::finish(app, |note| note.id == id),
                Settle::Raise => {
                    let title = match paths.len() {
                        1 => format!("Sync conflict in {}", project.project),
                        n => format!("{n} sync conflicts in {}", project.project),
                    };
                    let text = paths.into_iter().collect::<Vec<_>>().join("\n");
                    let note = Note {
                        text,
                        ..Note::new(NoteKind::Conflict, id, title, machine.machine_name)
                    };
                    history::record(app, note);
                }
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
        history::open_of(app, NoteKind::SkillConflict)
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
            Settle::Settled => history::finish(app, |note| note.id == id),
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
                    history::record(app, note);
                }
            }
        }
    }
}

fn skill_note(name: &str) -> String {
    format!("skill:{name}")
}

async fn failed_moves(app: &AppHandle, fleet: &Fleet, desk: &Desk, told: &mut HashSet<String>) {
    let now = now_ms();

    for record in desk.moves() {
        if now.saturating_sub(record.at) > MOVES_FOR_MS {
            continue;
        }

        let id = format!("move:{}:{}", record.id, record.at);

        if told.contains(&id) || history::get(app, &id).is_some() {
            continue;
        }

        let progress = match desk.move_state(&record.id).await {
            Ok(MoveState::Known(progress)) => progress,
            Ok(MoveState::NeedsUpdate) => {
                told.insert(id);
                continue;
            }
            // Not heard of yet, or the target is away: asked again later.
            Ok(MoveState::Unknown) | Err(_) => continue,
        };

        match progress.phase {
            MovePhase::Failed => {
                let note = Note {
                    text: progress.reason.unwrap_or_default(),
                    ..moved(fleet, &record, id.clone())
                };
                history::record(app, note);
                told.insert(id);
            }
            MovePhase::Done | MovePhase::Cancelled => {
                told.insert(id);
            }
            _ => {}
        }
    }
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
