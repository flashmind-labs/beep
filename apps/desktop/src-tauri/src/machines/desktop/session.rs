//! `desktop/1` sessions: Beep sees and uses this Mac's screen through the
//! node link. Each request runs on a blocking thread, one at a time.

use super::mac;
use crate::store::lock;

use anyhow::Result;
use beep_node::link::{Protocol, Session};
use beep_proto::node::{
    LinkRx, LinkTx,
    desktop::{PROTOCOL, Reply, Request},
};
use std::sync::{Arc, Mutex};
use tokio::task::spawn_blocking;

/// Serves `desktop/1` for the node link.
pub struct Desktop;

impl Protocol for Desktop {
    fn name(&self) -> &str {
        PROTOCOL
    }

    fn serve(&self, tx: LinkTx, rx: LinkRx) -> Session {
        Box::pin(serve(tx, rx))
    }
}

async fn serve(mut tx: LinkTx, mut rx: LinkRx) -> Result<()> {
    let session = Arc::new(Mutex::new(mac::Session::default()));

    while let Some(request) = rx.recv::<Request>().await? {
        let session = session.clone();
        let reply = spawn_blocking(move || lock(&session).handle(request))
            .await
            .unwrap_or_else(|e| Reply::Failed {
                reason: format!("The request failed on this computer: {e}"),
                missing: None,
            });
        tx.send(&reply).await?;
    }

    Ok(())
}

#[cfg(test)]
#[path = "../../tests/machines_desktop_session.rs"]
mod tests;
