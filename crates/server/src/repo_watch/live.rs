//! The copy of a project's workflow runs on each host its repository is on,
//! and the live progress of the running ones: their jobs are read again
//! only while a run is in progress and someone has the project page open,
//! at most every 10 seconds, and not while that host's rate limit for the
//! token runs low.

use super::{
    actions::{ListedRun, MirrorHead, PrHeadRef, mirror_checks, pr_checks, progress},
    hosts::{self, RepoOnHost, Tokens, host_name},
    http::{HostCalls, Who},
    runs::{self, Target, credentials, target},
    watch::{Entry, OpenHead, RepoWatch},
};
use crate::{
    app::App,
    credentials::host_client,
    git_items::HostError,
    projects::{fleet::NodeFleet, model::IssueHost},
    protocol::{
        HostedChecks, HostedRun, HostedRuns, ProjectRunDetail, ProjectRuns, RunJob, RunLog,
        RunState, RunsHost, WorkflowRun,
    },
    store::ProjectRow,
    user::User,
};
use futures::future::join_all;
use reqwest::Client;
use std::{
    collections::{HashMap, VecDeque},
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

/// One host's last runs read, the jobs read of each, and why the last read
/// failed.
#[derive(Default)]
struct HostCopy {
    runs: Vec<ListedRun>,
    /// By run id; `None` when the host has no API for jobs.
    jobs: HashMap<u64, Option<Vec<RunJob>>>,
    error: Option<String>,
    synced_at: Option<SystemTime>,
    /// Whose rate limit the last reads used, to check it without the
    /// target.
    who: Option<Who>,
}

impl HostCopy {
    fn limited(&self, calls: &HostCalls) -> Option<SystemTime> {
        calls.limited_until(self.who.as_ref()?, SystemTime::now())
    }

    fn going(&self) -> bool {
        self.runs.iter().any(|listed| listed.run.state.going())
    }

    /// The runs with the progress of each running one.
    fn shown(&self) -> Vec<WorkflowRun> {
        self.runs
            .iter()
            .map(|listed| {
                let mut run = listed.run.clone();
                if run.state.going()
                    && let Some(Some(jobs)) = self.jobs.get(&run.id)
                {
                    run.progress = progress(jobs);
                }
                run
            })
            .collect()
    }
}

/// A host the project's repository is on and its copy.
struct HostSlot {
    place: RepoOnHost,
    copy: HostCopy,
}

/// A failed job's log, by host and job.
struct KeptLog {
    host: IssueHost,
    job: u64,
    log: RunLog,
}

/// Every host's runs and the logs read.
#[derive(Default)]
pub(crate) struct RunsCopy {
    /// The primary host first.
    hosts: Vec<HostSlot>,
    /// Why no host could be read at all.
    error: Option<String>,
    /// Oldest first.
    logs: VecDeque<KeptLog>,
}

impl RunsCopy {
    /// Takes the hosts found, keeping what was read of the ones still
    /// there.
    fn set_places(&mut self, places: Vec<RepoOnHost>) {
        let mut old = std::mem::take(&mut self.hosts);
        self.hosts = places
            .into_iter()
            .map(|place| {
                let copy = old
                    .iter()
                    .position(|slot| slot.place.host == place.host && slot.place.repo == place.repo)
                    .map(|at| old.swap_remove(at).copy)
                    .unwrap_or_default();
                HostSlot { place, copy }
            })
            .collect();
    }

    fn primary(&self) -> Option<&HostSlot> {
        self.hosts.iter().find(|slot| slot.place.primary)
    }

    /// Which slot `host` names: the primary one for `None`.
    fn slot_of(&self, host: Option<&IssueHost>) -> Option<usize> {
        self.hosts.iter().position(|slot| match host {
            None => slot.place.primary,
            Some(host) => slot.place.host == *host,
        })
    }

    fn any_going(&self, calls: &HostCalls) -> bool {
        self.hosts
            .iter()
            .any(|slot| slot.copy.going() && slot.copy.limited(calls).is_none())
    }

    /// What `ListProjectRuns` gives: the primary host's runs alone, as
    /// before the other hosts were read.
    fn primary_view(&self, calls: &HostCalls, heads: &[OpenHead]) -> ProjectRuns {
        let Some(slot) = self.primary() else {
            return ProjectRuns {
                runs: Vec::new(),
                checks: Vec::new(),
                synced_at: None,
                error: Some(self.error.clone().unwrap_or_else(|| NO_HOST.to_owned())),
                rate_limited_until: None,
            };
        };
        let shown = slot.copy.shown();
        ProjectRuns {
            checks: pr_checks(&shown, &head_refs(heads)),
            runs: shown,
            synced_at: slot.copy.synced_at,
            error: self.error.clone().or_else(|| slot.copy.error.clone()),
            rate_limited_until: slot.copy.limited(calls),
        }
    }

    /// What `ListProjectRunsAll` gives: every host's runs, newest first,
    /// and each pull request's checks on each host.
    fn hosted_view(&self, calls: &HostCalls, heads: &[OpenHead]) -> HostedRuns {
        let mut runs = Vec::new();
        let mut checks = Vec::new();
        let mut primary_checks = Vec::new();
        if let Some(slot) = self.primary() {
            let shown = slot.copy.shown();
            primary_checks = pr_checks(&shown, &head_refs(heads));
        }
        let sha_of = |run_id: u64| {
            self.primary()
                .and_then(|slot| slot.copy.runs.iter().find(|l| l.run.id == run_id))
                .map(|listed| listed.sha.as_str())
                .unwrap_or_default()
        };
        let mirrored: Vec<MirrorHead<'_>> = heads
            .iter()
            .map(|head| MirrorHead {
                number: head.number,
                branch: &head.branch,
                sha: primary_checks
                    .iter()
                    .find(|checks| checks.number == head.number)
                    .map(|checks| sha_of(checks.run_id))
                    .unwrap_or_default(),
            })
            .collect();
        for slot in &self.hosts {
            let host = slot.place.name();
            let shown = slot.copy.shown();
            let found = if slot.place.primary {
                primary_checks.clone()
            } else {
                let listed: Vec<ListedRun> = slot
                    .copy
                    .runs
                    .iter()
                    .zip(&shown)
                    .map(|(listed, run)| ListedRun {
                        run: run.clone(),
                        sha: listed.sha.clone(),
                    })
                    .collect();
                mirror_checks(&listed, &mirrored)
            };
            checks.extend(found.into_iter().map(|found| HostedChecks {
                host: host.clone(),
                checks: found,
            }));
            runs.extend(shown.into_iter().map(|run| HostedRun {
                host: host.clone(),
                run,
            }));
        }
        newest_first(&mut runs);
        HostedRuns {
            runs,
            checks,
            hosts: self
                .hosts
                .iter()
                .map(|slot| RunsHost {
                    host: slot.place.name(),
                    kind: slot.place.kind(),
                    repo: slot.place.repo.to_string(),
                    primary: slot.place.primary,
                    synced_at: slot.copy.synced_at,
                    error: slot.copy.error.clone(),
                    rate_limited_until: slot.copy.limited(calls),
                })
                .collect(),
            error: self
                .error
                .clone()
                .or_else(|| self.hosts.is_empty().then(|| NO_HOST.to_owned())),
        }
    }
}

fn head_refs(heads: &[OpenHead]) -> Vec<PrHeadRef<'_>> {
    heads
        .iter()
        .map(|head| PrHeadRef {
            number: head.number,
            head: &head.branch,
        })
        .collect()
}

