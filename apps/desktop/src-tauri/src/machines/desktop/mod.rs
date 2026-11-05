//! `desktop/1` sessions: Beep sees and uses this Mac's screen through the
//! node link. Each request runs on a blocking thread, one at a time.

mod keys;
mod mac;
mod tree;

use anyhow::Result;
use beep_node::link::{Protocol, Session};
use beep_proto::node::{
    LinkRx, LinkTx,
    desktop::{PROTOCOL, Reply, Request},
};
use std::sync::{Arc, Mutex, PoisonError};
use tokio::task::spawn_blocking;

pub use mac::{permissions, request};

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
        let reply = spawn_blocking(move || {
            session
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .handle(request)
        })
        .await
        .unwrap_or_else(|e| Reply::Failed {
            reason: format!("The request failed on this Mac: {e}"),
            missing: None,
        });
        tx.send(&reply).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Takes a real screenshot and tree on this Mac and sends both through
    /// the wire encoding. Needs Screen Recording and Accessibility for the
    /// process running it, so it only runs by hand:
    /// `cargo test -- --ignored live_desktop --nocapture`.
    #[test]
    #[ignore = "needs a desktop session and macOS permissions"]
    fn live_desktop() {
        let mut session = mac::Session::default();
        println!("{:?}", permissions());
        for request in [
            Request::Screenshot { max_side: 1280 },
            Request::Tree { max_elements: 60 },
        ] {
            let reply = session.handle(request);
            let body = rmp_serde::to_vec(&reply).expect("a reply encodes");
            let back: Reply = rmp_serde::from_slice(&body).expect("a reply decodes");
            match &back {
                Reply::Tree(tree) => println!("{} ({} bytes)\n{}", tree.app, body.len(), tree.text),
                other => println!("{other:?} ({} bytes)", body.len()),
            }
            assert_eq!(back, reply);
        }
    }
}
