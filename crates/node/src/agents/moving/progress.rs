//! What the moves this node takes part in have done so far: for a target,
//! how far it is (`MoveProgress`, and what its source asks); for a source,
//! that a session it ended is still served as running until it is done
//! with it, and that it gave a move up.

use super::saved::{Folder, SentTree};
use crate::{AsyncMutex, lock};
use beep_proto::node::agent::{MovePhase, MoveProgress};
use beep_proto::watched;
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex, OnceLock},
};
use tokio::{sync::watch, task::spawn_blocking};

/// One move's progress, written by its task.
pub(super) type Tracker = Arc<watch::Sender<MoveProgress>>;

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
    /// The record the move answers.
    at: u64,
    /// The target found it can take the session.
    taken: bool,
    tracker: Tracker,
}

#[derive(Default)]
pub(crate) struct Moves {
    tracked: Mutex<HashMap<String, Tracked>>,
    held: Mutex<HashSet<String>>,
    refused: Mutex<HashMap<String, String>>,
    /// Where the moves are kept, once they are followed.
    kept: OnceLock<Folder>,
    /// Held while a worktree sent is noted, one at a time.
    noting: Arc<Mutex<()>>,
    /// Held while a target places a worktree and notes it, so taking the
    /// move back waits for that to finish.
    pub(super) placing: Arc<AsyncMutex<()>>,
}

/// A session the source ended is served as running while this lives, so a
/// target does not take it before the source is done with it.
pub(super) struct Held<'a> {
    moves: &'a Moves,
    id: String,
}

impl Drop for Held<'_> {
    fn drop(&mut self) {
        lock(&self.moves.held).remove(&self.id);
    }
}

impl Moves {
    /// A fresh tracker for the move of session `id` that the record made at
    /// `at` asked for.
    pub(super) fn begin(&self, id: &str, at: u64) -> Tracker {
        let tracker = Arc::new(watch::Sender::new(MoveProgress::default()));
        lock(&self.tracked).insert(
            id.to_owned(),
            Tracked {
                at,
                taken: false,
                tracker: tracker.clone(),
            },
        );
        tracker
    }

    /// The target of `id` can take the session.
    pub(super) fn take(&self, id: &str) {
        if let Some(one) = lock(&self.tracked).get_mut(id) {
            one.taken = true;
        }
    }

    /// Forgets what this node knew of session `id`'s earlier moves.
    pub(super) fn forget(&self, id: &str) {
        lock(&self.tracked).remove(id);
        lock(&self.refused).remove(id);
    }

    /// How the move of `id` stands here, when this machine is part of one.
    pub(crate) fn progress(&self, id: &str) -> Option<MoveProgress> {
        lock(&self.tracked)
            .get(id)
            .map(|one| watched::read_sent(&one.tracker, Clone::clone))
    }

    /// What the target of the move made at `at` tells its source.
    pub(super) fn target(&self, id: &str, at: u64) -> Target {
        let tracked = lock(&self.tracked);
        let Some(one) = tracked.get(id).filter(|one| one.at == at) else {
            return Target::Unknown;
        };
        let taken = one.taken;

        watched::read_sent(&one.tracker, |progress| match progress.phase {
            MovePhase::Resuming => Target::Starting,
            MovePhase::Done => Target::Landed,
            MovePhase::Failed | MovePhase::Cancelled => Target::Failed(
                progress
                    .reason
                    .clone()
                    .unwrap_or_else(|| "the move did not finish".to_owned()),
            ),
            _ if taken => Target::Taken,
            _ => Target::Unknown,
        })
    }

    /// Whether the move of `id` is past the point where it can be
    /// cancelled: the session is starting or running here.
    pub(super) fn committed(&self, id: &str) -> bool {
        self.progress(id)
            .is_some_and(|progress| matches!(progress.phase, MovePhase::Resuming | MovePhase::Done))
    }

    /// Serves session `id` as running until the guard drops.
    pub(super) fn hold(&self, id: &str) -> Held<'_> {
        lock(&self.held).insert(id.to_owned());
        Held {
            moves: self,
            id: id.to_owned(),
        }
    }

    pub(super) fn holds(&self, id: &str) -> bool {
        lock(&self.held).contains(id)
    }

    /// The source gave the move of `id` up; a target that asks hears why.
    pub(super) fn refuse(&self, id: &str, reason: &str) {
        lock(&self.refused).insert(id.to_owned(), reason.to_owned());
    }

    pub(super) fn refusal(&self, id: &str) -> Option<String> {
        lock(&self.refused).get(id).cloned()
    }

    pub(super) fn keep_in(&self, folder: Folder) {
        let _ = self.kept.set(folder);
    }

    /// The source sent `tree` for session `id`: noted in what is kept, so
    /// it is let go of once the session runs on the target, even after a
    /// restart.
    pub(super) async fn sent(&self, id: &str, tree: SentTree) {
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
    tracker.send_modify(change);
    crate::events::system().changed(beep_proto::node::events::Topic::new("moves"));
}
