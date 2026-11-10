//! Each session's newest transcript page and follow cursor, kept on disk so a
//! session opened after a restart or in a new window draws at once, then
//! follows from the cursor. The desktop's session follow writes them as it
//! reads; the webview only reads them. One file per session in the app data
//! folder, beside the kept views; the newest `MAX_SESSIONS` stay, within
//! `MAX_BYTES`. One thread does all the disk work, in the order asked.

use crate::error::{DesktopError, Result};
use beep_proto::node::agent::SessionCursor;
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::VecDeque,
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
    sync::{Arc, mpsc},
    thread,
    time::SystemTime,
};
use tauri::State;
use tokio::sync::oneshot;

/// The most sessions kept; writing another drops the oldest written.
const MAX_SESSIONS: usize = 64;
/// The most bytes all kept sessions take together.
const MAX_BYTES: u64 = 32 * 1024 * 1024;
/// The most one session's page may take.
const MAX_ONE: usize = 4 * 1024 * 1024;
/// How many deleted sessions are remembered, so a follow ending after the
/// delete does not write its page back.
const FORGOTTEN: usize = 64;

/// What was kept of one session.
#[derive(Debug, Serialize, Deserialize)]
pub struct KeptTranscript {
    /// The page as `agent_follow` sends it, with its updates joined.
    pub(crate) page: Value,
    /// Where the page ends, for `agent_follow`'s `after`; `None` for a page not read by
    /// following.
    pub(crate) cursor: Option<SessionCursor>,
}

/// One session, as the commands name it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SessionKey {
    pub(crate) machine_id: String,
    pub(crate) agent: String,
    pub(crate) session: String,
}

type Job = Box<dyn FnOnce(&mut Store) + Send>;

/// Hands the disk work to the kept transcripts' thread.
pub struct KeptTranscripts {
    jobs: mpsc::Sender<Job>,
}

impl KeptTranscripts {
    pub fn new(dir: PathBuf) -> Self {
        let (jobs, queue) = mpsc::channel::<Job>();
        let mut store = Store::new(dir);
        let spawned = thread::Builder::new()
            .name("kept-transcripts".into())
            .spawn(move || {
                for job in queue {
                    job(&mut store);
                }
            });

        if let Err(e) = spawned {
            log::warn!("starting the kept transcripts: {e}");
        }

        Self { jobs }
    }

    fn send(&self, job: impl FnOnce(&mut Store) + Send + 'static) {
        if self.jobs.send(Box::new(job)).is_err() {
            log::warn!("the kept transcripts stopped");
        }
    }

    async fn ask<T: Send + 'static>(
        &self,
        job: impl FnOnce(&mut Store) -> T + Send + 'static,
    ) -> Result<T> {
        let (reply, answer) = oneshot::channel();
        self.send(move |store| {
            let _ = reply.send(job(store));
        });
        answer
            .await
            .map_err(|_| DesktopError::Other(anyhow::anyhow!("the kept transcripts stopped")))
    }

    /// The page and cursor kept for `key`, or `None`.
    pub(crate) async fn read(&self, key: SessionKey) -> Result<Option<KeptTranscript>> {
        self.ask(move |store| store.read(&key)).await
    }

    /// Keeps `kept` for `key` in the background, replacing what was kept.
    pub(crate) fn keep(&self, key: SessionKey, kept: KeptTranscript) {
        self.send(move |store| {
            if let Err(e) = store.write(&key, &kept) {
                log::warn!("keeping the transcript: {e}");
            }
        });
    }

    /// Drops every kept session, when this computer starts over; returns once they are gone.
    pub fn clear(&self) {
        let (reply, done) = mpsc::channel();
        self.send(move |store| {
            store.clear();
            let _ = reply.send(());
        });
        let _ = done.recv();
    }
}

/// The files, touched only by the kept transcripts' thread.
struct Store {
    dir: PathBuf,
    /// Deleted sessions, newest last; their pages are not kept again.
    forgotten: VecDeque<PathBuf>,
}

impl Store {
    fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            forgotten: VecDeque::new(),
        }
    }

    fn file(&self, key: &SessionKey) -> PathBuf {
        let name = format!("{}\n{}\n{}", key.machine_id, key.agent, key.session);
        self.dir.join(format!(
            "{}.json",
            utf8_percent_encode(&name, NON_ALPHANUMERIC)
        ))
    }

    fn read(&self, key: &SessionKey) -> Option<KeptTranscript> {
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

    fn write(&self, key: &SessionKey, kept: &KeptTranscript) -> Result<()> {
        let file = self.file(key);

        if self.forgotten.contains(&file) {
            return Ok(());
        }

        let bytes = serde_json::to_vec(kept).map_err(anyhow::Error::from)?;

        if bytes.len() > MAX_ONE {
            return Err(DesktopError::BadRequest(format!(
                "the page is {} bytes, over the {MAX_ONE} kept",
                bytes.len()
            )));
        }

        let mut partial = file.as_os_str().to_owned();
        partial.push(".part");
        fs::create_dir_all(&self.dir)
            .and_then(|()| fs::write(&partial, &bytes))
            .and_then(|()| fs::rename(&partial, &file))
            .map_err(|e| anyhow::anyhow!("writing {}: {e}", file.display()))?;
        trim(&self.dir, MAX_SESSIONS, MAX_BYTES);
        Ok(())
    }

    fn forget(&mut self, key: &SessionKey) {
        let file = self.file(key);
        remove(&file);

        if self.forgotten.len() == FORGOTTEN {
            self.forgotten.pop_front();
        }

        self.forgotten.push_back(file);
    }

    fn clear(&self) {
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

/// The page and cursor kept for a session, or `None`.
#[tauri::command]
pub async fn transcript_kept(
    kept: State<'_, Arc<KeptTranscripts>>,
    machine_id: String,
    agent: String,
    session: String,
) -> Result<Option<KeptTranscript>> {
    kept.read(SessionKey {
        machine_id,
        agent,
        session,
    })
    .await
}

/// Drops what was kept of a session, when it is deleted.
#[tauri::command]
pub async fn transcript_forget(
    kept: State<'_, Arc<KeptTranscripts>>,
    machine_id: String,
    agent: String,
    session: String,
) -> Result<()> {
    let key = SessionKey {
        machine_id,
        agent,
        session,
    };
    kept.ask(move |store| store.forget(&key)).await
}

#[cfg(test)]
#[path = "../tests/agents_kept_transcripts.rs"]
mod tests;
