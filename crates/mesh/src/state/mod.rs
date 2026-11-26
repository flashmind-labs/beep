//! Replicated last-write-wins state. One log per origin device. On connect
//! each side sends its heads and receives what it lacks. The value of a key
//! is the entry with the highest `(at_ms, origin)`.
//!
//! These calls block on fsync, so callers run them off the async workers.
//!
//! Every entry is signed by its origin over the mesh id, so a node relaying
//! entries cannot forge or alter one. Sequence numbers rise per origin but
//! are sparse: a new entry takes `max(head + 1, now_ms)`, so a desktop that
//! lost its log restarts above its old numbers, and compaction can delete
//! superseded entries. A receiver skips `seq <= head` and moves the head to
//! the entry's `seq`.
//!
//! Kept in `<dir>/mesh2/state.log`, mode 0600. Each record is a length, a
//! CRC32 and a msgpack body. Compaction at open rewrites the file as a
//! `Heads` record plus the newest entry per key. Two replicas' heads may
//! differ after compaction (one may have dropped its newest entry), so
//! callers use `since`, never a comparison of heads, to decide they are in
//! sync. The first bad or short record at open ends the log there.

mod entry;
mod key;
mod log;

pub(crate) use entry::Entry;
pub use key::Key;
pub(crate) use log::{
    Applied, Heads, MESH_DIR, OLD_MESH_DIR, State, private_dir, private_options, sync_dir,
};
