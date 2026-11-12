//! The copies of other machines' sessions this node keeps
//! (`<state>/sessions/<agent>/<id>/`), apart from every harness's store, so
//! a harness never lists one and a spend scan never counts one. A copy
//! enters the store only when the user continues the session here.
//!
//! Beside a copy there may be sessions that lost to another machine's
//! (`lost-<time>.jsonl` with its `lost-<time>.json`): kept until discarded.
//! Everything here blocks.

use super::saved::valid_id;
use beep_proto::AgentSlug;
use iroh::EndpointId;
use serde::{Deserialize, Serialize};
use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

/// What is known of a copy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Meta {
    /// The machine that ran the session.
    pub(super) from: EndpointId,
    pub(super) project: String,
    /// The folder it ran in, relative to the project's checkout.
    pub(super) path: String,
    /// The name the source keeps the transcript under.
    pub(super) file_name: String,
    pub(super) size: u64,
    pub(super) hash: Vec<u8>,
    /// Unix seconds: the session's last activity on the source.
    pub(super) last_activity: u64,
    /// Unix seconds: when the copy was last brought up to date.
    pub(super) fetched_at: u64,
}

/// A note that the user continued a session here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Continued {
    /// Unix seconds: the session's last activity on its machine when the
    /// copy was taken.
    pub(super) base: u64,
    /// Unix seconds: when it was continued here.
    pub(super) at: u64,
}

/// One session in the cache.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Entry {
    pub(super) agent: AgentSlug,
    pub(super) id: String,
    /// The copy of the session, when there is one.
    pub(super) copy: Option<Meta>,
    /// The times (unix seconds) of versions that lost, oldest first.
    pub(super) lost: Vec<u64>,
    /// Bytes on disk of the copy's transcript alone.
    pub(super) copy_len: u64,
    /// Bytes on disk of the copy, and of the versions that lost.
    pub(super) copy_bytes: u64,
    pub(super) lost_bytes: u64,
}

#[derive(Debug, Clone)]
pub(super) struct Cache {
    dir: PathBuf,
    /// Counts the changes made through any clone, so a reader can tell the
    /// cache changed without reading it.
    changes: Arc<AtomicU64>,
}

/// A name a folder can carry for an agent.
fn valid_agent(agent: &AgentSlug) -> bool {
    matches!(agent.as_ref(), "claude" | "codex" | "pi")
}

impl Cache {
    pub(super) fn new(state_dir: &Path) -> Self {
        Self {
            dir: state_dir.join("sessions"),
            changes: Arc::default(),
        }
    }

    /// How many changes were made to the cache since the node started.
    pub(super) fn changes(&self) -> u64 {
        self.changes.load(Ordering::Relaxed)
    }

    fn changed(&self) {
        self.changes.fetch_add(1, Ordering::Relaxed);
    }

    /// The folder of session `id`, when both names are safe in a path.
    fn folder(&self, agent: &AgentSlug, id: &str) -> Option<PathBuf> {
        (valid_agent(agent) && valid_id(id)).then(|| self.dir.join(agent.as_ref()).join(id))
    }

    /// Where the copy of a session is kept.
    pub(super) fn copy(&self, agent: &AgentSlug, id: &str) -> Option<PathBuf> {
        Some(self.folder(agent, id)?.join("copy.jsonl"))
    }

