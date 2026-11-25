//! What the moves this node takes part in have done so far: for a target,
//! how far it is (`MoveProgress`, and what its source asks); for a source,
//! that a session it ended is still served as running until it is done
//! with it, and that it gave a move up.

use super::saved::{Folder, Outgoing, Previous, SentTree, valid_id};
use crate::{AsyncMutex, lock};
use beep_proto::{SessionId, watched};
use beep_proto::node::events::{MOVE_PROGRESS_TOPIC, Topic};
use beep_proto::node::agent::{MoveEntry, MovePhase, MoveProgress};
use iroh::EndpointId;
use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::{Arc, Mutex, OnceLock},
};
use tokio::{
    sync::{Notify, watch},
    task::spawn_blocking,
};
use tokio_util::task::AbortOnDropHandle;

/// How many ended moves are kept after their record was replaced, for old
/// sources that ask how one ended and for the push to a desktop that connects late.
const MAX_OUTCOMES: usize = 128;

/// How a move stands on its target. Each change reaches whoever follows it:
/// the source of the move, and the node's `move-progress` push.
#[derive(Default)]
pub(super) struct Tracker(watch::Sender<Landing>);

#[derive(Default)]
pub(super) struct Landing {
    state: LandingState,
    progress: MoveProgress,
}

#[derive(Clone, Copy)]
enum LandingState {
    Preparing { taken: bool },
    Cancelling,
    Settling,
    Finished,
}

impl Default for LandingState {
    fn default() -> Self {
        Self::Preparing { taken: false }
    }
}

impl Tracker {
    pub(super) fn take(&self) {
        let told = self.0.send_if_modified(|landing| match &mut landing.state {
            LandingState::Preparing { taken } if !*taken => {
                *taken = true;
                true
            }
            _ => false,
        });

        if told {
            changed();
        }
    }

    /// Follows what the target says of the move, and how far it is.
    pub(super) fn follow(&self) -> watch::Receiver<Landing> {
        self.0.subscribe()
    }

    /// Claims the move before the task is aborted and its work is undone.
    pub(super) fn request_cancel(&self) -> bool {
        let mut claimed = false;
        self.0.send_if_modified(|landing| {
            claimed = matches!(landing.state, LandingState::Preparing { .. });

            if claimed {
                landing.state = LandingState::Cancelling;
            }

            false
        });
        claimed
    }

    /// The target owns the move from here until it has resumed or rolled back.
    pub(super) fn resume(&self) -> bool {
        let mut resumed = false;
        self.0.send_modify(|landing| {
            resumed = matches!(landing.state, LandingState::Preparing { .. });

            if resumed {
                landing.state = LandingState::Settling;
                landing.progress.phase = MovePhase::Resuming;
            }
        });

        if resumed {
            changed();
        }

        resumed
    }

    pub(super) fn settle(&self) -> bool {
        let mut settled = true;
        self.0.send_if_modified(|landing| {
            match landing.state {
                LandingState::Preparing { .. } => landing.state = LandingState::Settling,
                LandingState::Settling => {}
                LandingState::Cancelling | LandingState::Finished => settled = false,
            }

            false
        });
        settled
    }

    pub(super) fn finish(&self, phase: MovePhase, reason: Option<String>) {
        self.0.send_modify(|landing| {
            landing.state = LandingState::Finished;
            if phase == MovePhase::Cancelled {
                landing.progress = MoveProgress::default();
            }
            landing.progress.phase = phase;
            landing.progress.reason = reason;
        });
        changed();
    }
}

impl Landing {
    /// What the target tells its source.
    pub(super) fn target(&self) -> Target {
        match self.progress.phase {
            MovePhase::Resuming => Target::Starting,
            MovePhase::Done => Target::Landed,
            MovePhase::Failed | MovePhase::Cancelled => Target::Failed(
                self.progress
                    .reason
                    .clone()
                    .unwrap_or_else(|| "the move did not finish".to_owned()),
            ),
            _ if matches!(self.state, LandingState::Preparing { taken: true }) => Target::Taken,
            _ => Target::Unknown,
        }
    }
}

/// What a target tells its source.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(super) enum Target {
    /// Nothing yet: not reachable, or it has not taken the move up.
    #[default]
    Unknown,
    /// It can take the session, so the source may end it.
    Taken,
    /// It starts the session: past taking back, not yet running.
    Starting,
    /// It gave up, and why.
    Failed(String),
    /// The session starts or runs there.
    Landed,
}

struct Tracked {
    at: u64,
    source: EndpointId,
    tracker: Arc<Tracker>,
}

#[derive(Default)]
struct Targets {
    latest: HashMap<SessionId, Tracked>,
    history: VecDeque<(SessionId, Tracked)>,
}

