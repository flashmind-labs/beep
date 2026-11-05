//! What a node keeps of a move across a restart: how it ended, and the
//! copy of the conversation while it waits (`<state>/moves/`).

use super::worktree::Placed;
use crate::agents::OpenSession;
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
    /// The worktree a target put the session on, until it runs: a restart
    /// that finds it takes it back before it starts over.
    #[serde(default)]
    pub(super) made: Option<MadeWorktree>,
}

/// A worktree a target made or reused for a move.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct MadeWorktree {
    /// This machine's checkout, and the worktree's folder.
    pub(super) checkout: PathBuf,
    pub(super) folder: PathBuf,
    pub(super) placed: Placed,
    /// What the worktree held once the move's patch was in it: the folder is
    /// taken back only while it still holds exactly that.
    pub(super) applied: String,
}

impl Saved {
    /// A move that ended as `phase`.
    pub(super) fn over(at: u64, phase: Phase, reason: Option<String>) -> Self {
        Self {
            at,
            phase,
            reason,
            open: None,
            made: None,
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

/// An id a file name can carry: ids come from the mesh's state.
pub(super) fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
}

impl Folder {
    pub(super) fn new(state_dir: &Path) -> Self {
        Self {
            dir: state_dir.join("moves"),
        }
    }

    fn file(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.json"))
    }

    /// Where the conversation is copied to while the move waits.
    pub(super) fn copy(&self, id: &str) -> PathBuf {
        self.dir.join(id).join("copy.jsonl")
    }

    /// The sessions with a worktree a target made for a move that did not
    /// finish.
    pub(super) fn with_made(&self) -> Vec<String> {
        let Ok(files) = fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        files
            .flatten()
            .filter_map(|file| {
                let name = file.file_name().to_str()?.strip_suffix(".json")?.to_owned();
                self.load(&name)?.made.map(|_| name)
            })
            .collect()
    }

    pub(super) fn load(&self, id: &str) -> Option<Saved> {
        serde_json::from_slice(&fs::read(self.file(id)).ok()?).ok()
    }

    pub(super) fn save(&self, id: &str, saved: &Saved) -> io::Result<()> {
        fs::create_dir_all(&self.dir)?;
        let partial = self.dir.join(format!("{id}.json.partial"));
        fs::write(&partial, serde_json::to_vec(saved)?)?;
        fs::rename(partial, self.file(id))
    }

    /// Takes the copy away, and the saved state with `state`.
    pub(super) fn clear(&self, id: &str, state: bool) {
        // Gone already is as good as removed.
        let _ = fs::remove_dir_all(self.dir.join(id));
        if state {
            let _ = fs::remove_file(self.file(id));
        }
    }
}
