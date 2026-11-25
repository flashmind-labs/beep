//! Watching the CI runs of the user's pinned projects, with no server
//! (#338). One machine per project watches: among the project's machines that
//! are online and have its checkout, the one that reaches the project's host
//! first, then the smallest endpoint id (`machine::elect`). It lists the
//! project's runs every minute through `gh` or `teacli`, keeps the ones the signed-in user
//! started by a push or a pull request, waits on each going one with the
//! CLI's watch command, and tells the desktops once each ends (`book`), over
//! their `FollowRuns` streams. Between lists, a GitHub project is checked
//! cheaply over HTTPS (`check`). Entry point: `watch`, run by the node while
//! it is in a mesh.

mod book;
mod check;
pub(crate) mod forge;
mod github;
pub(crate) mod projects;
mod watcher;

#[cfg(test)]
#[path = "../../tests/machine_ci.rs"]
mod tests;

use crate::agents::ci::Ci;

pub(crate) use watcher::watch;
