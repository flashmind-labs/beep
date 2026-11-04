//! What a project keeps on disk between runs, and its state in memory.

use crate::files::sync::{
    map::{Map, Replica, Version},
    root::RootId,
    scan::Stat,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fs,
    path::Path,
};

/// The last change this node has from a peer's replica.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(in crate::files::sync) struct Cursor {
    pub(in crate::files::sync) replica: Replica,
    pub(in crate::files::sync) seq: u64,
}

/// A file as the last scan or write left it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct Seen {
    pub(super) stat: Stat,
    #[serde(with = "serde_bytes")]
    pub(super) hash: [u8; 32],
    /// The version of the list entry its content is.
    pub(super) version: Version,
}

/// What a project keeps on disk between runs.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(super) struct Saved {
    pub(super) map: Map,
    pub(super) seen: BTreeMap<String, Seen>,
    /// By peer key.
    pub(super) cursors: BTreeMap<String, Cursor>,
    /// The folder it belongs to.
    #[serde(default)]
    pub(super) root: Option<RootId>,
    /// The peers whose checkout was found fit to merge with (see
    /// `Shared::first_contact`), by peer key.
    #[serde(default)]
    pub(super) matched: BTreeSet<String>,
    /// A ref HEAD names moved, and the index is not rebuilt yet.
    #[serde(default)]
    pub(super) index_stale: bool,
}

pub(super) struct State {
    pub(super) saved: Saved,
    /// How many seen files hold each hash: what this node can serve.
    pub(super) have: HashMap<[u8; 32], u32>,
    pub(super) unsaved: bool,
}

impl State {
    pub(super) fn new(saved: Saved) -> Self {
        let mut have = HashMap::new();
        for seen in saved.seen.values() {
            *have.entry(seen.hash).or_default() += 1;
        }
        Self {
            saved,
            have,
            unsaved: false,
        }
    }

    /// A copy of what to save, once after each change.
    pub(super) fn snapshot(&mut self) -> Option<Saved> {
        if !self.unsaved {
            return None;
        }
        self.unsaved = false;
        Some(self.saved.clone())
    }

    pub(super) fn set_seen(&mut self, path: &str, seen: Seen) {
        *self.have.entry(seen.hash).or_default() += 1;
        if let Some(old) = self.saved.seen.insert(path.to_owned(), seen) {
            self.forget_hash(&old.hash);
        }
        self.unsaved = true;
    }

    pub(super) fn remove_seen(&mut self, path: &str) -> Option<Seen> {
        let old = self.saved.seen.remove(path)?;
        self.forget_hash(&old.hash);
        self.unsaved = true;
        Some(old)
    }

    fn forget_hash(&mut self, hash: &[u8; 32]) {
        if let Some(count) = self.have.get_mut(hash) {
            *count -= 1;
            if *count == 0 {
                self.have.remove(hash);
            }
        }
    }
}

pub(super) fn read_saved(file: &Path) -> Saved {
    let bytes = match fs::read(file) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Saved::default(),
        Err(e) => {
            log::warn!("starting over: could not read {}: {e}", file.display());
            return Saved::default();
        }
    };
    match rmp_serde::from_slice::<Saved>(&bytes) {
        Ok(mut saved) => {
            saved.map = saved.map.loaded();
            saved
        }
        Err(e) => {
            // Kept aside, not lost, and a fresh replica starts.
            let aside = file.with_extension("unreadable");
            log::warn!(
                "starting over: {} is unreadable ({e}); kept as {}",
                file.display(),
                aside.display()
            );
            if let Err(e) = fs::rename(file, &aside) {
                log::warn!("could not keep {}: {e}", file.display());
            }
            Saved::default()
        }
    }
}
