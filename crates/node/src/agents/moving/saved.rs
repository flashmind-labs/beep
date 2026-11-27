//! What a node keeps of a move across a restart: how it ended, and the
//! copy of the conversation while it waits (`<state>/moves/`).

use super::worktree::{Moved, Placed};
use crate::agents::OpenSession;
use beep_proto::SessionId;
use iroh::EndpointId;
use serde::{Deserialize, Serialize};
use std::{
    fs, io,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(super) enum Phase {
    /// Not over: a restart does it again from the copy.
    Working,
    /// The source ended the session and waits to hear that the target runs
    /// it; if the target gives up, the session starts again here.
    Closed,
    /// Over: the target runs the session.
    Done,
    /// Over, without moving the session.
    Failed,
}

/// The state of the move that the record made at `at` asked for.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(super) struct Saved {
    pub(super) at: u64,
    pub(super) phase: Phase,
    #[serde(default)]
    pub(super) reason: Option<String>,
    /// What starts the session again at the source, once it ended there.
    #[serde(default)]
    pub(super) open: Option<OpenSession>,
    /// The worktrees a target placed for the move so far, taken back if it
    /// does not happen.
    #[serde(default)]
    pub(super) placed: Vec<PlacedTree>,
    /// For a source: the worktrees it sent, let go of once the session
    /// runs on the target.
    #[serde(default)]
    pub(super) sent: Vec<SentTree>,
    /// Where a source sent the session. A target leaves this empty.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) destination: Option<EndpointId>,
}

#[derive(Clone, Copy)]
pub(super) struct Outgoing {
    pub(super) at: u64,
    pub(super) to: EndpointId,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Previous {
    Released,
    Settling,
    Departed,
}

pub(super) const MOVED_AWAY: &str = "the previous move may still run this session on its target";

/// A worktree a source sent for a move, and what it held then
/// (`worktree::fingerprint`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(super) struct SentTree {
    pub(super) dir: PathBuf,
    pub(super) held: String,
}

/// A worktree a target placed for a move.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(super) struct PlacedTree {
    /// The project's checkout, and the worktree's folder in it.
    pub(super) main: PathBuf,
    pub(super) path: PathBuf,
    pub(super) placed: Placed,
    /// `worktree::fingerprint` once placed.
    pub(super) applied: String,
    /// The branch the move put at the source's commit.
    #[serde(default)]
    pub(super) moved: Option<Moved>,
}

impl Saved {
    pub(super) fn before(&self, next: Outgoing) -> Previous {
        if self.at == next.at && self.destination.is_none_or(|to| to == next.to) {
            return Previous::Released;
        }

        match self.phase {
            Phase::Closed => Previous::Settling,
            Phase::Done if self.destination.is_some() => Previous::Departed,
            _ => Previous::Released,
        }
    }

    /// A move that ended as `phase`.
    pub(super) fn over(at: u64, phase: Phase, reason: Option<String>) -> Self {
        Self {
            at,
            phase,
            reason,
            open: None,
            placed: Vec::new(),
            sent: Vec::new(),
            destination: None,
        }
    }

    /// Over, for this record: nothing is left to do after a restart.
    pub(super) fn is_over(&self, at: u64) -> bool {
        self.at == at && matches!(self.phase, Phase::Done | Phase::Failed)
    }
}

/// The node's `moves` folder.
#[derive(Debug, Clone)]
pub(super) struct Folder {
    dir: PathBuf,
}

pub(super) use crate::text::valid_id;

impl Folder {
    pub(super) fn new(state_dir: &Path) -> Self {
        Self {
            dir: state_dir.join("moves"),
        }
    }

    fn file(&self, id: &SessionId) -> PathBuf {
        self.dir.join(format!("{id}.json"))
    }

    /// Where the conversation is copied to while the move waits.
    pub(super) fn copy(&self, id: &SessionId) -> PathBuf {
        self.dir.join(id.as_ref()).join("copy.jsonl")
    }

    pub(super) fn load(&self, id: &SessionId) -> Option<Saved> {
        serde_json::from_slice(&fs::read(self.file(id)).ok()?).ok()
    }

    pub(super) fn save(&self, id: &SessionId, saved: &Saved) -> io::Result<()> {
        fs::create_dir_all(&self.dir)?;
        let partial = self.dir.join(format!("{id}.json.partial"));
        fs::write(&partial, serde_json::to_vec(saved)?)?;
        fs::rename(partial, self.file(id))
    }

    /// Notes that the source sent `tree` for session `id`, whose session
    /// it ended for the move.
    pub(super) fn keep_sent(&self, id: &SessionId, tree: SentTree) -> io::Result<()> {
        let Some(mut saved) = self.load(id).filter(|saved| saved.phase == Phase::Closed) else {
            return Ok(());
        };
        saved.sent.retain(|sent| sent.dir != tree.dir);
        saved.sent.push(tree);
        self.save(id, &saved)
    }

    pub(super) fn clear_copy(&self, id: &SessionId) {
        // Gone already is as good as removed.
        let _ = fs::remove_dir_all(self.dir.join(id.as_ref()));
    }

    pub(super) fn clear(&self, id: &SessionId, at: u64) {
        if self.load(id).is_none_or(|saved| saved.at == at) {
            self.clear_copy(id);
            let _ = fs::remove_file(self.file(id));
        }
    }
}
