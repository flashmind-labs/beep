//! What the node has read of the folders it lists sessions in.

use super::{images::Images, reopen::Reopen, reviews::kept::Reviews, sessions::Sessions};
use crate::AsyncMutex;
use crate::events::Hub;
use crate::logs::found;
use crate::occupancy::Occupancy;
use beep_proto::node::agent::SessionFolder;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use tokio::time::Instant;

pub struct Listings {
    /// The sessions this node runs, which a listing marks as taken over.
    pub(super) sessions: Arc<Sessions>,
    /// Where the agents keep their sessions, and who holds them open.
    pub(super) occupancy: Occupancy,
    /// The pictures sessions' transcripts show, by session.
    pub(super) images: Arc<Images>,
    pub(super) events: Hub,
    /// The reviewers running, which a listing leaves out.
    pub(super) reviews: Arc<Reviews>,
    /// What the node kept of sessions that ended, which a listing shows.
    pub(super) reopen: Arc<Reopen>,
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

impl Listings {
    pub(crate) fn new(
        sessions: Arc<Sessions>,
        occupancy: Occupancy,
        images: Arc<Images>,
        events: Hub,
        reviews: Arc<Reviews>,
        reopen: Arc<Reopen>,
    ) -> Self {
        Self {
            sessions,
            occupancy,
            images,
            events,
            reviews,
            reopen,
            on_disk: Mutex::default(),
            rescanning: AsyncMutex::default(),
            listed: Mutex::default(),
        }
    }
}

/// The last folder listing, for the asks that waited while it ran.
pub(super) struct Listed {
    /// When it ended.
    pub(super) at: Instant,
    pub(super) folders: Arc<Vec<SessionFolder>>,
}