impl Targets {
    fn archive(&mut self, id: &SessionId) {
        if let Some(old) = self.latest.remove(id) {
            if self.history.len() >= MAX_OUTCOMES {
                self.history.pop_front();
            }
            self.history.push_back((id.clone(), old));
        }
    }

    fn find(&self, id: &SessionId, at: u64) -> Option<&Tracked> {
        self.latest.get(id).filter(|one| one.at == at).or_else(|| {
            self.history
                .iter()
                .rev()
                .find_map(|(session, one)| (session == id && one.at == at).then_some(one))
        })
    }
}

#[derive(Default)]
pub(crate) struct Moves {
    tracked: Mutex<Targets>,
    held: Mutex<HashSet<SessionId>>,
    refused: Mutex<HashMap<SessionId, String>>,
    /// Where the moves are kept, once they are followed.
    kept: OnceLock<Folder>,
    /// Held while a worktree sent is noted, one at a time.
    noting: Arc<Mutex<()>>,
    /// Held while a target places a worktree of a session and notes it, so
    /// taking that session's move back waits for that to finish.
    placing: Mutex<HashMap<SessionId, Arc<AsyncMutex<()>>>>,
    /// The take-back of each session's last move that did not happen, which
    /// the session's next move waits for. Moves owns it until it ends.
    cleaning: Mutex<HashMap<SessionId, Cleaning>>,
    /// Told each time a move ends on this side, which leaves room for one
    /// that found none: a chat moves as many sessions at once.
    pub(super) ended: Notify,
    /// Counts each time a session stops being held for a move, or a move is
    /// given up, for a target that waits for the source to be done.
    released: watch::Sender<u64>,
    /// The sessions a target read from this machine for the move it has now,
    /// and a count of those reads.
    contacted: Mutex<HashSet<SessionId>>,
    reached: watch::Sender<u64>,
}

/// A take-back going, and the signal that it ended: the sender lives in the
/// task, so the receiver reports a change, an error, once it is gone.
struct Cleaning {
    task: AbortOnDropHandle<()>,
    done: watch::Receiver<()>,
}

/// A session the source ended is served as running while this lives, so a
/// target does not take it before the source is done with it.
pub(super) struct Held<'a> {
    moves: &'a Moves,
    id: SessionId,
}

impl Drop for Held<'_> {
    fn drop(&mut self) {
        lock(&self.moves.held).remove(&self.id);
        self.moves.release();
    }
}

impl Moves {
    /// A fresh tracker for the move of session `id` that the record made at
    /// `at` asked for.
    pub(super) fn begin(&self, id: &SessionId, at: u64, source: EndpointId) -> Arc<Tracker> {
        let tracker = Arc::new(Tracker::default());
        let mut targets = lock(&self.tracked);
        targets.archive(id);
        targets.latest.insert(
            id.to_owned(),
            Tracked {
                at,
                source,
                tracker: tracker.clone(),
            },
        );
        tracker
    }

    /// Forgets what this node knew of session `id`'s earlier moves.
    pub(super) fn forget(&self, id: &SessionId) {
        lock(&self.tracked).archive(id);
        lock(&self.refused).remove(id);
        lock(&self.contacted).remove(id);
    }

    /// The lock for placing `id`'s worktrees. Only sessions placing now keep one.
    pub(super) fn placing(&self, id: &SessionId) -> Arc<AsyncMutex<()>> {
        let mut all = lock(&self.placing);
        all.retain(|_, one| Arc::strong_count(one) > 1);
        all.entry(id.to_owned()).or_default().clone()
    }

    /// Runs `work`, the take-back of `id`'s move, beside the caller, after
    /// any earlier one of the session. The task stays here, whoever waits for it.
    pub(super) fn clean(&self, id: &SessionId, work: impl Future<Output = ()> + Send + 'static) {
        let mut all = lock(&self.cleaning);
        all.retain(|_, one| !one.task.is_finished());
        let before = all.remove(id);
        let (ending, done) = watch::channel(());
        let task = AbortOnDropHandle::new(tokio::spawn(async move {
            let _ending = ending;

            if let Some(before) = before {
                let _ = before.task.await;
            }

            work.await;
        }));
        all.insert(id.to_owned(), Cleaning { task, done });
    }

    /// Resolves once the take-back of `id`'s last move, if one is going, has
    /// ended, for its next move to wait for.
    pub(super) fn cleaned(&self, id: &SessionId) -> Option<impl Future<Output = ()> + use<>> {
        let mut done = lock(&self.cleaning).get(id)?.done.clone();

        Some(async move {
            let _ = done.changed().await;
        })
    }

    /// How the move of `id` stands here, when this machine is part of one.
    pub(crate) fn progress(&self, id: &SessionId) -> Option<MoveProgress> {
        lock(&self.tracked)
            .latest
            .get(id)
            .map(|one| watched::read_sent(&one.tracker.0, |landing| landing.progress.clone()))
    }

