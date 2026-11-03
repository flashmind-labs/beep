//! The sessions Beep started on this node, kept in `sessions.json` in the
//! node's folder so they outlive a restart. A session goes by its agent's
//! own id (Codex's thread, Claude Code's session), which the agent's own
//! logs still know after a restart; the ledger only adds what those logs
//! cannot say: that Beep started it, the short id it went by before the
//! agent gave one, and how it ended. Its folders are reachable by file
//! requests without a look in the agents' logs.

use crate::AsyncMutex;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
    sync::{Mutex, MutexGuard, OnceLock, PoisonError},
};
use tokio::task::spawn_blocking;

/// Why a session ended with the node.
pub(super) const NODE_STOPPED: &str = "beep-node restarted; send a message to resume the session";

/// The file in the node's folder.
const FILE: &str = "sessions.json";
/// The most sessions kept; the oldest go first.
const MAX_ENTRIES: usize = 200;

/// One session Beep started.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(super) struct Entry {
    /// The id it started under on the node: the agent's own when it was
    /// known from the start (a resumed session), else a short one.
    pub(super) id: String,
    /// The agent's own id, once it gave one.
    #[serde(default)]
    pub(super) agent_session: Option<String>,
    /// The agent, such as `codex`.
    pub(super) agent: String,
    pub(super) cwd: PathBuf,
    /// Unix seconds.
    pub(super) started_at: u64,
    /// Why it ended, once it did.
    #[serde(default)]
    pub(super) ended: Option<String>,
}

impl Entry {
    /// Whether `id` names it, by either id.
    fn named(&self, id: &str) -> bool {
        self.id == id || self.agent_session.as_deref() == Some(id)
    }

    /// The id it goes by: the agent's own once known.
    pub(super) fn current_id(&self) -> &str {
        self.agent_session.as_deref().unwrap_or(&self.id)
    }
}

#[derive(Debug, Default)]
pub(super) struct Ledger {
    /// Where it is saved; unset, it is kept only in memory.
    path: OnceLock<PathBuf>,
    entries: Mutex<Vec<Entry>>,
    /// Saves one at a time, so an older snapshot never lands last.
    saving: AsyncMutex<()>,
}

impl Ledger {
    /// Reads the ledger in `dir` and saves it there from now on. Sessions
    /// it lists as running ended with the node. Set once.
    pub(super) async fn open(&self, dir: &Path) -> Result<()> {
        let path = dir.join(FILE);
        if self.path.set(path.clone()).is_err() {
            anyhow::bail!("the session ledger was opened already");
        }
        let read = spawn_blocking(move || read(&path))
            .await
            .context("reading the session ledger stopped")??;
        {
            let mut entries = self.lock();
            let started_here: Vec<Entry> = std::mem::take(&mut *entries);
            *entries = read;
            for entry in entries.iter_mut() {
                entry.ended.get_or_insert_with(|| NODE_STOPPED.to_owned());
            }
            // Sessions started before it was read still run.
            for entry in started_here {
                upsert(&mut entries, entry);
            }
        }
        self.save().await
    }

    fn lock(&self) -> MutexGuard<'_, Vec<Entry>> {
        self.entries.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Adds or replaces the session `entry.id`.
    pub(super) fn record(&self, entry: Entry) {
        upsert(&mut self.lock(), entry);
    }

    /// Notes the agent's own id of session `id`.
    pub(super) fn named_by_agent(&self, id: &str, agent_session: &str) {
        if let Some(entry) = self.lock().iter_mut().find(|entry| entry.named(id)) {
            entry.agent_session = Some(agent_session.to_owned());
        }
    }

    /// Notes that session `id` ended, and why.
    pub(super) fn ended(&self, id: &str, reason: &str) {
        if let Some(entry) = self.lock().iter_mut().find(|entry| entry.named(id)) {
            entry.ended = Some(reason.to_owned());
        }
    }

    /// The session `id` names, by either of its ids; the newest when two
    /// share the agent's id, as a resumed one does.
    pub(super) fn find(&self, id: &str) -> Option<Entry> {
        self.lock()
            .iter()
            .filter(|entry| entry.named(id))
            .max_by_key(|entry| entry.started_at)
            .cloned()
    }

    /// Every session, oldest first.
    pub(super) fn entries(&self) -> Vec<Entry> {
        self.lock().clone()
    }

