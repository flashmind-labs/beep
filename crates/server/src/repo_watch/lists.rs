//! The copy of a project's open pull requests and issues, read on a
//! machine of the project with its host's CLI.

use super::watch::{Entry, OpenHead, RepoWatch};
use crate::{
    app::App,
    projects::{
        fleet::NodeFleet,
        issues::{self, HostIssues, IssueError, ListedPr},
    },
    store::ProjectRow,
    user::User,
};
use std::{
    sync::Arc,
    time::{Instant, SystemTime},
};

/// The last lists read, and why the last read of each failed.
#[derive(Debug, Clone, Default)]
pub(crate) struct ListsCopy {
    pub(crate) prs: Vec<ListedPr>,
    pub(crate) prs_error: Option<String>,
    /// Every open issue, tracked or not; `None` without an issues host.
    pub(crate) issues: Option<HostIssues>,
    pub(crate) issues_error: Option<String>,
    /// When a read last succeeded.
    pub(crate) synced_at: Option<SystemTime>,
}

impl RepoWatch {
    /// `project`'s lists from the copy, read first when there is none or it
    /// is a few minutes old, or when `refresh` may read now.
    pub(crate) async fn lists(
        &self,
        app: &Arc<App>,
        user: &Arc<User>,
        project: &ProjectRow,
        refresh: bool,
    ) -> ListsCopy {
        let entry = self.entry(user.id(), project.id);
        let ask = entry.lists.ask(Instant::now(), refresh);
        let mut copy = entry.lists.copy.lock().await;
        if ask.forced || entry.lists.stale() {
            read(app, user, project, &entry, &mut copy).await;
        }
        copy.clone()
    }
}

/// What the user reads of why a list failed. A store error is logged
/// rather than shown.
fn message(error: IssueError) -> String {
    match error {
        IssueError::Store(e) => {
            log::warn!("repo watch: a list did not read: {e:#}");
            "Beep could not read this project.".to_owned()
        }
        other => other.to_string(),
    }
}

/// Reads both lists into `copy`. A list that fails keeps the one read
/// before, with its error.
async fn read(
    app: &Arc<App>,
    user: &Arc<User>,
    project: &ProjectRow,
    entry: &Entry,
    copy: &mut ListsCopy,
) {
    let fleet = NodeFleet {
        app: app.clone(),
        user: user.clone(),
    };
    let (prs, open) = tokio::join!(
        issues::list_open_prs(&fleet, user, project),
        issues::read_open(&fleet, user, project),
    );
    let mut any = false;
    match prs {
        Ok(prs) => {
            any = true;
            copy.prs = prs;
            copy.prs_error = None;
        }
        Err(e) => copy.prs_error = Some(message(e)),
    }
    match open {
        Ok(open) => {
            any = true;
            copy.issues = open;
            copy.issues_error = None;
        }
        Err(e) => copy.issues_error = Some(message(e)),
    }
    if any {
        copy.synced_at = Some(SystemTime::now());
    }
    entry.set_heads(
        copy.prs
            .iter()
            .map(|listed| OpenHead {
                number: listed.pr.number,
                branch: listed.pr.head.clone(),
            })
            .collect(),
    );
    let ok = copy.prs_error.is_none() && copy.issues_error.is_none();
    entry.lists.timing().synced(Instant::now(), ok);
}

/// The background sync's read, unless an ask read it meanwhile.
pub(super) async fn sync_in_background(
    app: &Arc<App>,
    user: &Arc<User>,
    project: &ProjectRow,
    entry: &Entry,
) {
    let mut copy = entry.lists.copy.lock().await;
    if entry.lists.stale() {
        read(app, user, project, entry, &mut copy).await;
    }
}
