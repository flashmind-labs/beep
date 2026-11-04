//! The copy of a project's open pull requests and issues, read with its host's CLI on a project machine.

use super::{
    timing::AskRead,
    watch::{Entry, OpenHead, RepoWatch},
};
use crate::{
    account::user::User,
    app::App,
    projects::{fleet::NodeFleet, stored::StoredProject},
    store::ProjectRow,
};
use beep_projects::issues::{HostIssues, ListedPr, SourceError, list_open_prs, read_open};
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
    /// `project`'s lists from the copy, read first when missing or on `refresh`; an old copy syncs in the background.
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
        match entry.lists.ask_read(ask.forced, copy.synced_at.is_some()) {
            AskRead::Host => read(app, user, project, &entry, &mut copy).await,
            AskRead::CopyThenSync => self.sync_soon(),
            AskRead::Copy => {}
        }
        copy.clone()
    }
}

/// What the user reads of why a list failed; store errors are logged instead.
fn message(error: SourceError) -> String {
    match error {
        SourceError::Store(e) => {
            log::warn!("repo watch: a list did not read: {e:#}");
            "Beep could not read this project.".to_owned()
        }
        other => other.to_string(),
    }
}

/// Reads both lists into `copy`; a failing list keeps its previous value with the error.
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
    let store = StoredProject::new(user, project);
    let (prs, open) = tokio::join!(list_open_prs(&store, &fleet), read_open(&store, &fleet),);
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

/// The background sync's read unless an ask read it meanwhile; whether it read.
pub(super) async fn sync_in_background(
    app: &Arc<App>,
    user: &Arc<User>,
    project: &ProjectRow,
    entry: &Entry,
) -> bool {
    let mut copy = entry.lists.copy.lock().await;
    if !entry.lists.stale() {
        return false;
    }
    read(app, user, project, entry, &mut copy).await;
    true
}
