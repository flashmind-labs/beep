//! Each project's open pull requests, issues and workflow runs, kept so the
//! project page reads its host seldom: from a copy per user and project,
//! read again in the background every few minutes while someone looks at
//! it, and on Refresh at most twice a minute. A running run's jobs are read
//! every 10 seconds only while the page is open (`GetProjectLists`,
//! `ListProjectRuns`, `GetProjectRun`).

mod actions;
mod hosts;
mod http;
mod lists;
mod live;
mod runs;
#[cfg(test)]
mod test_host;
mod timing;
mod watch;

pub(crate) use lists::ListsCopy;
pub use watch::RepoWatch;

use crate::app::App;
use std::sync::Arc;

/// Starts the background sync. It holds the app weakly and ends with it.
pub fn start(app: &Arc<App>) {
    tokio::spawn(watch::run(Arc::downgrade(app)));
}
