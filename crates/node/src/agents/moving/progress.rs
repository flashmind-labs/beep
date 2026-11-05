//! What the moves this node takes part in have done so far: for a target,
//! how far it is (`MoveProgress`, and what its source asks); for a source,
//! that a session it ended is still served as running until its files are
//! scanned, and that it gave a move up.

use super::{Env, offers::project_folder};
use crate::lock;
use beep_proto::node::agent::{MovePhase, MoveProgress};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicBool, Ordering},
    },
    time::SystemTime,
};
use tokio::sync::watch;

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
    /// It gave up, and why.
    Failed(String),
    /// The session runs there.
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
    /// The worktree a session being moved out of works in.
    worktrees: Mutex<HashMap<String, PathBuf>>,
    /// What each of those held when it was sent.
    sent: Mutex<HashMap<String, String>>,
    /// Worktree sessions whose snapshot a live read already tried.
    previewed: Mutex<HashSet<String>>,
    /// What the moves and copies of this node share, for requests that
    /// continue a copy here.
    env: Mutex<Weak<Env>>,
    hashes: Hashes,
    /// Sessions being continued from a copy right now.
    continuing: Mutex<HashSet<String>>,
    /// Background copying waits for a session of this node.
    paused: AtomicBool,
}

/// A session being continued from its copy, until this drops: a second
/// continue of it is refused meanwhile.
pub(super) struct Continuing<'a> {
    moves: &'a Moves,
    id: String,
}

impl Drop for Continuing<'_> {
    fn drop(&mut self) {
        lock(&self.moves.continuing).remove(&self.id);
    }
}

/// The hash of each transcript offered, read again only when its size or
/// time changed.
#[derive(Clone, Default)]
pub(super) struct Hashes(Arc<Mutex<HashMap<PathBuf, Hashed>>>);

/// A transcript's hash, with the size and time it was read at.
struct Hashed {
    size: u64,
    modified: SystemTime,
    hash: Vec<u8>,
}

/// The most transcripts whose hashes are kept.
const MAX_HASHES: usize = 5000;

impl Hashes {
    pub(super) fn get(&self, file: &Path, size: u64, modified: SystemTime) -> Option<Vec<u8>> {
        let kept = lock(&self.0);
        let found = kept.get(file)?;
        (found.size == size && found.modified == modified).then(|| found.hash.clone())
    }

