//! Routines: scheduled prompts, listed, created, changed, run and deleted.

use crate::{beep::Service, error::Result};
use beep_proto::{CreateCron, Cron, DeleteCron, ListCrons, RunCron, ToggleCron, UpdateCron};

#[tauri::command]
pub async fn list_crons(beep: Service<'_>) -> Result<Vec<Cron>> {
    beep.call(ListCrons {}).await
}

#[tauri::command]
pub async fn create_cron(
    beep: Service<'_>,
    label: String,
    prompt: String,
    schedule: String,
) -> Result<String> {
    beep.call(CreateCron {
        label,
        prompt,
        schedule,
    })
    .await
}

#[tauri::command]
pub async fn toggle_cron(beep: Service<'_>, id: String) -> Result<bool> {
    beep.call(ToggleCron { id }).await
}

/// Renames routine `id` or changes when it runs. `None` leaves that part as it is.
#[tauri::command]
pub async fn update_cron(
    beep: Service<'_>,
    id: String,
    label: Option<String>,
    schedule: Option<String>,
) -> Result<()> {
    beep.call(UpdateCron {
        id,
        enabled: None,
        label,
        schedule,
    })
    .await
}

#[tauri::command]
pub async fn delete_cron(beep: Service<'_>, id: String) -> Result<()> {
    beep.call(DeleteCron { id }).await
}

/// Runs routine `id` now and returns the session it runs in.
#[tauri::command]
pub async fn run_cron(beep: Service<'_>, id: String) -> Result<String> {
    beep.call(RunCron { id }).await
}
