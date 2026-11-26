//! What the app keeps on disk for the next launch: the views the window last
//! drew and the windows to reopen. Nothing here depends on a feature.

mod disk;
pub(crate) mod view_cache;
pub(crate) mod window_label;
#[cfg(desktop)]
pub(crate) mod window_session;

pub(crate) use disk::{lock, write_part};
pub(crate) use view_cache::{cache_view, kept};
