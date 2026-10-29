//! Claude Code sessions the user started on the machine outside Beep, in a
//! terminal or an IDE: finding them, reading their transcripts and
//! following them. Taking one over runs it through the ACP adapter like a
//! session Beep started (see `crate::agents`).

use anyhow::{Context, Result};
use beep_proto::node::{
    LinkRx, LinkTx,
    agent::{Entry, ExternalSession},
};
use std::{collections::HashMap, path::Path};
use tokio::task::spawn_blocking;

mod discover;
pub mod hooks;
mod procs;
mod transcript;
mod watch;

/// The most sessions one listing reads.
const MAX_LISTED: usize = 50;
/// How many a listing reads when the server names no number.
const DEFAULT_LISTED: usize = 20;

/// Claude Code's sessions on this machine, and the hook events about them.
#[derive(Default)]
pub struct External {
    store: Option<discover::Store>,
    hub: watch::Hub,
}

impl External {
    /// Looks for Claude Code's folder; without one there is nothing to list.
    pub fn locate() -> Self {
        Self {
            store: discover::Store::locate(),
            hub: watch::Hub::default(),
        }
    }

    /// The newest `limit` sessions, or only `only`. `owned` maps the
    /// sessions this node runs, by Claude Code's id, to the node's id.
    pub(crate) async fn list(
        &self,
        limit: u32,
        only: Option<String>,
        owned: HashMap<String, String>,
    ) -> Result<Vec<ExternalSession>> {
        let Some(store) = self.store.clone() else {
            return Ok(Vec::new());
        };
        let processes = procs::claude_processes().await;
        let limit = match usize::try_from(limit).unwrap_or(MAX_LISTED) {
            0 => DEFAULT_LISTED,
            limit => limit.min(MAX_LISTED),
        };
        spawn_blocking(move || {
            let snapshot = discover::Snapshot {
                processes,
                pid_files: store.pid_files(),
                owned,
                now: None,
            };
            discover::list(&store, &snapshot, limit, only.as_deref())
        })
        .await?
        .context("reading Claude Code's sessions")
    }

    /// The last `last` entries of session `id`'s transcript and how many
    /// came before them, `None` when there is no such session.
    pub(crate) async fn transcript(
        &self,
        id: &str,
        last: usize,
    ) -> Result<Option<(Vec<Entry>, u64)>> {
        let Some(path) = self.store.as_ref().and_then(|store| store.find(id)) else {
            return Ok(None);
        };
        let read = spawn_blocking(move || transcript::entries(&path, last)).await??;
        Ok(Some(read))
    }

    /// Takes hook events on the socket in `dir` until the node stops.
    pub async fn listen_hooks(&self, dir: &Path) -> Result<()> {
        watch::listen(&hooks::socket_path(dir), &self.hub).await
    }

    /// Serves one `Watch` stream.
    pub(crate) async fn watch(&self, tx: LinkTx, rx: LinkRx, sessions: Vec<String>) -> Result<()> {
        watch::serve(tx, rx, self.store.clone(), &self.hub, sessions).await
    }
}

/// Why `found` cannot be taken over, when it cannot.
pub(crate) fn refuse_takeover(found: &ExternalSession) -> Option<String> {
    if found.presence.is_live() {
        let process = found
            .pid
            .map(|pid| format!(" (process {pid})"))
            .unwrap_or_default();
        return Some(format!(
            "session {} is {} in a terminal or IDE{process}, so Beep can only follow it. \
             Once the user exits it there, it can be taken over.",
            found.id, found.presence
        ));
    }
    if found.cwd.is_empty() || !Path::new(&found.cwd).is_dir() {
        return Some(format!(
            "session {} ran in a folder that is not here any more",
            found.id
        ));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use beep_proto::node::agent::Presence;

    #[test]
    fn only_a_stopped_session_in_a_folder_that_exists_is_taken_over() {
        let mut found = ExternalSession {
            id: "0f3c2a9e-1111-4222-8333-944455556666".into(),
            cwd: "/".into(),
            presence: Presence::Working,
            pid: Some(41),
            ..ExternalSession::default()
        };
        let refused = refuse_takeover(&found).unwrap();
        assert!(refused.contains("can only follow it"), "{refused}");
        assert!(refused.contains("process 41"), "{refused}");
        found.presence = Presence::Recent;
        assert!(refuse_takeover(&found).is_some());
        found.presence = Presence::Stopped;
        assert_eq!(refuse_takeover(&found), None);
        found.cwd = "/no/such/folder/here".into();
        assert!(refuse_takeover(&found).is_some());
    }
}
