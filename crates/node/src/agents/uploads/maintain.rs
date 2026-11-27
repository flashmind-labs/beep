//! The hourly cleanup of old attachments.

use super::{WRITING, folder, owner, sweep};
use crate::{agents::{sessions::Map, work_folder::{self, prune}}, lock};
use anyhow::Result;
use beep_proto::{node::agent::Status, watched};
use std::{collections::HashSet, path::PathBuf, sync::Arc, time::{Duration, SystemTime}};
use tokio::sync::watch;
use tokio_util::task::AbortOnDropHandle;

const SWEEP_EVERY: Duration = Duration::from_secs(60 * 60);

/// Cleanup runs hourly from the first hour on, so sessions are back after a
/// restart, and stops with the node.
pub(crate) fn maintain(sessions: watch::Receiver<Arc<Map>>) -> AbortOnDropHandle<()> {
    AbortOnDropHandle::new(tokio::spawn(async move {
        let start = tokio::time::Instant::now() + SWEEP_EVERY;
        let mut every = tokio::time::interval_at(start, SWEEP_EVERY);
        every.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            every.tick().await;
            let (running, folders) = watched::read(&sessions, |map| of(map));
            let done = tokio::task::spawn_blocking(move || -> Result<()> {
                let now = SystemTime::now();

                for folder in folders {
                    prune(&work_folder::uploads(&folder), now)?;
                }

                let _writing = lock(&WRITING);

                if let Some(cache) = folder() {
                    sweep(&cache, now, &running)?;
                }

                Ok(())
            })
            .await;

            if let Err(e) = done.unwrap_or_else(|e| Err(e.into())) {
                log::warn!("could not clean temporary attachments: {e:#}");
            }
        }
    }))
}

/// The attachment owners of the sessions still running, and the folders
/// every known session works in.
pub(super) fn of(map: &Map) -> (HashSet<String>, HashSet<PathBuf>) {
    let mut running = HashSet::new();
    let mut folders = HashSet::new();

    for session in map.values() {
        watched::read(&session.live, |live| {
            if live.status() != Status::Ended {
                running.insert(owner(&live.info.agent, &live.info.id));
            }
            folders.insert(PathBuf::from(&live.info.cwd));
        });
    }

    (running, folders)
}
