//! Holding a checkout for Git recovery when nothing works in it.

use super::{MAX_IN_FOLDERS, Occupancy, RepositoryHold};
use crate::logs::found::{self, Agent};
use anyhow::{Result, ensure};
use beep_proto::{SessionId, node::agent::{ExternalSession, Presence}};
use std::{collections::{HashMap, HashSet}, path::Path, time::SystemTime};
use tokio::task::spawn_blocking;

/// Whether `sessions`, all there are in a folder, leave it idle: a folder
/// with as many as the read counts cannot be known idle, and a presence
/// that is not known to be stopped is not evidence of an idle checkout.
fn idle(sessions: &[ExternalSession]) -> bool {
    sessions.len() < MAX_IN_FOLDERS
        && sessions
            .iter()
            .all(|session| session.presence == Presence::Stopped)
}

impl Occupancy {
    /// Holds the checkout `folder` for recovery when no session works in
    /// it, judged now: the sessions this node runs, and the sessions
    /// started outside Beep as the stores and the processes show them.
    /// Refused while another hold has it, while a session of this node runs
    /// in it, and when a session outside Beep runs there or cannot be
    /// told from one that does.
    pub async fn reserve(&self, folder: &Path) -> Result<RepositoryHold> {
        let hold = self.holds().hold(folder).await?;
        let folder = hold.folder().to_owned();
        let running = {
            let (holds, folder) = (self.holds().clone(), folder.clone());
            spawn_blocking(move || holds.running_under(&folder)).await?
        };
        ensure!(
            !running,
            "Local edits block Git sync; close running sessions before automatic recovery."
        );
        // A presence that cannot be established is not evidence of an idle checkout.
        let sessions = self.sessions_in(&folder).await;
        ensure!(
            sessions.is_ok_and(|sessions| idle(&sessions)),
            "Local edits block Git sync; an external session is running or its presence is unknown."
        );

        Ok(hold)
    }

    /// The sessions started outside Beep that ran in `folder` or below, as
    /// they stand now, with no titles or prompts read.
    async fn sessions_in(&self, folder: &Path) -> Result<Vec<ExternalSession>> {
        let seen = self.sighted().await?;
        let seen = self.with_present(seen.seen.as_ref().clone());
        let picked = found::in_folders(seen, std::slice::from_ref(&folder.to_owned()), MAX_IN_FOLDERS);
        let (claude, others): (Vec<_>, Vec<_>) = picked
            .into_iter()
            .partition(|seen| seen.agent == Agent::Claude);
        let ids: HashSet<SessionId> = claude.into_iter().map(|seen| seen.id).collect();
        let stores = self.stores().clone();
        let described = spawn_blocking(move || {
            let now = SystemTime::now();
            others
                .into_iter()
                .map(|seen| (seen.agent, stores.describe(seen, now)))
                .collect::<Vec<_>>()
        });
        let (claude, others) = tokio::join!(self.claude().named(ids, HashMap::new()), described);
        let mut sessions = claude?;

        for (agent, mut found) in others? {
            found.presence = self.presence_of(agent, &found).await;
            sessions.push(found);
        }

        self.presence().merge(&mut sessions);
        self.settle_closed(&mut sessions);
        sessions.retain(|session| found::within(Path::new(&session.cwd), folder));

        Ok(sessions)
    }
}

#[cfg(test)]
#[path = "../tests/occupancy_reserve.rs"]
mod tests;
