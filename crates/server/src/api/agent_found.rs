//! Checkouts the user's coding agents worked in, and sessions in imported projects' folders.

use super::{
    Reply,
    agent_control::{Live, live_hosts},
    agent_sessions::{IN_FLIGHT, listing},
};
use crate::{
    api::router::Cx,
    projects::imports,
    protocol::{node::agent::SESSIONS_PROTOCOL, *},
};
use beep_mesh::machine::listing::MAX_PROJECT_SESSIONS as MAX_SESSIONS;
use beep_projects::imports::found::MachineFound;
use futures::{StreamExt, stream};
use std::cmp::Reverse;

/// The most checkouts one machine names.
const FOLDERS_PER_MACHINE: u32 = 100;

/// Checkouts found on the caller's machines; a machine that cannot say is left out.
pub(super) async fn list_found_folders(cx: Cx, _: ListFoundFolders) -> Reply<Vec<FoundProject>> {
    let asks: Vec<_> = live_hosts(&cx)
        .await?
        .into_iter()
        .filter(|live| live.online.offers(SESSIONS_PROTOCOL))
        .map(machine_found)
        .collect();
    let found: Vec<MachineFound> = stream::iter(asks)
        .buffer_unordered(IN_FLIGHT)
        .filter_map(|found| async move { found })
        .collect()
        .await;
    Ok(imports::found_projects(&cx.app, &cx.user, found).await?)
}

async fn machine_found(live: Live) -> Option<MachineFound> {
    match live.online.found_folders(FOLDERS_PER_MACHINE).await {
        Ok(folders) => Some(MachineFound {
            machine_id: live.host.machine_id,
            machine_name: live.host.machine_name,
            folders,
        }),
        Err(e) => {
            log::info!(
                "found folders not listed; node={} error={e:#}",
                live.host.machine_id
            );
            None
        }
    }
}

/// `NotFound` unless the caller imported `project`; a machine slower than `NODE_LIMIT` is left out.
pub(super) async fn list_project_sessions(
    cx: Cx,
    request: ListProjectSessions,
) -> Reply<Vec<MachineSession>> {
    let hosts = live_hosts(&cx).await?;
    let places: Vec<_> = cx
        .user
        .imports()
        .await?
        .into_iter()
        .filter(|row| row.project.eq_ignore_ascii_case(&request.project))
        .collect();
    if places.is_empty() {
        return Err(ApiError::NotFound {});
    }
    let asks: Vec<_> = hosts
        .into_iter()
        .filter(|live| live.online.offers(SESSIONS_PROTOCOL))
        .filter_map(|live| {
            let folders: Vec<String> = places
                .iter()
                .filter(|row| row.endpoint == live.host.machine_id)
                .map(|row| row.folder.clone())
                .collect();
            (!folders.is_empty()).then_some(async move {
                listing::project_sessions(&live.online, &live.host(), folders).await
            })
        })
        .collect();
    let mut sessions: Vec<MachineSession> = stream::iter(asks)
        .buffer_unordered(IN_FLIGHT)
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .flatten()
        .collect();
    sessions.sort_by_key(|session| Reverse(session.last_activity));
    sessions.truncate(MAX_SESSIONS);
    Ok(sessions)
}
