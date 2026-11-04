//! Each project's open PRs, issues and workflow runs, cached per user and project, refreshed in
//! the background while viewed and on Refresh at most twice a minute; running jobs every 10 s.

mod actions;
mod forward;
mod hosts;
mod http;
mod lists;
mod live;
mod runs;
#[cfg(test)]
mod test_host;
mod timing;
mod watch;

pub(crate) use forward::{LiveState, sweep, sweep_any};
pub(crate) use lists::ListsCopy;
pub(crate) use watch::Changed;
pub use watch::RepoWatch;

use crate::app::App;
use std::sync::Arc;

/// Starts the background sync. It holds the app weakly and ends with it.
pub fn start(app: &Arc<App>) {
    tokio::spawn(watch::run(Arc::downgrade(app)));
}
