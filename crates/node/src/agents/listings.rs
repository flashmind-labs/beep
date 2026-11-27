//! What the node has read of the folders it lists sessions in.

use crate::logs::found;
use crate::AsyncMutex;
use beep_proto::node::agent::SessionFolder;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use tokio::time::Instant;

#[derive(Default)]
pub(crate) struct Listings {
    /// The checkouts found on disk under home, once `warm` read them.
    /// Written after a walk, by the one walk running.
    pub(super) on_disk: Mutex<Option<Arc<Vec<found::Checkout>>>>,
    /// Held while a walk for checkouts runs, deep or quick; one at a time.
    /// `warm` waits for it, a rescan does not.
    pub(super) rescanning: AsyncMutex<()>,
    /// The last folder listing for each limit; held while one runs, so asks
    /// for that limit that come meanwhile share it instead of each running
    /// git for every folder.
    pub(super) listed: Mutex<HashMap<usize, Arc<AsyncMutex<Option<Listed>>>>>,
}

/// The last folder listing, for the asks that waited while it ran.
pub(super) struct Listed {
    /// When it ended.
    pub(super) at: Instant,
    pub(super) folders: Arc<Vec<SessionFolder>>,
}
