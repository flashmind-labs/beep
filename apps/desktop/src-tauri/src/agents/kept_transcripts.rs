//! Each session's newest transcript page and follow cursor, kept on disk so a
//! session opened after a restart or in a new window draws at once, then
//! follows from the cursor. One file per session in the app data folder,
//! beside the kept views; the newest `MAX_SESSIONS` stay, within `MAX_BYTES`.

use crate::error::{DesktopError, Result};
use beep_proto::node::agent::SessionCursor;
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, PoisonError},
    time::SystemTime,
};
use tauri::State;

/// The most sessions kept; writing another drops the oldest written.
const MAX_SESSIONS: usize = 64;
/// The most bytes all kept sessions take together.
const MAX_BYTES: u64 = 32 * 1024 * 1024;
/// The most one session's page may take.
const MAX_ONE: usize = 4 * 1024 * 1024;

/// What a window kept of one session.
#[derive(Debug, Serialize, Deserialize)]
pub struct KeptTranscript {
    /// The page as the window drew it, as `agent_follow` sends it.
    page: Value,
    /// Where the page ends, for `agent_follow`'s `after`; `None` for a page not read by
    /// following.
    cursor: Option<SessionCursor>,
}

/// One session, as the commands name it.
struct Key<'a> {
    machine_id: &'a str,
    agent: &'a str,
    session: &'a str,
}

pub struct KeptTranscripts {
    dir: PathBuf,
    /// Held while a page is written and the oldest dropped, so two writes never drop each
    /// other's file.
    writing: Mutex<()>,
}

impl KeptTranscripts {
    pub fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            writing: Mutex::new(()),
        }
    }

    fn file(&self, key: &Key<'_>) -> PathBuf {
        let name = format!("{}\n{}\n{}", key.machine_id, key.agent, key.session);
        self.dir.join(format!(
            "{}.json",
            utf8_percent_encode(&name, NON_ALPHANUMERIC)
        ))
    }

    fn read(&self, key: &Key<'_>) -> Option<KeptTranscript> {
        let file = self.file(key);
        let bytes = match fs::read(&file) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == ErrorKind::NotFound => return None,
            Err(e) => {
                log::warn!("reading {}: {e}", file.display());
                return None;
            }
        };

        match serde_json::from_slice(&bytes) {
            Ok(kept) => Some(kept),
            Err(e) => {
                log::info!("dropping the unreadable {}: {e}", file.display());
                remove(&file);
                None
            }
        }
    }

    fn write(&self, key: &Key<'_>, kept: &KeptTranscript) -> Result<()> {
        let bytes = serde_json::to_vec(kept).map_err(anyhow::Error::from)?;

        if bytes.len() > MAX_ONE {
            return Err(DesktopError::BadRequest(format!(
                "the page is {} bytes, over the {MAX_ONE} kept",
                bytes.len()
            )));
        }

        let _writing = self.writing.lock().unwrap_or_else(PoisonError::into_inner);
        let file = self.file(key);
        let mut partial = file.as_os_str().to_owned();
        partial.push(".part");
        fs::create_dir_all(&self.dir)
            .and_then(|()| fs::write(&partial, &bytes))
            .and_then(|()| fs::rename(&partial, &file))
            .map_err(|e| anyhow::anyhow!("writing {}: {e}", file.display()))?;
        trim(&self.dir, MAX_SESSIONS, MAX_BYTES);
        Ok(())
    }

    fn forget(&self, key: &Key<'_>) {
        let _writing = self.writing.lock().unwrap_or_else(PoisonError::into_inner);
        remove(&self.file(key));
    }

    /// Drops every kept session, when this computer starts over.
    pub fn clear(&self) {
        let _writing = self.writing.lock().unwrap_or_else(PoisonError::into_inner);

        match fs::remove_dir_all(&self.dir) {
            Ok(()) => {}
            Err(e) if e.kind() == ErrorKind::NotFound => {}
            Err(e) => log::warn!("removing {}: {e}", self.dir.display()),
        }
    }
}

/// Keeps the newest written `sessions` files in `dir` that fit in `bytes` together.
fn trim(dir: &Path, sessions: usize, bytes: u64) {
    let Ok(read) = fs::read_dir(dir) else {
        return;
    };
    let mut files: Vec<(SystemTime, u64, PathBuf)> = read
        .flatten()
        .filter_map(|entry| {
            let meta = entry.metadata().ok()?;
            let written = meta.modified().ok()?;
            Some((written, meta.len(), entry.path()))
        })
        .filter(|(_, _, path)| path.extension().is_some_and(|ext| ext == "json"))
        .collect();
    files.sort_by_key(|(written, _, _)| std::cmp::Reverse(*written));
    let mut total = 0u64;

    for (kept, (_, size, path)) in files.into_iter().enumerate() {
        total = total.saturating_add(size);

        if kept >= sessions || total > bytes {
            remove(&path);
        }
    }
}

fn remove(file: &Path) {
    match fs::remove_file(file) {
        Ok(()) => {}
        Err(e) if e.kind() == ErrorKind::NotFound => {}
        Err(e) => log::warn!("removing {}: {e}", file.display()),
    }
}

/// Runs `work` on a blocking thread, so disk work stays off the async workers.
async fn blocking<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> Result<T> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|e| DesktopError::Other(anyhow::anyhow!("the kept transcripts stopped: {e}")))
}

/// The page and cursor kept for a session, or `None`.
#[tauri::command]
pub async fn transcript_kept(
    kept: State<'_, Arc<KeptTranscripts>>,
    machine_id: String,
    agent: String,
    session: String,
) -> Result<Option<KeptTranscript>> {
    let kept = kept.inner().clone();
    blocking(move || {
        kept.read(&Key {
            machine_id: &machine_id,
            agent: &agent,
            session: &session,
        })
    })
    .await
}

/// Keeps a session's newest page and its cursor, replacing what was kept.
#[tauri::command]
pub async fn transcript_keep(
    kept: State<'_, Arc<KeptTranscripts>>,
    machine_id: String,
    agent: String,
    session: String,
    page: Value,
    cursor: Option<SessionCursor>,
) -> Result<()> {
    let kept = kept.inner().clone();
    blocking(move || {
        kept.write(
            &Key {
                machine_id: &machine_id,
                agent: &agent,
                session: &session,
            },
            &KeptTranscript { page, cursor },
        )
    })
    .await?
}

/// Drops what was kept of a session, when it is deleted.
#[tauri::command]
pub async fn transcript_forget(
    kept: State<'_, Arc<KeptTranscripts>>,
    machine_id: String,
    agent: String,
    session: String,
) -> Result<()> {
    let kept = kept.inner().clone();
    blocking(move || {
        kept.forget(&Key {
            machine_id: &machine_id,
            agent: &agent,
            session: &session,
        })
    })
    .await
}

#[cfg(test)]
#[path = "../tests/agents_kept_transcripts.rs"]
mod tests;
