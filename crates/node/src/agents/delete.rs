//! Deleting an ended session: the agent's own files and the node's record.

use super::{Agents, found::Agent};
use anyhow::{Context, Result, bail, ensure};
use beep_proto::node::agent::Status;
use tokio::task::spawn_blocking;

/// The longest session id accepted.
const MAX_ID: usize = 128;

/// An id that cannot name anything outside a store: letters, digits, `-`
/// and `_`, so no separator, no dot and no `..`.
fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_ID
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

impl Agents {
    /// Deletes session `id` of the harness `agent`, the agent's own id or
    /// this node's. It refuses one that runs or that a process has open,
    /// and the harnesses that keep sessions in a database. The agent's
    /// files go first, then the node's record of it, so a failure leaves
    /// the session listed. What it spent stays in the spend totals.
    pub(super) async fn delete_session(&self, agent: &str, id: &str) -> Result<()> {
        let _store = self.session_store.lock().await;
        let which = agent
            .parse::<Agent>()
            .ok()
            .with_context(|| format!("this node has no agent {agent}"))?;
        if matches!(which, Agent::Flsh | Agent::OpenCode) {
            bail!("{agent} keeps its sessions in a database, which Beep does not edit");
        }

        ensure!(valid_id(id), "{id:?} is not a session id");
        let agent_id = match self.get(id) {
            Some(ours) => {
                ensure!(ours.agent == agent, "session {id} runs {}", ours.agent);
                ours.live.borrow().agent_session.clone()
            }
            None => Some(id.to_owned()),
        };

        if let Some(agent_id) = &agent_id {
            ensure!(valid_id(agent_id), "{agent_id:?} is not a session id");
        }

        let records: Vec<(String, bool)> = self
            .lock()
            .iter()
            .filter(|(key, session)| {
                session.agent == agent
                    && (key.as_str() == id
                        || agent_id.is_some() && session.live.borrow().agent_session == agent_id)
            })
            .map(|(key, session)| {
                let ended = session.live.borrow().status() == Status::Ended;
                (key.clone(), ended)
            })
            .collect();
        ensure!(
            records.iter().all(|(_, ended)| *ended),
            "session {id} still runs here; close it first"
        );
        let removed = match &agent_id {
            Some(agent_id) => {
                self.delete_files(which, agent_id, !records.is_empty())
                    .await?
            }
            None => false,
        };
        ensure!(
            removed || !records.is_empty(),
            "there is no {agent} session {id} on this machine"
        );
        let attachment_agent = agent.to_owned();
        let attachment_id = agent_id.unwrap_or_else(|| id.to_owned());
        spawn_blocking(move || super::uploads::remove(&attachment_agent, &attachment_id)).await??;
        let mut sessions = self.lock();

        for (key, _) in records {
            sessions.remove(&key);
        }

        Ok(())
    }

    /// Deletes `agent`'s files of its session `id` unless a process has
    /// them open; `false` when it has none. `ours` says this node ran the
    /// session and it ended, so a recent write is not a sign of life.
    pub(super) async fn check_closed_files(
        &self,
        agent: Agent,
        id: &str,
        ours: bool,
    ) -> Result<()> {
        let mut found = match agent {
            Agent::Claude => {
                let mut owned = self.owned();
                owned.remove(id);
                self.external
                    .list(1, Some(id.to_owned()), owned)
                    .await?
                    .pop()
            }
            other => {
                let stores = self.stores.clone();
                let find = id.to_owned();
                spawn_blocking(move || stores.find(other, &find)).await?
            }
        };

        if let Some(found) = &mut found {
            // Deleting cannot be undone: for Claude Code a recent write still
            // counts, in case a process Beep cannot see is writing it.
            if agent != Agent::Claude {
                found.presence = self.presence(agent, found).await;
            }

            let live = found.pid.is_some() || (found.presence.is_live() && !ours);

            if live {
                bail!(
                    "session {id} is open in a terminal or IDE (it is {}). \
                     Exit it there, then delete it.",
                    found.presence
                );
            }
        }

        Ok(())
    }

    async fn delete_files(&self, agent: Agent, id: &str, ours: bool) -> Result<bool> {
        self.check_closed_files(agent, id, ours).await?;

        match agent {
            Agent::Claude => self.external.delete(id).await,
            other => {
                let stores = self.stores.clone();
                let id = id.to_owned();
                spawn_blocking(move || stores.delete(other, &id)).await?
            }
        }
    }
}

#[cfg(all(test, unix))]
#[path = "../tests/agents_delete.rs"]
mod tests;
