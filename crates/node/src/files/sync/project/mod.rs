//! One synced project on this node: its list, what its folder held at the
//! last scan, and the task (`task::run`) that keeps the two in step.

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
