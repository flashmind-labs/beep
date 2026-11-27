//! Keeps the copies of the machines' search indexes current over each machine's `FollowIndex`
//! stream, and searches them for Cmd+K, so it answers at once and finds sessions of offline
//! machines too. The copies themselves are kept in `palette_store`.

use super::projects::Found;
use crate::{fleet::Fleet, palette_store::copies::Copies, store::lock};
use anyhow::{Context, bail};
use beep_fleet::alerts::each_machine;
use beep_mesh::RETRY;
use beep_proto::node::agent::{SESSIONS_PROTOCOL, SessionsReply, SessionsRequest};
use futures::StreamExt;
use iroh::EndpointId;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager};
use tokio::{task::spawn_blocking, time::sleep};

/// Follows every online machine's index while a mesh is open.
pub(super) async fn follow(app: AppHandle) {
    let fleet = app.state::<Arc<Fleet>>().inner().clone();
    let copies = app.state::<Arc<Copies>>().inner().clone();

    loop {
        let mesh = match fleet.ready().await {
            Ok(mesh) => mesh,
            Err(e) => {
                log::info!("no mesh to follow search indexes in: {e}");
                sleep(RETRY).await;
                continue;
            }
        };
        let members: Vec<EndpointId> = mesh
            .code_hosts()
            .into_iter()
            .map(|status| status.device.id)
            .collect();
        copies.keep(&members);
        // Ends when the mesh closes, such as on joining another.
        each_machine(&mesh, |machine| {
            tokio::spawn(follow_machine(app.clone(), copies.clone(), machine))
        })
        .await;
    }
}

/// Keeps `machine`'s copy current while the machine is online.
async fn follow_machine(app: AppHandle, copies: Arc<Copies>, machine: EndpointId) {
    loop {
        if let Err(e) = copy_from(&app, &copies, machine).await {
            log::debug!("following the search index of {machine}: {e:#}");
        }

        sleep(RETRY).await;
    }
}

/// Applies the rows `machine` sends until its stream ends.
async fn copy_from(
    app: &AppHandle,
    copies: &Arc<Copies>,
    machine: EndpointId,
) -> anyhow::Result<()> {
    let opening = copies.clone();
    // Opening a new copy writes its schema: off the async workers.
    let copy = spawn_blocking(move || opening.copy(machine)).await??;
    let held = copy.clone();
    let (mut copied, after) = spawn_blocking(move || lock(&held).position()).await??;
    let fleet = app.state::<Arc<Fleet>>().inner().clone();
    let client = fleet.client(machine).await?;
    let mut replies = Box::pin(
        client
            .follow::<_, SessionsReply>(
                SESSIONS_PROTOCOL,
                &SessionsRequest::FollowIndex {
                    epoch: copied.clone(),
                    after,
                },
            )
            .await?,
    );
    while let Some(reply) = replies.next().await {
        match reply? {
            SessionsReply::IndexRows { epoch, rows, .. } => {
                // A new index replaces the copy even with no rows yet.
                let changed = !rows.is_empty() || epoch != copied;
                let held = copy.clone();
                let applied = epoch.clone();
                spawn_blocking(move || lock(&held).apply(&applied, &rows))
                    .await?
                    .context("saving the rows")?;
                copied = epoch;

                if changed {
                    let _ = app.emit("palette-text-changed", ());
                }
            }
            SessionsReply::Error { message } => bail!(message),
            _ => {}
        }
    }

    Ok(())
}

/// The hits of every copy, for the window.
pub(super) async fn find(app: &AppHandle, query: &str) -> anyhow::Result<Vec<Found>> {
    let copies = app.state::<Arc<Copies>>().inner().clone();
    let query = query.to_owned();
    let hits = spawn_blocking(move || copies.hits(&query)).await?;

    Ok(hits
        .into_iter()
        .map(|(machine, hit)| Found {
            machine: machine.to_string(),
            hit,
        })
        .collect())
}
