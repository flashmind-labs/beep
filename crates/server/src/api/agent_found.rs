//! What the user's coding agents left on their machines: the git checkouts
//! they worked in, to suggest as projects, and the sessions that ran in an
//! imported project's folders.

use super::{
    Reply,
    agent_control::{Live, live_hosts},
    agent_sessions::{Host, IN_FLIGHT, limited},
};
use crate::{
    imports::{self, MachineFound},
    protocol::{
        node::agent::{ExternalSession, Presence, SESSIONS_PROTOCOL},
        *,
    },
    router::Cx,
};
use futures::{StreamExt, stream};
use std::{
    cmp::Reverse,
    time::{Duration, Instant, UNIX_EPOCH},
};

/// The most checkouts one machine names.
const FOLDERS_PER_MACHINE: u32 = 100;
/// The most sessions a project lists.
const MAX_SESSIONS: usize = 50;
/// A node slower than this to list a project's sessions is logged.
const SLOW_NODE: Duration = Duration::from_secs(1);

/// The caller's own machines, like `ListMachines`. A machine that cannot
/// say is left out.
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

/// `NotFound` unless the caller imported `project`. The machines are asked
/// side by side, `IN_FLIGHT` at a time, each for at most `NODE_LIMIT`; one
/// that is slower is left out, so it does not hold up the rest.
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
            (!folders.is_empty()).then(|| machine_sessions(live, folders))
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

async fn machine_sessions(live: Live, folders: Vec<String>) -> Vec<MachineSession> {
    let limit = u32::try_from(MAX_SESSIONS).unwrap_or(u32::MAX);
    let started = Instant::now();
    let listed = limited(live.online.sessions_in(folders, limit)).await;
    let took = started.elapsed();
    if took >= SLOW_NODE {
        log::info!(
            "project sessions listed slowly; node={} ms={}",
            live.host.machine_id,
            took.as_millis()
        );
    }
    match listed {
        Ok(sessions) => {
            let host = live.host();
            sessions
                .into_iter()
                .map(|session| past_session(&host, session))
                .collect()
        }
        Err(e) => {
            log::info!(
                "project sessions not listed; node={} error={e:#}",
                live.host.machine_id
            );
            Vec::new()
        }
    }
}

/// A session as a project lists it. One Beep took over opens as Beep's own
/// session. Only Claude Code's outside sessions can be followed live, so
/// another agent's reads as ended, and the project offers to resume it.
fn past_session(host: &Host, session: ExternalSession) -> MachineSession {
    let followed = session.agent == "claude";
    let status = match session.presence {
        _ if session.taken_over.is_some() => AgentStatus::Running {},
        _ if !followed => AgentStatus::Ended {},
        Presence::Working => AgentStatus::Working {},
        Presence::Waiting => AgentStatus::Waiting {},
        Presence::Running | Presence::Recent => AgentStatus::Running {},
        _ => AgentStatus::Ended {},
    };
    let unix = |seconds: u64| UNIX_EPOCH + Duration::from_secs(seconds);
    let external = session.taken_over.is_none();
    MachineSession {
        machine_id: host.id.clone(),
        machine_name: host.name.clone(),
        id: session.taken_over.unwrap_or(session.id),
        agent: session.agent,
        title: session
            .title
            .or_else(|| Some(session.last_prompt).filter(|prompt| !prompt.is_empty())),
        cwd: session.cwd,
        status,
        doing: None,
        started_at: unix(session.started_at),
        last_activity: unix(session.last_activity),
        external,
        question: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_recent_outside_session_offers_resume_unless_it_can_be_followed() {
        let host = Host {
            id: "m".into(),
            name: "build-box".into(),
        };
        let codex = ExternalSession {
            id: "s".into(),
            agent: "codex".into(),
            presence: Presence::Recent,
            ..ExternalSession::default()
        };
        assert_eq!(
            past_session(&host, codex.clone()).status,
            AgentStatus::Ended {}
        );
        let claude = ExternalSession {
            agent: "claude".into(),
            ..codex
        };
        assert_eq!(past_session(&host, claude).status, AgentStatus::Running {});
    }
}