/// Sorts runs from several hosts newest first by when they started; one
/// not started yet is newest. Runs that tie keep their order.
fn newest_first(runs: &mut [HostedRun]) {
    runs.sort_by(|a, b| {
        let key = |r: &HostedRun| (r.run.started_at.is_none(), r.run.started_at);
        key(b).cmp(&key(a))
    });
}

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

    /// The copy, its lists read again as `lists` reads its lists, and the
    /// running runs' jobs while the page asks.
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
        let stale = entry.runs.stale();
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

    /// The project's hosts into `copy`, with a target for each, or why
    /// none can be read.
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

    /// Reads every host of `copy` as `plan` says, at once; `targets` are
    /// theirs, in order. A host whose rate limit runs low is read only on
    /// Refresh. Whether every host read.
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
                    // The list again too while polling: unchanged, it is a
                    // 304 GitHub does not count, and it says when a run
                    // ends.
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
        caller: &crate::git_items::Caller<'_>,
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
        caller: &crate::git_items::Caller<'_>,
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

    /// One run on `host` (the primary host for `None`) with its jobs and,
    /// when it failed, its failing job's log.
    pub(crate) async fn run_detail(
        &self,
        app: &Arc<App>,
        user: &Arc<User>,
        project: &ProjectRow,
        host: Option<&IssueHost>,
        id: u64,
    ) -> Result<ProjectRunDetail, String> {
        let entry = self.entry(user.id(), project.id);
        entry.runs.ask(Instant::now(), false);
        let mut copy = entry.runs.copy.lock().await;
        if copy.slot_of(host).is_none() {
            // Not read yet, or a host found since: look again.
            self.find_hosts(app, user, project, &entry, &mut copy)
                .await?;
        }
        let Some(at) = copy.slot_of(host) else {
            return Err(match host {
                None => NO_HOST.to_owned(),
                Some(host) => format!("This project's repository is not on {}.", host_name(host)),
            });
        };
        let opened = credentials(app, user).await?;
        let target = target(user.id(), &opened, &copy.hosts[at].place).await?;
        let client = host_client().map_err(|e| {
            log::warn!("repo watch: no client for the host: {e:#}");
            HostError::Internal(target.host.clone()).to_string()
        })?;
        let RunsCopy { hosts, logs, .. } = &mut *copy;
        let slot = &mut hosts[at];
        slot.copy.who = Some(target.who.clone());
        let polled =
            slot.copy.limited(&self.calls).is_none() && entry.runs.timing().poll(Instant::now());
        let read = DetailRead {
            calls: app.git_items.calls(),
            client: &client,
            target: &target,
            polled,
        };
        self.detail_on(&read, slot, logs, id).await
    }

    /// Run `id` of `slot`, read as `read` says.
    async fn detail_on(
        &self,
        read: &DetailRead<'_>,
        slot: &mut HostSlot,
        logs: &mut VecDeque<KeptLog>,
        id: u64,
    ) -> Result<ProjectRunDetail, String> {
        let target = read.target;
        let caller = target.caller(read.calls, read.client);
        let copy = &mut slot.copy;
        let limited = copy.limited(&self.calls);
        let mut problems = Vec::new();

        let known = copy
            .runs
            .iter()
            .find(|listed| listed.run.id == id)
            .map(|listed| listed.run.clone());
        let run = match known {
            Some(run) if !(run.state.going() && read.polled) => run,
            known => match runs::one(&self.calls, &caller, target, id).await {
                Ok(run) => {
                    if let Some(at) = copy.runs.iter_mut().find(|l| l.run.id == id) {
                        at.run = run.clone();
                    }
                    run
                }
                Err(e) => match known {
                    Some(run) => {
                        problems.push(e.to_string());
                        run
                    }
                    None => return Err(e.to_string()),
                },
            },
        };

        let cached = copy.jobs.get(&id).cloned();
        let stale_jobs = |jobs: &Option<Vec<RunJob>>| {
            jobs.as_ref()
                .is_some_and(|jobs| jobs.iter().any(|job| job.state.going()))
        };
        let read_jobs = match &cached {
            None => limited.is_none(),
            Some(jobs) => {
                stale_jobs(jobs) && (read.polled || run.state.finished()) && limited.is_none()
            }
        };
        let jobs = if read_jobs {
            match runs::jobs(&self.calls, &caller, target, id).await {
                Ok(jobs) => {
                    copy.jobs.insert(id, jobs.clone());
                    jobs
                }
                Err(e) => {
                    problems.push(e.to_string());
                    cached.flatten()
                }
            }
        } else {
            cached.flatten()
        };
        let jobs = jobs.unwrap_or_default();

        let host = &slot.place.host;
        let log = match jobs.iter().find(|job| job.state == RunState::Failed {}) {
            Some(job) if run.state == (RunState::Failed {}) => {
                match logs
                    .iter()
                    .find(|kept| kept.host == *host && kept.job == job.id)
                {
                    Some(kept) => Some(kept.log.clone()),
                    None if limited.is_some() => None,
                    None => {
                        let step = job
                            .steps
                            .iter()
                            .find(|step| step.state == RunState::Failed {})
                            .map(|step| step.name.clone());
                        match runs::log(&self.calls, &caller, target, job, step).await {
                            Ok(log) => {
                                if logs.len() >= MAX_LOGS {
                                    logs.pop_front();
                                }
                                logs.push_back(KeptLog {
                                    host: host.clone(),
                                    job: job.id,
                                    log: log.clone(),
                                });
                                Some(log)
                            }
                            Err(e) => {
                                problems.push(format!("The log did not read: {e}"));
                                None
                            }
                        }
                    }
                }
            }
            _ => None,
        };

        let mut run = run;
        run.progress = if run.state.going() {
            progress(&jobs)
        } else {
            None
        };
        Ok(ProjectRunDetail {
            run,
            jobs,
            log,
            error: (!problems.is_empty()).then(|| problems.join(" ")),
            rate_limited_until: copy.limited(&self.calls),
        })
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

/// The background sync's read of the run lists, unless an ask read them
/// meanwhile.
pub(super) async fn sync_in_background(
    app: &Arc<App>,
    user: &Arc<User>,
    project: &ProjectRow,
    entry: &Entry,
) {
    let mut copy = entry.runs.copy.lock().await;
    if !entry.runs.stale() {
        return;
    }
    let plan = Plan {
        forced: false,
        stale: true,
        poll: false,
    };
    app.repo_watch
        .sync(app, user, project, entry, &mut copy, plan)
        .await;
}

#[cfg(test)]
mod tests;
