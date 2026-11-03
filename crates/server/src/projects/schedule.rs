//! When the project lifecycle wakes: PR checks and idle closes. [`run`] sleeps until one is due.

use super::model::{ProjectId, WorkerId};
use crate::app::App;
use chrono::{DateTime, Utc};
use std::{
    collections::HashMap,
    sync::{Arc, Weak},
};
use tokio::sync::Notify;

/// Something the lifecycle does at a set time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Due {
    /// Check the project's pull requests and CI.
    Check(ProjectId),
    /// Close the worker if it is still idle.
    Idle(WorkerId),
    /// Run the worker's waiting jobs, which found the queue full.
    Worker(WorkerId),
}

/// What is armed, and when. Arming something armed moves it.
#[derive(Debug, Default)]
pub(crate) struct Schedule {
    armed: HashMap<Due, DateTime<Utc>>,
}

impl Schedule {
    pub(crate) fn arm(&mut self, due: Due, at: DateTime<Utc>) {
        self.armed.insert(due, at);
    }

    pub(crate) fn disarm(&mut self, due: &Due) {
        self.armed.remove(due);
    }

    /// When the next one is due.
    pub(crate) fn next(&self) -> Option<DateTime<Utc>> {
        self.armed.values().min().copied()
    }

    /// Takes everything due by `now`, soonest first.
    pub(crate) fn take_due(&mut self, now: DateTime<Utc>) -> Vec<Due> {
        let mut due: Vec<(DateTime<Utc>, Due)> = self
            .armed
            .iter()
            .filter(|(_, at)| **at <= now)
            .map(|(due, at)| (*at, *due))
            .collect();
        due.sort_by_key(|(at, _)| *at);
        for (_, taken) in &due {
            self.armed.remove(taken);
        }
        due.into_iter().map(|(_, due)| due).collect()
    }

    #[cfg(test)]
    pub(crate) fn armed_at(&self, due: &Due) -> Option<DateTime<Utc>> {
        self.armed.get(due).copied()
    }
}

/// Sleeps until something is due or armed, then runs it. Holds the app weakly: the app owns it.
pub(crate) async fn run(app: Weak<App>, armed: Arc<Notify>) {
    loop {
        let notified = armed.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();
        let next = {
            let Some(app) = app.upgrade() else { return };
            app.projects.next_due()
        };
        match next {
            Some(at) => {
                let wait = (at - Utc::now()).to_std().unwrap_or_default();
                tokio::select! {
                    () = tokio::time::sleep(wait) => {}
                    () = &mut notified => {}
                }
            }
            None => notified.await,
        }
        let Some(app) = app.upgrade() else { return };
        let now = Utc::now();
        for due in app.projects.take_due(now) {
            app.projects.run_due(&app, due, now);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::projects::model::CHECK_EVERY;
    use chrono::TimeDelta;
    use uuid::Uuid;

    fn t0() -> DateTime<Utc> {
        DateTime::from_timestamp(1_800_000_000, 0).expect("a time")
    }

    #[test]
    fn checks_repeat_every_ten_minutes() {
        let project = Due::Check(ProjectId(Uuid::new_v4()));
        let mut schedule = Schedule::default();
        let every = TimeDelta::from_std(CHECK_EVERY).expect("ten minutes");
        schedule.arm(project, t0() + every);
        assert!(
            schedule
                .take_due(t0() + every - TimeDelta::seconds(1))
                .is_empty()
        );
        assert_eq!(schedule.take_due(t0() + every), vec![project]);
        // Taken once; re-armed by whoever handles it.
        assert!(schedule.take_due(t0() + every).is_empty());
        assert_eq!(schedule.next(), None);
    }

    #[test]
    fn idle_is_due_after_thirty_minutes() {
        let worker = Due::Idle(WorkerId(Uuid::new_v4()));
        let check = Due::Check(ProjectId(Uuid::new_v4()));
        let mut schedule = Schedule::default();
        let idle = TimeDelta::from_std(super::super::model::IDLE_CLOSE).expect("thirty minutes");
        schedule.arm(worker, t0() + idle);
        schedule.arm(check, t0() + TimeDelta::minutes(10));
        assert_eq!(schedule.next(), Some(t0() + TimeDelta::minutes(10)));
        assert!(
            schedule
                .take_due(t0() + idle - TimeDelta::seconds(1))
                .contains(&check)
        );
        assert!(schedule.take_due(t0() + TimeDelta::minutes(29)).is_empty());
        assert_eq!(schedule.take_due(t0() + idle), vec![worker]);
        // Arming again moves it rather than adding a second.
        schedule.arm(worker, t0());
        schedule.arm(worker, t0() + idle);
        assert_eq!(schedule.armed_at(&worker), Some(t0() + idle));
    }
}
