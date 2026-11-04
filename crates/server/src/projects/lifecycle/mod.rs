//! Running project workers: their reports, re-attaching, PR checks, idle close and queued starts.

mod checks;
mod turns;
mod workers;
pub(crate) use checks::on_due;
use turns::{Working, catch_up, relabel, working};
pub(crate) use turns::{block, caught_up, on_news};
pub(crate) use workers::{
    fill_slots, finish_task, idle_close, machine_removed, reconnected, refill, start_failed,
};

use super::{
    fleet::{Fleet, NodeFleet},
    model::{CHECK_EVERY, IDLE_CLOSE, TaskState, WorkerId},
    schedule::Due,
    turn::REPLAN_LABEL,
};
use crate::app::App;
use anyhow::Result;
use beep_projects::commands::{LABEL_BLOCKED, LABEL_RUNNING};
use chrono::{DateTime, TimeDelta, Utc};
use std::{sync::Arc, time::Duration};

/// Workers re-attached at once after a reconnect.
const REATTACH_AT_ONCE: usize = 4;
/// Most pull requests one check reads, and how long each may take.
const MAX_CHECKED: usize = 8;
const PR_LIMIT: Duration = Duration::from_secs(10);
/// Most queued tasks one pass tries to start.
const MAX_FILLS: usize = 6;
/// The longest last message a report quotes.
const MAX_QUOTED: usize = 4000;

fn after(at: DateTime<Utc>, wait: Duration) -> DateTime<Utc> {
    at + TimeDelta::from_std(wait).unwrap_or(TimeDelta::MAX)
}

/// The owner of `worker` and a fleet of their nodes, while signed in.
async fn fleet_of_worker(app: &Arc<App>, worker: WorkerId) -> Result<Option<NodeFleet>> {
    let Some(owner) = app.store.worker_owner(worker).await? else {
        return Ok(None);
    };
    Ok(fleet_of(app, &owner))
}

fn fleet_of(app: &Arc<App>, owner: &str) -> Option<NodeFleet> {
    Some(NodeFleet::new(app, &app.user(owner)?))
}

/// At server start: fails workers left starting and arms pending checks and idle closes.
pub(crate) async fn startup(app: &Arc<App>) -> Result<()> {
    for worker in app.store.fail_starting_workers().await? {
        let Some(fleet) = fleet_of_worker(app, worker.id).await? else {
            continue;
        };
        let user = fleet.user.clone();
        let Some(Working {
            task,
            project,
            chat,
            machine,
            ..
        }) = working(&fleet, &user, worker.id).await?
        else {
            continue;
        };
        if task.state.is_open() && task.state != TaskState::Review {
            user.set_task_state(task.id, TaskState::Blocked).await?;
            relabel(
                &fleet,
                &user,
                &project,
                &task,
                Some(LABEL_BLOCKED),
                &[LABEL_RUNNING],
            )
            .await;
        }
        fleet.post(
            chat,
            REPLAN_LABEL,
            format!(
                "The worker for {} on {machine} did not start: the server restarted while it was \
                 starting.",
                task.slug
            ),
        );
    }
    let now = Utc::now();
    for project in app.store.projects_wanting_checks().await? {
        app.projects
            .arm(Due::Check(project), after(now, CHECK_EVERY));
    }
    for worker in app.store.waiting_workers().await? {
        let last = worker.last_turn_at.unwrap_or(now);
        app.projects
            .arm(Due::Idle(worker.id), after(last, IDLE_CLOSE));
    }
    Ok(())
}

pub(crate) fn quote(text: &str) -> String {
    match text.char_indices().nth(MAX_QUOTED) {
        Some((end, _)) => format!("{}...", &text[..end]),
        None => text.to_owned(),
    }
}

#[cfg(test)]
mod tests;
