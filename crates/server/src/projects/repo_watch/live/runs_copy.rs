//! The runs last read for a project, per host, and the views built from them.

use super::{
    super::{
        actions::{ListedRun, MirrorHead, PrHeadRef, mirror_checks, pr_checks, progress},
        hosts::RepoOnHost,
        http::{HostCalls, Who},
        watch::OpenHead,
    },
    NO_HOST,
};
use crate::protocol::{
    HostedChecks, HostedRun, HostedRuns, ProjectRuns, RunJob, RunLog, RunsHost, WorkflowRun,
};
use beep_projects::model::IssueHost;
use std::{
    collections::{HashMap, VecDeque},
    time::SystemTime,
};

/// One host's last runs, each run's jobs, and why the last read failed.
#[derive(Default)]
pub(super) struct HostCopy {
    pub(super) runs: Vec<ListedRun>,
    /// By run id; `None` when the host has no API for jobs.
    pub(super) jobs: HashMap<u64, Option<Vec<RunJob>>>,
    pub(super) error: Option<String>,
    pub(super) synced_at: Option<SystemTime>,
    /// Whose rate limit the last reads used, to check it without the target.
    pub(super) who: Option<Who>,
}

impl HostCopy {
    pub(super) fn limited(&self, calls: &HostCalls) -> Option<SystemTime> {
        calls.limited_until(self.who.as_ref()?, SystemTime::now())
    }

    pub(super) fn going(&self) -> bool {
        self.runs.iter().any(|listed| listed.run.state.going())
    }

    /// The runs with the progress of each running one.
    pub(super) fn shown(&self) -> Vec<WorkflowRun> {
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
pub(super) struct HostSlot {
    pub(super) place: RepoOnHost,
    pub(super) copy: HostCopy,
}

/// A failed job's log, by host and job.
pub(super) struct KeptLog {
    pub(super) host: IssueHost,
    pub(super) job: u64,
    pub(super) log: RunLog,
}

/// Every host's runs and the logs read.
#[derive(Default)]
pub(crate) struct RunsCopy {
    /// The primary host first.
    pub(super) hosts: Vec<HostSlot>,
    /// Why no host could be read at all.
    pub(super) error: Option<String>,
    /// Oldest first.
    pub(super) logs: VecDeque<KeptLog>,
}

impl RunsCopy {
    /// Whether some host's runs were read once.
    pub(super) fn read_once(&self) -> bool {
        self.hosts.iter().any(|slot| slot.copy.synced_at.is_some())
    }

    /// Takes the hosts found, keeping what was read of the ones still there.
    pub(super) fn set_places(&mut self, places: Vec<RepoOnHost>) {
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

    pub(super) fn primary(&self) -> Option<&HostSlot> {
        self.hosts.iter().find(|slot| slot.place.primary)
    }

    /// Which slot `host` names: the primary one for `None`.
    pub(super) fn slot_of(&self, host: Option<&IssueHost>) -> Option<usize> {
        self.hosts.iter().position(|slot| match host {
            None => slot.place.primary,
            Some(host) => slot.place.host == *host,
        })
    }

    pub(super) fn any_going(&self, calls: &HostCalls) -> bool {
        self.hosts
            .iter()
            .any(|slot| slot.copy.going() && slot.copy.limited(calls).is_none())
    }

    /// What `ListProjectRuns` gives: the primary host's runs alone.
    pub(super) fn primary_view(&self, calls: &HostCalls, heads: &[OpenHead]) -> ProjectRuns {
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

    /// What `ListProjectRunsAll` gives: every host's runs newest first, and PR checks per host.
    pub(super) fn hosted_view(&self, calls: &HostCalls, heads: &[OpenHead]) -> HostedRuns {
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

pub(super) fn head_refs(heads: &[OpenHead]) -> Vec<PrHeadRef<'_>> {
    heads
        .iter()
        .map(|head| PrHeadRef {
            number: head.number,
            head: &head.branch,
        })
        .collect()
}

/// Sorts runs newest first by start, or by finish for one cancelled before it
/// started, with those still waiting to start first; ties keep their order.
pub(super) fn newest_first(runs: &mut [HostedRun]) {
    runs.sort_by(|a, b| {
        let key = |r: &HostedRun| {
            let at = r.run.started_at.or(r.run.finished_at);
            (at.is_none(), at)
        };
        key(b).cmp(&key(a))
    });
}