    pub(super) fn meta(&self, agent: &AgentSlug, id: &str) -> Option<Meta> {
        let bytes = fs::read(self.folder(agent, id)?.join("meta.json")).ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    pub(super) fn save_meta(&self, agent: &AgentSlug, id: &str, meta: &Meta) -> io::Result<()> {
        self.changed();
        let folder = self
            .folder(agent, id)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "not a session name"))?;
        fs::create_dir_all(&folder)?;
        let partial = folder.join("meta.json.partial");
        fs::write(&partial, serde_json::to_vec(meta)?)?;
        fs::rename(partial, folder.join("meta.json"))
    }

    /// Every session in the cache.
    pub(super) fn entries(&self) -> Vec<Entry> {
        let mut out = Vec::new();
        let Ok(agents) = fs::read_dir(&self.dir) else {
            return out;
        };

        for agent in agents.flatten() {
            let slug = AgentSlug::from(agent.file_name().to_string_lossy().as_ref());
            let Ok(ids) = fs::read_dir(agent.path()) else {
                continue;
            };

            for id in ids.flatten() {
                let id = id.file_name().to_string_lossy().into_owned();

                if self.folder(&slug, &id).is_none() {
                    continue;
                }

                out.extend(self.entry(&slug, &id));
            }
        }

        out
    }

    /// Session `id` in the cache; `None` when it has no folder.
    pub(super) fn entry(&self, agent: &AgentSlug, id: &str) -> Option<Entry> {
        let files = fs::read_dir(self.folder(agent, id)?).ok()?;
        let mut lost = Vec::new();
        let (mut copy_len, mut copy_bytes, mut lost_bytes) = (0, 0, 0);

        for file in files.flatten() {
            let len = file.metadata().map_or(0, |meta| meta.len());
            let name = file.file_name().to_string_lossy().into_owned();
            if name.starts_with("lost-") {
                lost_bytes += len;
            } else {
                copy_bytes += len;
            }

            if name == "copy.jsonl" {
                copy_len = len;
            }

            if let Some(at) = name
                .strip_prefix("lost-")
                .and_then(|rest| rest.strip_suffix(".jsonl"))
                .and_then(|at| at.parse().ok())
            {
                lost.push(at);
            }
        }

        lost.sort_unstable();
        let copy = self.meta(agent, id);
        Some(Entry {
            agent: agent.clone(),
            id: id.to_owned(),
            copy,
            lost,
            copy_len,
            copy_bytes,
            lost_bytes,
        })
    }

    /// Notes that the user continued the session here at `at` (unix seconds)
    /// from a copy whose session was last active on its machine at `base`.
    pub(super) fn mark_continued(
        &self,
        agent: &AgentSlug,
        id: &str,
        base: u64,
        at: u64,
    ) -> io::Result<()> {
        self.changed();
        let folder = self
            .folder(agent, id)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "not a session name"))?;
        fs::create_dir_all(&folder)?;
        fs::write(folder.join("continued"), format!("{base}\n{at}"))
    }

    /// What `mark_continued` noted, when the session was continued here.
    pub(super) fn continued(&self, agent: &AgentSlug, id: &str) -> Option<Continued> {
        let text = fs::read_to_string(self.folder(agent, id)?.join("continued")).ok()?;
        let mut lines = text.lines();
        Some(Continued {
            base: lines.next()?.trim().parse().ok()?,
            at: lines.next()?.trim().parse().ok()?,
        })
    }

    pub(super) fn clear_continued(&self, agent: &AgentSlug, id: &str) {
        self.changed();
        if let Some(folder) = self.folder(agent, id) {
            let _ = fs::remove_file(folder.join("continued"));
            let _ = fs::remove_dir(folder);
        }
    }

    /// Seconds since the copy of a session was last written, when there is
    /// one on disk.
    pub(super) fn idle_secs(&self, agent: &AgentSlug, id: &str) -> Option<u64> {
        let modified = fs::metadata(self.copy(agent, id)?).ok()?.modified().ok()?;
        modified.elapsed().ok().map(|since| since.as_secs())
    }

    /// Takes away the older copy a restarted pull kept beside the new one.
    pub(super) fn drop_diverged(&self, agent: &AgentSlug, id: &str) {
        self.changed();
        if let Some(folder) = self.folder(agent, id) {
            let _ = fs::remove_file(folder.join("copy.jsonl.diverged"));
        }
    }

    /// Takes the copy of a session away, and with `all` the versions that
    /// lost too.
    pub(super) fn remove(&self, agent: &AgentSlug, id: &str, all: bool) {
        self.changed();
        let Some(folder) = self.folder(agent, id) else {
            return;
        };

        if all {
            // Gone already is as good as removed.
            let _ = fs::remove_dir_all(&folder);
            return;
        }

        for name in [
            "copy.jsonl",
            "meta.json",
            "meta.json.partial",
            "copy.jsonl.diverged",
        ] {
            let _ = fs::remove_file(folder.join(name));
        }
        // An empty folder goes; one that keeps a lost version stays.
        let _ = fs::remove_dir(&folder);
    }

    /// Keeps `transcript`, which lost to another machine's version at `at`,
    /// by moving it in, with what is known of it.
    pub(super) fn keep_lost(
        &self,
        agent: &AgentSlug,
        id: &str,
        transcript: &Path,
        meta: &Meta,
        at: u64,
    ) -> io::Result<u64> {
        self.changed();
        let folder = self
            .folder(agent, id)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "not a session name"))?;
        fs::create_dir_all(&folder)?;
        // A name not taken: two versions set aside within a second must not
        // write over each other.
        let mut at = at;

        while folder.join(format!("lost-{at}.json")).exists() {
            at += 1;
        }

        fs::write(
            folder.join(format!("lost-{at}.json")),
            serde_json::to_vec(meta)?,
        )?;
        let kept = folder.join(format!("lost-{at}.jsonl"));
        // Another disk than the store: copy, then remove.
        if fs::rename(transcript, &kept).is_err() {
            fs::copy(transcript, &kept)?;
            fs::remove_file(transcript)?;
        }
        // Whatever mode the harness gave the file, the kept version is the
        // user's alone.
        #[cfg(unix)]
        fs::set_permissions(&kept, std::os::unix::fs::PermissionsExt::from_mode(0o600))?;
        Ok(at)
    }

    /// Puts the version set aside at `at` back at `to`, when nothing is
    /// there; false when something is, and the version stays listed.
    pub(super) fn put_back(&self, agent: &AgentSlug, id: &str, at: u64, to: &Path) -> bool {
        if to.exists() {
            return false;
        }

        let Some((kept, _)) = self.lost(agent, id, at) else {
            return false;
        };
        let moved = fs::rename(&kept, to).is_ok()
            || (fs::copy(&kept, to).is_ok() && fs::remove_file(&kept).is_ok());
        if moved {
            self.drop_lost(agent, id, at);
        }

        moved
    }

    /// The version that lost at `at`, and what is known of it.
    pub(super) fn lost(&self, agent: &AgentSlug, id: &str, at: u64) -> Option<(PathBuf, Meta)> {
        let folder = self.folder(agent, id)?;
        let meta =
            serde_json::from_slice(&fs::read(folder.join(format!("lost-{at}.json"))).ok()?).ok()?;
        Some((folder.join(format!("lost-{at}.jsonl")), meta))
    }

    /// Takes the version that lost at `at` away.
    pub(super) fn drop_lost(&self, agent: &AgentSlug, id: &str, at: u64) {
        self.changed();
        let Some(folder) = self.folder(agent, id) else {
            return;
        };
        let _ = fs::remove_file(folder.join(format!("lost-{at}.jsonl")));
        let _ = fs::remove_file(folder.join(format!("lost-{at}.json")));
        let _ = fs::remove_dir(&folder);
    }
}

