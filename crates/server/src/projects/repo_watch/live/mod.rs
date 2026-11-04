//! The copy of a project's workflow runs on each host, and live job progress: read at most every
//! 10 seconds while a run is going and the page is open, and not while the rate limit is low.

mod runs_copy;
pub(crate) use runs_copy::RunsCopy;
use runs_copy::{HostCopy, HostSlot, KeptLog};

mod detail;

use super::{
    hosts::{self, Tokens},
    runs::{self, Target, credentials, target},
    timing::AskRead,
    watch::{Entry, RepoWatch},
};
use crate::{
    account::{credentials::host_client, user::User},
    app::App,
    projects::fleet::NodeFleet,
    protocol::{HostedRuns, ProjectRuns},
    store::ProjectRow,
};
use beep_projects::git_items::HostError;
use futures::future::join_all;
use reqwest::Client;
use std::{
    sync::Arc,
    time::{Instant, SystemTime},
};
use tokio::sync::Semaphore;

/// Running runs whose jobs one poll reads, per host.
const MAX_LIVE: usize = 4;
/// Failed jobs' logs kept per project.
const MAX_LOGS: usize = 8;
/// What a project with no host says.
const NO_HOST: &str = "This project has no GitHub or Forgejo host to read runs from.";

/// What one read of the hosts does.
#[derive(Debug, Clone, Copy)]
struct Plan {
    /// Refresh: read every host's list now.
    forced: bool,
    /// The copy is old: read the lists of hosts whose rate limit allows.
    stale: bool,
    /// Read the running runs' jobs.
    poll: bool,
}

impl RepoWatch {
    /// `project`'s runs on its primary host, for `ListProjectRuns`.
    pub(crate) async fn runs(
        &self,
        app: &Arc<App>,
        user: &Arc<User>,
        project: &ProjectRow,
        refresh: bool,
    ) -> ProjectRuns {
        let entry = self.entry(user.id(), project.id);
        let copy = self.read_runs(app, user, project, &entry, refresh).await;
        copy.primary_view(&self.calls, &entry.heads())
    }

    /// `project`'s runs on every host, for `ListProjectRunsAll`.
    pub(crate) async fn runs_everywhere(
        &self,
        app: &Arc<App>,
        user: &Arc<User>,
        project: &ProjectRow,
        refresh: bool,
    ) -> HostedRuns {
        let entry = self.entry(user.id(), project.id);
        let copy = self.read_runs(app, user, project, &entry, refresh).await;
        copy.hosted_view(&self.calls, &entry.heads())
    }

