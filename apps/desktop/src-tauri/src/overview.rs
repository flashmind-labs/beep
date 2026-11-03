//! The Projects overview: one card per project with its tasks per state,
//! its workers and what changed last, from one call for all of them.

use crate::{
    beep::{Beep, epoch_ms},
    center::kept,
    error::Result,
    projects::unknown_op,
    view_cache::{View, ViewCache},
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
    beep: State<'_, Arc<Beep>>,
    cache: State<'_, Arc<ViewCache>>,
) -> Result<Option<Vec<OverviewCard>>> {
    let cards = match beep.call(ListProjectOverview {}).await {
        Ok(rows) => Ok(Some(rows.into_iter().map(Into::into).collect())),
        Err(e) if unknown_op(&e) => Ok(None),
        Err(e) => Err(e),
    };
    kept(&cache, View::Overview, cards)
}

#[cfg(test)]
mod tests {
    use super::*;
    use beep_proto::ProjectTaskState;
    use std::time::{Duration, UNIX_EPOCH};

    #[test]
    fn a_card_keeps_the_counts_and_names_the_latest_state() {
        let at = UNIX_EPOCH + Duration::from_secs(10);
        let card = OverviewCard::from(ProjectOverview {
            project_id: Uuid::nil(),
            name: "acme/widget".into(),
            tasks: TaskCounts {
                planned: 2,
                running: 1,
                blocked: 0,
                review: 1,
                done: 7,
            },
            workers: 1,
            latest: Some(ProjectTask {
                id: Uuid::nil(),
                slug: "login-copy".into(),
                title: "Login screen copy".into(),
                body: String::new(),
                state: ProjectTaskState::Review {},
                depends_on: None,
                issue_number: None,
                issue_url: None,
                branch: None,
                pr_url: Some("https://example.com/pr/41".into()),
                created_at: at,
                updated_at: at,
            }),
        });
        let json = serde_json::to_value(&card).expect("a card serializes");
        assert_eq!(json["tasks"]["done"], 7);
        assert_eq!(json["latest"]["state"], "review");
        assert_eq!(json["latest"]["updated_at"], 10_000);
    }
}
