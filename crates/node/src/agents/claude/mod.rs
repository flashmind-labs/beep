//! Claude Code sessions started outside Beep, in a terminal or an IDE
//! (`External`): what their hooks report, and taking one over through the
//! ACP adapter.

use crate::logs::claude::Store;
use anyhow::{Result, bail};
use beep_proto::SessionId;
use beep_proto::node::{
    LinkRx, LinkTx,
    agent::{ExternalSession, LiveStatus, Presence, Steer},
};
use std::{collections::HashMap, path::Path};

pub mod hooks;
mod live;
mod watch;

/// Claude Code's sessions on this machine, and the hook events about them.
#[derive(Default)]
pub struct External {
    store: Store,
    hub: watch::Hub,
    live: live::Live,
}

impl std::ops::Deref for External {
    type Target = Store;

    fn deref(&self) -> &Store {
        &self.store
    }
}

impl External {
    /// Looks for Claude Code's folder; without one there is nothing to list.
    pub fn locate() -> Self {
        Self {
            store: Store::locate(),
            ..Self::default()
        }
    }

    #[cfg(test)]
    pub(crate) fn locate_in(root: &Path) -> Self {
        Self {
            store: Store::locate_in(root),
            ..Self::default()
        }
    }

    #[cfg(test)]
    pub(crate) fn at(root: &Path) -> Self {
        Self {
            store: Store::at(root),
            ..Self::default()
        }
    }

    /// Takes hook events on the socket in `dir` until the node stops.
    pub async fn listen_hooks(&self, dir: &Path) -> Result<()> {
        watch::listen(&hooks::socket_path(dir), &self.hub, &self.live).await
    }

    /// How session `id` does now as its hooks report it, after queuing
    /// `steer` for its next hook when given. `owned` is as for `list`.
    pub async fn live(
        &self,
        id: &SessionId,
        steer: Option<Steer>,
        owned: HashMap<SessionId, SessionId>,
    ) -> Result<LiveStatus> {
        let found = match self.list(1, Some(id.clone()), owned).await?.pop() {
            Some(found) => found,
            None if self.live.knows(id) => ExternalSession {
                id: id.clone(),
                agent: "claude".to_owned(),
                ..ExternalSession::default()
            },
            None => bail!("there is no Claude Code session {id} here"),
        };

        if let Some(taken) = &found.taken_over {
            bail!("session {id} runs here as session {taken}, so steer it as that session");
        }

        if let Some(steer) = steer {
            self.live.steer(id, steer).map_err(anyhow::Error::msg)?;
        }

        Ok(self.live.status(found))
    }

    /// Serves one `Watch` stream.
    pub(crate) async fn watch(
        &self,
        tx: LinkTx,
        rx: LinkRx,
        sessions: Vec<SessionId>,
        rings: tokio::sync::broadcast::Receiver<crate::events::Change>,
    ) -> Result<()> {
        watch::serve(tx, rx, self.store.store(), &self.hub, sessions, rings).await
    }
}

/// Why `found` cannot be taken over, when it cannot.
pub(crate) fn refuse_takeover(found: &ExternalSession) -> Option<String> {
    if found.presence.is_live() {
        let process = found
            .pid
            .map(|pid| format!(" (process {pid})"))
            .unwrap_or_default();
        let how = match found.presence {
            Presence::Recent => "was active a moment ago".to_owned(),
            presence => format!("is {presence}"),
        };
        return Some(format!(
            "session {} {how} in a terminal or IDE{process}, so Beep can only follow it. \
             Once the user exits it there, it can be taken over.",
            found.id
        ));
    }

    if found.cwd.is_empty() {
        return Some(format!(
            "session {} does not record the folder it ran in",
            found.id
        ));
    }

    None
}

#[cfg(test)]
#[path = "../../tests/agents_claude.rs"]
mod tests;
