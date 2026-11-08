//! The Projects overview: one card per project with its tasks per state,
//! its workers and what changed last, from one call for all of them.

use crate::{
    beep::{Service, epoch_ms},
    center::kept,
    center::view_cache::{View, ViewCache},
    error::Result,
    projects::known,
};
use beep_proto::{ListProjectOverview, ProjectOverview, ProjectTask, TaskCounts};
use serde::Serialize;
use std::sync::Arc;
use tauri::State;
use uuid::Uuid;

/// One project's card.
#[derive(Debug, Serialize)]
pub struct OverviewCard {
    project_id: Uuid,
    /// `org/repo`.
    name: String,
    tasks: TaskCounts,
    /// Workers that have not ended.
    workers: u32,
    latest: Option<LatestTask>,
}

/// The task that changed last, for the card's status line.
#[derive(Debug, Serialize)]
struct LatestTask {
    slug: String,
    title: String,
    /// `planned`, `running`, `blocked`, `review` or `done`.
    state: &'static str,
    pr_url: Option<String>,
    /// In milliseconds since the Unix epoch.
    updated_at: u64,
}

impl From<ProjectTask> for LatestTask {
    fn from(task: ProjectTask) -> Self {
        Self {
            slug: task.slug,
            title: task.title,
            state: task.state.into(),
            pr_url: task.pr_url,
            updated_at: epoch_ms(task.updated_at),
        }
    }
}

impl From<ProjectOverview> for OverviewCard {
    fn from(p: ProjectOverview) -> Self {
        Self {
            project_id: p.project_id,
            name: p.name,
            tasks: p.tasks,
            workers: p.workers,
            latest: p.latest.map(Into::into),
        }
    }
}

/// Every project's card, by name; `None` from a server too old to sum them
/// up, when the page shows its table instead.
#[tauri::command]
pub async fn project_overview(
    beep: Service<'_>,
    cache: State<'_, Arc<ViewCache>>,
) -> Result<Option<Vec<OverviewCard>>> {
    let cards = known(beep.call(ListProjectOverview {}).await)
        .map(|rows| rows.map(|rows| rows.into_iter().map(Into::into).collect()));
    kept(&cache, View::Overview, cards)
}

#[cfg(test)]
#[path = "../tests/projects_overview.rs"]
mod tests;