    /// The copy with its lists reread like `lists` does, and running jobs while the page asks.
    async fn read_runs<'a>(
        &self,
        app: &Arc<App>,
        user: &Arc<User>,
        project: &ProjectRow,
        entry: &'a Entry,
        refresh: bool,
    ) -> tokio::sync::MutexGuard<'a, RunsCopy> {
        let ask = entry.runs.ask(Instant::now(), refresh);
        let mut copy = entry.runs.copy.lock().await;
        // An old copy answers as it is; the background sync reads it again.
        let stale = match entry.runs.ask_read(ask.forced, copy.read_once()) {
            AskRead::Host => true,
            AskRead::CopyThenSync => {
                self.sync_soon();
                false
            }
            AskRead::Copy => false,
        };
        let poll = copy.any_going(&self.calls) && entry.runs.timing().poll(Instant::now());
        if ask.forced || stale || poll {
            let plan = Plan {
                forced: ask.forced,
                stale,
                poll,
            };
            self.sync(app, user, project, entry, &mut copy, plan).await;
        }
        copy
    }

    /// Finds the project's hosts and reads each as `plan` says.
    async fn sync(
        &self,
        app: &Arc<App>,
        user: &Arc<User>,
        project: &ProjectRow,
        entry: &Entry,
        copy: &mut RunsCopy,
        plan: Plan,
    ) {
        let ok = match self.find_hosts(app, user, project, entry, copy).await {
            Err(why) => {
                copy.error = Some(why);
                false
            }
            Ok(found) => {
                copy.error = None;
                self.read_hosts(
                    app.git_items.calls(),
                    &found.client,
                    copy,
                    found.targets,
                    plan,
                )
                .await
            }
        };
        entry.runs.timing().synced(Instant::now(), ok);
    }

    /// The project's hosts into `copy` with a target each, or why none can be read.
    async fn find_hosts(
        &self,
        app: &Arc<App>,
        user: &Arc<User>,
        project: &ProjectRow,
        entry: &Entry,
        copy: &mut RunsCopy,
    ) -> Result<Found, String> {
        let opened = credentials(app, user).await?;
        let fleet = NodeFleet {
            app: app.clone(),
            user: user.clone(),
        };
        let places = hosts::find(&fleet, project, entry, &Tokens(&opened)).await?;
        if places.is_empty() {
            copy.set_places(places);
            return Err(NO_HOST.to_owned());
        }
        copy.set_places(places);
        let client = host_client().map_err(|e| {
            log::warn!("repo watch: no client for the host: {e:#}");
            HostError::Internal("the host".to_owned()).to_string()
        })?;
        let mut targets = Vec::with_capacity(copy.hosts.len());
        for slot in &copy.hosts {
            targets.push(target(user.id(), &opened, &slot.place).await);
        }
        Ok(Found { client, targets })
    }

    /// Reads every host of `copy` concurrently per `plan`; low-limit hosts only on Refresh. Whether all read.
    async fn read_hosts(
        &self,
        calls: &Arc<Semaphore>,
        client: &Client,
        copy: &mut RunsCopy,
        targets: Vec<Result<Target, String>>,
        plan: Plan,
    ) -> bool {
        let reads = copy
            .hosts
            .iter_mut()
            .zip(targets)
            .map(|(slot, target)| async move {
                let target = match target {
                    Ok(target) => target,
                    Err(why) => {
                        slot.copy.error = Some(why);
                        return false;
                    }
                };
                slot.copy.who = Some(target.who.clone());
                let limited = slot.copy.limited(&self.calls).is_some();
                let poll = plan.poll && !limited && slot.copy.going();
                if plan.forced || (plan.stale && !limited) || poll {
                    let caller = target.caller(calls, client);
                    // Reread the list too: an unchanged one is a free 304, and it shows when a run ends.
                    self.read_list(&caller, &target, &mut slot.copy).await;
                    if poll || slot.copy.going() {
                        self.read_running(&caller, &target, &mut slot.copy).await;
                    }
                }
                slot.copy.error.is_none()
            });
        join_all(reads).await.into_iter().all(|ok| ok)
    }

    async fn read_list(
        &self,
        caller: &beep_projects::git_items::Caller<'_>,
        target: &Target,
        copy: &mut HostCopy,
    ) {
        match runs::list(&self.calls, caller, target).await {
            Ok(runs) => {
                copy.jobs
                    .retain(|id, _| runs.iter().any(|listed| listed.run.id == *id));
                copy.runs = runs;
                copy.error = None;
                copy.synced_at = Some(SystemTime::now());
            }
            Err(e) => copy.error = Some(e.to_string()),
        }
    }

    /// The jobs of the running runs, the newest few.
    async fn read_running(
        &self,
        caller: &beep_projects::git_items::Caller<'_>,
        target: &Target,
        copy: &mut HostCopy,
    ) {
        let running: Vec<u64> = copy
            .runs
            .iter()
            .filter(|listed| listed.run.state.going())
            .take(MAX_LIVE)
            .map(|listed| listed.run.id)
            .collect();
        for id in running {
            match runs::jobs(&self.calls, caller, target, id).await {
                Ok(jobs) => {
                    copy.jobs.insert(id, jobs);
                }
                // The progress shown stays the last read; the list says
                // why when the host is down.
                Err(e) => log::info!("repo watch: jobs of a run did not read: {e}"),
            }
        }
    }
}

/// A client and the targets of the hosts found.
struct Found {
    client: Client,
    /// One per host, in the copy's order.
    targets: Vec<Result<Target, String>>,
}

/// How a run's detail is read.
struct DetailRead<'a> {
    calls: &'a Arc<Semaphore>,
    client: &'a Client,
    target: &'a Target,
    /// Running jobs may be read again now.
    polled: bool,
}

/// The background sync's read of the run lists unless an ask read them meanwhile; whether it read.
pub(super) async fn sync_in_background(
    app: &Arc<App>,
    user: &Arc<User>,
    project: &ProjectRow,
    entry: &Entry,
) -> bool {
    let mut copy = entry.runs.copy.lock().await;
    if !entry.runs.stale() {
        return false;
    }
    let plan = Plan {
        forced: false,
        stale: true,
        poll: false,
    };
    app.repo_watch
        .sync(app, user, project, entry, &mut copy, plan)
        .await;
    true
}

#[cfg(test)]
mod tests;
