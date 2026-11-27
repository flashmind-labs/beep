//! Where a transfer's progress goes and how often.

use anyhow::Result;
use beep_proto::node::{
    LinkTx,
    transfer::{Reply, Stage},
};
use std::time::Duration;
use tokio::time::Instant;

/// How often progress goes out while bytes move.
const PROGRESS_EVERY: Duration = Duration::from_secs(1);

/// Where a transfer's progress goes: the stream of the one who asked, or
/// nowhere when the node moves files for its own agent.
pub(super) enum Out<'a> {
    Stream(&'a mut LinkTx),
    Silent,
}

impl Out<'_> {
    async fn send(&mut self, reply: &Reply) -> Result<()> {
        match self {
            Out::Stream(tx) => tx.send(reply).await,
            Out::Silent => Ok(()),
        }
    }
}

/// Sends progress for one stage at most every `PROGRESS_EVERY`, so the
/// server's idle timeouts only fire on a transfer that stopped moving.
#[derive(Debug)]
pub(super) struct Ticker {
    stage: Stage,
    total: u64,
    done: u64,
    last: Option<Instant>,
}

impl Ticker {
    pub(super) fn new(stage: Stage, total: u64) -> Self {
        Self {
            stage,
            total,
            done: 0,
            last: None,
        }
    }

    pub(super) fn done(&self) -> u64 {
        self.done
    }

    /// Whether a report is due at `now`; the first one always is.
    fn due(&self, now: Instant) -> bool {
        self.last
            .is_none_or(|last| now.saturating_duration_since(last) >= PROGRESS_EVERY)
    }

    /// Records `done` and reports it if one is due.
    pub(super) async fn tick(&mut self, tx: &mut Out<'_>, done: u64) -> Result<()> {
        self.done = done;

        if self.due(Instant::now()) {
            self.send(tx).await?;
        }

        Ok(())
    }

    /// Reports `done` now, as the stage's last report.
    pub(super) async fn finish(&mut self, tx: &mut Out<'_>, done: u64) -> Result<()> {
        self.done = done;
        self.send(tx).await
    }

    async fn send(&mut self, tx: &mut Out<'_>) -> Result<()> {
        self.last = Some(Instant::now());
        tx.send(&Reply::Progress {
            done: self.done.min(self.total),
            total: self.total,
            stage: self.stage,
        })
        .await
    }
}