    /// The folders the sessions worked in.
    pub(super) fn folders(&self) -> Vec<PathBuf> {
        let mut folders: Vec<PathBuf> = self.lock().iter().map(|entry| entry.cwd.clone()).collect();
        folders.sort();
        folders.dedup();
        folders
    }

    /// Writes the ledger to its file, when it has one.
    pub(super) async fn save(&self) -> Result<()> {
        let Some(path) = self.path.get().cloned() else {
            return Ok(());
        };
        let _saving = self.saving.lock().await;
        let entries = self.entries();
        spawn_blocking(move || write(&path, &entries))
            .await
            .context("saving the session ledger stopped")?
    }
}

/// Puts `entry` in place of the one with its id, keeping at most
/// `MAX_ENTRIES`, oldest first.
fn upsert(entries: &mut Vec<Entry>, entry: Entry) {
    match entries.iter_mut().find(|old| old.id == entry.id) {
        Some(old) => *old = entry,
        None => entries.push(entry),
    }
    entries.sort_by_key(|entry| entry.started_at);
    let extra = entries.len().saturating_sub(MAX_ENTRIES);
    entries.drain(..extra);
}

fn read(path: &Path) -> Result<Vec<Entry>> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
    };
    match serde_json::from_str(&text) {
        Ok(entries) => Ok(entries),
        Err(e) => {
            // A ledger it cannot read only loses what sessions Beep
            // started; it is written anew.
            log::warn!("{} is not a session ledger: {e}", path.display());
            Ok(Vec::new())
        }
    }
}

/// Writes `entries` to `path` through a file beside it, so a crash leaves
/// the old ledger or the new one.
fn write(path: &Path, entries: &[Entry]) -> Result<()> {
    let text = serde_json::to_string_pretty(entries).context("encoding the session ledger")?;
    let part = path.with_extension("json.part");
    fs::write(&part, text).with_context(|| format!("writing {}", part.display()))?;
    fs::rename(&part, path).with_context(|| format!("writing {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn entry(id: &str, started_at: u64) -> Entry {
        Entry {
            id: id.into(),
            agent_session: None,
            agent: "codex".into(),
            cwd: PathBuf::from("/srv/project"),
            started_at,
            ended: None,
        }
    }

    #[tokio::test]
    async fn a_running_session_ends_with_the_node_and_keeps_its_agent_id() {
        let dir = TempDir::new().unwrap();
        let before = Ledger::default();
        before.open(dir.path()).await.unwrap();
        before.record(entry("da193d", 10));
        before.named_by_agent("da193d", "01a1a19e-d549-7941-860f-9e2f02fbbe5a");
        before.save().await.unwrap();

        let after = Ledger::default();
        after.open(dir.path()).await.unwrap();
        let found = after.entries().pop().unwrap();
        assert_eq!(found.id, "da193d");
        assert_eq!(
            found.agent_session.as_deref(),
            Some("01a1a19e-d549-7941-860f-9e2f02fbbe5a")
        );
        assert_eq!(found.ended.as_deref(), Some(NODE_STOPPED));
        assert_eq!(after.folders(), [PathBuf::from("/srv/project")]);
    }

    #[test]
    fn keeps_the_newest_entries() {
        let ledger = Ledger::default();
        for at in 0..(MAX_ENTRIES as u64 + 5) {
            ledger.record(entry(&format!("{at:06x}"), at));
        }
        let entries = ledger.entries();
        assert_eq!(entries.len(), MAX_ENTRIES);
        assert_eq!(entries[0].started_at, 5);
    }

    #[test]
    fn the_newest_of_two_with_one_agent_id_is_found() {
        let ledger = Ledger::default();
        let mut first = entry("aaaaaa", 1);
        first.agent_session = Some("thread".into());
        first.ended = Some("closed".into());
        let mut second = entry("thread", 2);
        second.agent_session = Some("thread".into());
        ledger.record(first);
        ledger.record(second);
        assert_eq!(ledger.find("thread").unwrap().started_at, 2);
        assert_eq!(ledger.find("aaaaaa").unwrap().current_id(), "thread");
    }

    #[test]
    fn a_broken_file_reads_as_empty() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join(FILE), "{not json").unwrap();
        assert!(read(&dir.path().join(FILE)).unwrap().is_empty());
    }
}
