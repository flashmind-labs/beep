//! `report/1` (see `beep_proto::node::report`): this machine's part of a
//! problem report, its log for a period, status, version and OS.

use crate::{
    link::local_status::{LocalStatus, render},
    machine::{env::os_version, service::logs::Logs},
};
use anyhow::Result;
use beep_proto::node::{
    LinkRx, LinkTx,
    report::{MAX_BYTES, Reply, Request},
};
use std::{
    env::consts::OS,
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::task::spawn_blocking;

/// Answers the one `Request` the stream carries.
pub(crate) async fn serve_report(
    mut tx: LinkTx,
    mut rx: LinkRx,
    status: LocalStatus,
    log: Logs,
) -> Result<()> {
    let Some(Request { since, max_bytes }) = rx.recv().await? else {
        return Ok(());
    };
    let reply = match i64::try_from(since) {
        Ok(since) => {
            let max_bytes = usize::try_from(max_bytes.min(MAX_BYTES))?;
            // A log that cannot be read still leaves the rest worth sending.
            let log = spawn_blocking(move || log.window(since, max_bytes))
                .await?
                .unwrap_or_else(|e| format!("the log could not be read: {e:#}"));
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |now| now.as_secs());
            Reply::Share {
                log,
                status: render(&status, now),
                version: env!("CARGO_PKG_VERSION").to_owned(),
                os: OS.to_owned(),
                os_version: os_version().await,
            }
        }
        Err(_) => Reply::Failed {
            reason: format!("{since} is not a time this machine can read its log from"),
        },
    };
    tx.send(&reply).await?;
    tx.finish().await
}
