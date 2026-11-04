//! One synced project on this node: its list, what its folder held at the
//! last scan, and the task that keeps the two in step.
//!
//! The task scans the folder when the watcher says it changed (or on a
//! timer when it cannot be watched), records the edits it finds in the
//! list, and then applies the list to the folder: fetching files other
//! nodes changed, removing deleted ones, and moving the losing side of a
//! conflict to its copy. Pulls from peers merge into the list from their
//! own tasks and wake this one.

mod apply;
mod fetch;
mod list;
mod scanning;
mod shared;
mod state;
mod task;

pub(super) use shared::{Shared, Start};
pub(super) use state::Cursor;
pub(super) use task::{run, tag_name};

#[cfg(test)]
mod tests;