/// Which copies to take away so `need` more bytes of a session last active
/// at `activity` fit under `cap`: the oldest copies first, by last activity
/// (a partial one, with no note, counts as the oldest), and only those older
/// than the session, never `own`, the session's own copy, which counts in
/// the total. A version that lost is never chosen: it is the user's to
/// discard. `None` when `need` cannot fit then.
pub(super) fn evictions(
    entries: &[Entry],
    cap: u64,
    need: u64,
    activity: u64,
    own: Option<(&AgentSlug, &str)>,
) -> Option<Vec<(AgentSlug, String)>> {
    let mut gone = Vec::new();
    let mut total: u64 = entries
        .iter()
        .map(|entry| entry.copy_bytes + entry.lost_bytes)
        .sum();
    let mut copies: Vec<&Entry> = entries
        .iter()
        .filter(|e| e.copy_bytes > 0 && own != Some((&e.agent, e.id.as_str())))
        .collect();
    copies.sort_by_key(|entry| entry.copy.as_ref().map_or(0, |meta| meta.last_activity));

    for entry in copies {
        if total.saturating_add(need) <= cap {
            break;
        }

        if entry
            .copy
            .as_ref()
            .is_some_and(|meta| meta.last_activity >= activity)
        {
            break;
        }
        total = total.saturating_sub(entry.copy_bytes);
        gone.push((entry.agent.clone(), entry.id.clone()));
    }
    (total.saturating_add(need) <= cap).then_some(gone)
}

#[cfg(test)]
#[path = "../../tests/agents_moving_cache.rs"]
mod tests;