    pub(super) fn put(&self, file: &Path, size: u64, modified: SystemTime, hash: Vec<u8>) {
        let mut kept = lock(&self.0);
        if kept.len() >= MAX_HASHES {
            kept.clear();
        }
        kept.insert(
            file.to_owned(),
            Hashed {
                size,
                modified,
                hash,
            },
        );
    }
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
        lock(&self.worktrees).remove(id);
        lock(&self.sent).remove(id);
        lock(&self.previewed).remove(id);
    }

    /// The worktree of session `id` is `dir`: a target asks for its commit
    /// and uncommitted work once the session ended.
    pub(super) fn set_worktree(&self, id: &str, dir: PathBuf) {
        lock(&self.worktrees).insert(id.to_owned(), dir);
    }

    /// The fingerprint of what the worktree of session `id` held when its
    /// commit and work were sent.
    pub(super) fn set_sent(&self, id: &str, fingerprint: String) {
        lock(&self.sent).insert(id.to_owned(), fingerprint);
    }

    /// True the first time it is asked for session `id`: a live read of a
    /// worktree session tries the snapshot once, not on every poll.
    pub(super) fn first_preview(&self, id: &str) -> bool {
        lock(&self.previewed).insert(id.to_owned())
    }

    pub(super) fn sent_of(&self, id: &str) -> Option<String> {
        lock(&self.sent).get(id).cloned()
    }

    pub(super) fn worktree_of(&self, id: &str) -> Option<PathBuf> {
        lock(&self.worktrees).get(id).cloned()
    }

    /// Starts continuing session `id`; `None` when that is going already.
    pub(super) fn begin_continue(&self, id: &str) -> Option<Continuing<'_>> {
        lock(&self.continuing)
            .insert(id.to_owned())
            .then(|| Continuing {
                moves: self,
                id: id.to_owned(),
            })
    }

    /// Notes whether background copying waits; true when that changed.
    pub(super) fn set_paused(&self, paused: bool) -> bool {
        self.paused.swap(paused, Ordering::Relaxed) != paused
    }

    pub(super) fn clone_hashes(&self) -> Hashes {
        self.hashes.clone()
    }

    pub(super) fn use_env(&self, env: Weak<Env>) {
        *lock(&self.env) = env;
    }

    /// Continues the copy of session `id` of `agent` here.
    pub(crate) async fn continue_copy(
        &self,
        agent: &str,
        id: &str,
        lost: Option<u64>,
    ) -> anyhow::Result<()> {
        let env = lock(&self.env)
            .upgrade()
            .ok_or_else(|| anyhow::anyhow!("this node does not copy sessions"))?;
        super::resume::continue_here(&env, &beep_proto::AgentSlug::from(agent), id, lost).await
    }

    /// The copies this node keeps, and the versions that lost.
    pub(crate) async fn copies(&self) -> Vec<beep_proto::node::agent::CopyView> {
        let env = lock(&self.env).upgrade();
        match env {
            Some(env) => super::background::views(&env).await,
            None => Vec::new(),
        }
    }

    /// Throws a copy away, or the version set aside at `lost`.
    pub(crate) async fn discard_copy(&self, agent: &str, id: &str, lost: Option<u64>) {
        let env = lock(&self.env).upgrade();
        if let Some(env) = env {
            super::background::discard(&env, &beep_proto::AgentSlug::from(agent), id, lost).await;
        }
    }

    /// Whether Syncthing keeps `project` here and its `.stignore` leaves
    /// out a worktree on `branch`.
    pub(crate) async fn ignores_worktree(&self, project: &str, branch: &str) -> bool {
        let env = lock(&self.env).upgrade();
        let Some(env) = env else {
            return false;
        };
        let (Some(syncs), Some(folder)) = (
            &env.syncs,
            project_folder(&env.mesh, project, &env.mesh.me()),
        ) else {
            return false;
        };
        syncs
            .syncthing_ignores(
                Path::new(&folder),
                &beep_proto::node::agent::worktree_path(branch),
            )
            .await
    }

    /// How the move of `id` stands here, when this machine is part of one.
    pub(crate) fn progress(&self, id: &str) -> Option<MoveProgress> {
        lock(&self.tracked)
            .get(id)
            .map(|one| one.tracker.borrow().clone())
    }

    /// What the target of the move made at `at` tells its source.
    pub(super) fn target(&self, id: &str, at: u64) -> Target {
        let tracked = lock(&self.tracked);
        let Some(one) = tracked.get(id).filter(|one| one.at == at) else {
            return Target::Unknown;
        };
        let progress = one.tracker.borrow();
        match progress.phase {
            MovePhase::Done => Target::Landed,
            MovePhase::Failed | MovePhase::Cancelled => Target::Failed(
                progress
                    .reason
                    .clone()
                    .unwrap_or_else(|| "the move did not finish".to_owned()),
            ),
            _ if one.taken => Target::Taken,
            _ => Target::Unknown,
        }
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
}

/// Changes the progress shown, keeping what `change` leaves alone.
pub(super) fn show(tracker: &Tracker, change: impl FnOnce(&mut MoveProgress)) {
    tracker.send_modify(change);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_session_is_continued_by_one_request_at_a_time() {
        let moves = Moves::default();
        let first = moves.begin_continue("s1").expect("the first goes");
        assert!(
            moves.begin_continue("s1").is_none(),
            "the second is refused"
        );
        assert!(
            moves.begin_continue("s2").is_some(),
            "another session is free"
        );
        drop(first);
        assert!(
            moves.begin_continue("s1").is_some(),
            "free again once it ended"
        );
    }
}
