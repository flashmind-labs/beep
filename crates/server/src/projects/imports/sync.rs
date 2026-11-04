//! Starting sync between the checkouts of a new project.

use crate::{account::user::User, app::App, nodes::push_sync};
use beep_projects::imports::ImportError;
use beep_projects::imports::records::SyncRow;
use beep_proto::{CheckoutFailure, ImportedProject};

/// Starts syncing the checkouts with each other; a machine that refuses is listed and keeps its copy.
pub(super) async fn join_sync(
    app: &App,
    user: &User,
    imported: &[ImportedProject],
    failed: &mut Vec<CheckoutFailure>,
) -> anyhow::Result<()> {
    let syncing: Vec<&ImportedProject> = imported
        .iter()
        .filter(|landed| {
            app.nodes
                .online(&landed.machine_id)
                .is_some_and(|node| node.syncs())
        })
        .collect();
    if syncing.len() < 2 {
        return Ok(());
    }
    for landed in &syncing {
        user.set_project_sync(&SyncRow {
            project: landed.project.clone(),
            endpoint: landed.machine_id.clone(),
            folder: landed.folder.clone(),
        })
        .await?;
    }
    let mut refused = false;
    for refusal in push_sync(&app.nodes, user, None).await? {
        let endpoint = refusal.node.to_string();
        let Some(landed) = syncing.iter().find(|landed| {
            landed.machine_id == endpoint && landed.project == refusal.refused.project
        }) else {
            continue;
        };
        user.remove_project_sync(&landed.project, &endpoint).await?;
        refused = true;
        failed.push(CheckoutFailure {
            machine_id: endpoint,
            machine_name: landed.machine_name.clone(),
            why: ImportError::CannotSync {
                machine: landed.machine_name.clone(),
                why: refusal.refused.message,
            }
            .to_string(),
        });
    }
    if refused {
        push_sync(&app.nodes, user, None).await?;
    }
    Ok(())
}
