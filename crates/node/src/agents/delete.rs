//! Deleting an ended session: the agent's own files and the node's record.

use beep_proto::SessionId;
use super::{Agents};
use crate::logs::found::Agent;
use crate::text::valid_id;
use anyhow::{Context, Result, bail, ensure};
use beep_proto::node::agent::{SessionInfo, Status};
use beep_proto::watched;
use std::path::PathBuf;
use tokio::task::spawn_blocking;

impl Agents {
    /// Deletes session `id` of the harness `agent`, the agent's own id or
    /// this node's. It refuses one that runs here, ends the process that has it open,
    /// and the harnesses that keep sessions in a database. The agent's
    /// files go first, then the node's record of it, so a failure leaves
    /// the session listed. What it spent stays in the spend totals.
    pub(super) async fn delete_session(&self, agent: &str, id: &SessionId) -> Result<()> {
        let store = self.sessions.store_of(id);
        let _store = store.lock().await;
        let which = agent
            .parse::<Agent>()
            .ok()
            .with_context(|| format!("this node has no agent {agent}"))?;
        if matches!(which, Agent::Flsh | Agent::OpenCode | Agent::Agy) {
            bail!("{agent} keeps its sessions in a database, which Beep does not edit");
        }

        ensure!(valid_id(id.as_ref()), "{id:?} is not a session id");
        let conversation = self.conversation_of(id);
        let agent_id = match self.get(id) {
            Some(ours) => {
                ensure!(ours.agent == agent, "session {id} runs {}", ours.agent);
                watched::read(&ours.live, |live| live.agent_session.clone())
            }
            None => Some(id.clone()),
        };

        if let Some(agent_id) = &agent_id {
            ensure!(valid_id(agent_id.as_ref()), "{agent_id:?} is not a session id");
        }

        let records: Vec<(SessionId, bool)> = self
            .session_map()
            .iter()
            .filter(|(key, session)| {
                session.agent == agent
                    && (*key == id
                        || agent_id.is_some()
                            && watched::read(&session.live, |live| live.agent_session == agent_id))
            })
            .map(|(key, session)| {
                let ended = watched::read(&session.live, |live| live.status() == Status::Ended);
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
        // A read of the stores that began before the delete still lists it.
        self.occupancy.forget_reading();
        let attachment_agent = agent.to_owned();
        let attachment_id = agent_id.unwrap_or_else(|| id.clone());
        let indexed = attachment_id.clone();
        self.forget_saved(&indexed);
        spawn_blocking(move || super::uploads::remove(&attachment_agent, &attachment_id)).await??;

        self.sessions
            .remove(&records.iter().map(|(key, _)| key.clone()).collect::<Vec<_>>());

        if let Some(index) = self.search.as_ref() {
            for (key, _) in records.into_iter().filter(|(key, _)| *key != indexed) {
                index.forget(key).await;
            }
            index.forget(indexed).await;
        }

        if let (Some(folder), Some(home)) = (conversation, dirs::home_dir()) {
            let removed =
                spawn_blocking(move || super::conversation::remove(&home, &folder)).await;

            if let Ok(Err(e)) = removed {
                log::warn!("the deleted conversation's folder stays: {e}");
            }
        }

        Ok(())
    }

    /// The folder session `id` takes with it when it is deleted: its own,
    /// when it is the main chat of an assistant conversation, as it is here
    /// or as it was kept from before the node restarted. `delete_session`
    /// removes it only if it is a conversation's folder.
    ///
    /// A chat started in another's folder shares it, so neither takes it
    /// while the other is still known here: the last of them does.
    pub(super) fn conversation_of(&self, id: &SessionId) -> Option<PathBuf> {
        let main = |info: &SessionInfo| info.assistant && info.parent.is_none();
        let mine = self.saved(id).map(|saved| saved.info).filter(main)?;
        let shared = self.saved_sessions().iter().any(|other| {
            other.info.id != mine.id && main(&other.info) && other.info.cwd == mine.cwd
        });

        (!shared).then(|| PathBuf::from(mine.cwd))
    }

    /// Checks that no process has `agent`'s files of its session `id` open.
    /// `ours` says this node ran the session and it ended, so a recent write
    /// is not a sign of life.
    pub(super) async fn check_closed_files(
        &self,
        agent: Agent,
        id: &SessionId,
        ours: bool,
    ) -> Result<()> {
        let mut owned = self.sessions.owned();
        owned.remove(id);
        let mut found = self.find_external(agent, id, owned).await?;

        if let Some(found) = &mut found {
            // Deleting cannot be undone: for Claude Code a recent write still
            // counts, in case a process Beep cannot see is writing it.
            if agent != Agent::Claude {
                found.presence = self.occupancy.presence_of(agent, found).await;
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

    /// Ends the process that has the session's files open, so the delete
    /// that follows finds them closed. Nothing happens for a session no
    /// process is known to hold.
    async fn end_holder(&self, agent: Agent, id: &SessionId) -> Result<()> {
        let mut owned = self.sessions.owned();
        owned.remove(id);

        if let Some(pid) = self
            .find_external(agent, id, owned)
            .await?
            .and_then(|found| found.pid)
        {
            end_process(pid).await?;
        }

        Ok(())
    }

    async fn delete_files(&self, agent: Agent, id: &SessionId, ours: bool) -> Result<bool> {
        self.end_holder(agent, id).await?;
        self.check_closed_files(agent, id, ours).await?;

        match agent {
            Agent::Claude => self.external().delete(id).await,
            other => {
                let stores = self.stores().clone();
                let id = id.clone();
                spawn_blocking(move || stores.delete(other, &id)).await?
            }
        }
    }
}

/// Asks process `pid` to exit and waits for it; kills it when it does not.
#[cfg(unix)]
async fn end_process(pid: u32) -> Result<()> {
    use crate::events::exits::Exit;
    use rustix::process::{Pid, Signal, kill_process};
    use std::time::Duration;
    use tokio::time::timeout;

    /// How long a process gets to exit after being asked to.
    const EXIT_WAIT: Duration = Duration::from_secs(5);

    let target = i32::try_from(pid)
        .ok()
        .and_then(Pid::from_raw)
        .with_context(|| format!("{pid} is not a process id"))?;
    // Watched before the signal, so its exit cannot come first.
    let exit = Exit::watch(pid)?;

    if kill_process(target, Signal::TERM).is_err() {
        return Ok(());
    }

    if timeout(EXIT_WAIT, exit.wait()).await.is_err() {
        let _ = kill_process(target, Signal::KILL);
        let _ = timeout(EXIT_WAIT, exit.wait()).await;
    }

    Ok(())
}

/// Ends process `pid` and its children with `taskkill`.
#[cfg(windows)]
async fn end_process(pid: u32) -> Result<()> {
    let status = tokio::process::Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .status()
        .await
        .context("could not run taskkill")?;
    ensure!(status.success(), "could not end process {pid}");

    Ok(())
}

#[cfg(all(test, unix))]
#[path = "../tests/agents_delete.rs"]
mod tests;