    /// How each move this node is the target of stands, as it pushes it.
    pub(crate) fn entries(&self) -> Vec<MoveEntry> {
        let targets = lock(&self.tracked);
        let kept = targets.history.iter().map(|(session, one)| (session, one));

        targets
            .latest
            .iter()
            .chain(kept)
            .map(|(session, one)| MoveEntry {
                session: session.clone(),
                at: one.at,
                progress: watched::read_sent(&one.tracker.0, |landing| landing.progress.clone()),
            })
            .collect()
    }

    /// The tracker of the move of `id` made at `at`, kept after it ended
    /// for a while.
    pub(super) fn tracker(&self, id: &SessionId, at: u64) -> Option<Arc<Tracker>> {
        lock(&self.tracked)
            .find(id, at)
            .map(|one| one.tracker.clone())
    }

    /// What the target of the move made at `at` tells its source.
    pub(super) fn target(&self, id: &SessionId, at: u64) -> Target {
        lock(&self.tracked)
            .find(id, at)
            .map_or(Target::Unknown, |one| watched::read_sent(&one.tracker.0, Landing::target))
    }

    pub(super) fn owns_target(&self, id: &SessionId, at: u64, source: EndpointId) -> bool {
        lock(&self.tracked)
            .find(id, at)
            .is_some_and(|one| one.source == source)
    }

    pub(super) fn reached(&self) -> watch::Receiver<u64> {
        self.reached.subscribe()
    }

    pub(super) fn contacted(&self, id: &SessionId) -> bool {
        lock(&self.contacted).contains(id)
    }

    /// The target of the move of `id` read from this machine.
    pub(super) fn contact(&self, id: &SessionId) {
        if lock(&self.contacted).insert(id.to_owned()) {
            self.reached.send_modify(|count| *count = count.wrapping_add(1));
        }
    }

    pub(super) fn released(&self) -> watch::Receiver<u64> {
        self.released.subscribe()
    }

    fn release(&self) {
        self.released.send_modify(|count| *count = count.wrapping_add(1));
    }

    /// Serves session `id` as running until the guard drops.
    pub(super) fn hold(&self, id: &SessionId) -> Held<'_> {
        lock(&self.held).insert(id.to_owned());
        Held {
            moves: self,
            id: id.to_owned(),
        }
    }

    pub(super) fn holds(&self, id: &SessionId) -> bool {
        lock(&self.held).contains(id)
    }

    /// The source gave the move of `id` up; a target that asks hears why.
    pub(super) fn refuse(&self, id: &SessionId, reason: &str) {
        lock(&self.refused).insert(id.to_owned(), reason.to_owned());
        self.release();
    }

    pub(super) fn refusal(&self, id: &SessionId) -> Option<String> {
        lock(&self.refused).get(id).cloned()
    }

    pub(super) fn keep_in(&self, folder: Folder) {
        let _ = self.kept.set(folder);
    }

    pub(super) async fn preceding(&self, id: &SessionId, to: Outgoing) -> Previous {
        if !valid_id(id.as_ref()) {
            return Previous::Departed;
        }

        let Some(folder) = self.kept.get().cloned() else {
            return Previous::Released;
        };
        let id = id.clone();
        spawn_blocking(move || {
            folder
                .load(&id)
                .map_or(Previous::Released, |saved| saved.before(to))
        })
        .await
        .unwrap_or(Previous::Settling)
    }

    /// The source sent `tree` for session `id`: noted in what is kept, so
    /// it is let go of once the session runs on the target, even after a
    /// restart.
    pub(super) async fn sent(&self, id: &SessionId, tree: SentTree) {
        let Some(folder) = self.kept.get().cloned() else {
            return;
        };
        let (noting, id) = (self.noting.clone(), id.to_owned());
        let noted = spawn_blocking(move || {
            let _one = lock(&noting);
            folder.keep_sent(&id, tree)
        })
        .await;

        match noted {
            Ok(Ok(())) => {}
            Ok(Err(e)) => log::warn!("a worktree sent for a move is not noted: {e}"),
            Err(e) => log::warn!("noting a worktree sent for a move failed: {e}"),
        }
    }
}

/// Changes the progress shown, keeping what `change` leaves alone.
pub(super) fn show(tracker: &Tracker, change: impl FnOnce(&mut MoveProgress)) {
    let shown = tracker.0.send_if_modified(|landing| {
        if !matches!(landing.state, LandingState::Preparing { .. }) {
            return false;
        }

        let before = landing.progress.clone();
        change(&mut landing.progress);
        landing.progress != before
    });

    if shown {
        changed();
    }
}

fn changed() {
    let events = crate::events::system();
    events.changed(Topic::new("moves"));
    events.changed(Topic::new(MOVE_PROGRESS_TOPIC));
}
