//! What sync hands its transfers.

use crate::files::place::{Journal, Room};
use iroh::Endpoint;
use iroh_blobs::store::fs::FsStore;
use std::{path::PathBuf, sync::Arc, time::Duration};

/// How long a transfer may wait for its next data before it gives up. A
/// connection can stay open on a path that no longer carries data (a NAT
/// mapping gone, with the relay keeping the connection alive); without a
/// limit such a transfer waits forever. The next round tries again over a
/// new connection and keeps what came.
pub(crate) const STALL: Duration = Duration::from_secs(30);

/// What sync shares with transfers.
#[derive(Clone)]
pub(crate) struct Parts {
    pub(crate) endpoint: Endpoint,
    pub(crate) store: FsStore,
    pub(crate) store_dir: PathBuf,
    pub(crate) journal: Journal,
    pub(crate) room: Arc<Room>,
}
