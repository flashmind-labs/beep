//! The protocols a host serves beside the node's own, such as the Mac app's
//! `desktop/1`.

use anyhow::Result;
use beep_proto::node::{LinkRx, LinkTx};
use std::{future::Future, pin::Pin};

/// A session protocol a host serves beside the node's own, such as the Mac
/// app's `desktop/1`.
pub trait Protocol: Send + Sync {
    /// The name a session opens with, which the hello offers.
    fn name(&self) -> &str;
    /// Serves one session stream until it ends.
    fn serve(&self, tx: LinkTx, rx: LinkRx) -> Session;
}

/// One session served by a `Protocol`.
pub type Session = Pin<Box<dyn Future<Output = Result<()>> + Send>>;
