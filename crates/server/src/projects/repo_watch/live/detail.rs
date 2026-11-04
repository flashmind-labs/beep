//! One CI run's jobs and logs, read from its host.

use super::{
    super::{
        actions::progress,
        hosts::host_name,
        runs::{self, credentials, target},
        watch::RepoWatch,
    },
    DetailRead, HostSlot, KeptLog, MAX_LOGS, NO_HOST, RunsCopy,
};
use crate::{
    account::credentials::host_client,
    account::user::User,
    app::App,
    projects::git_items::HostError,
    projects::model::IssueHost,
    protocol::{ProjectRunDetail, RunJob, RunState},
    store::ProjectRow,
};
use std::{collections::VecDeque, sync::Arc, time::Instant};

impl RepoWatch {
    /// One run on `host` (`None` is primary) with its jobs and, if failed, the failing job's log.
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
    pub(super) async fn detail_on(
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
