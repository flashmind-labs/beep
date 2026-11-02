//! What the user's coding agents left on their machines: the git checkouts
//! they worked in, to suggest as projects, and the sessions that ran in an
//! imported project's folders.

use super::{
    Reply,
    agent_control::{Live, live_hosts},
    agent_sessions::{Host, IN_FLIGHT},
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
    time::{Duration, UNIX_EPOCH},
};

/// The most checkouts one machine names.
const FOLDERS_PER_MACHINE: u32 = 100;
/// The most sessions a project lists.
const MAX_SESSIONS: usize = 50;

/// Admins only, like `ListMachines`. A machine that cannot say is left out.
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

/// Admins only. `NotFound` unless the caller imported `project`.
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
    match live.online.sessions_in(folders, limit).await {
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

/// A session as a project lists it: one nothing runs reads as ended, and
/// one Beep took over as running.
fn past_session(host: &Host, session: ExternalSession) -> MachineSession {
    let status = match session.presence {
        _ if session.taken_over.is_some() => AgentStatus::Running {},
        Presence::Working => AgentStatus::Working {},
        Presence::Waiting => AgentStatus::Waiting {},
        Presence::Running | Presence::Recent => AgentStatus::Running {},
        _ => AgentStatus::Ended {},
    };
    let unix = |seconds: u64| UNIX_EPOCH + Duration::from_secs(seconds);
    MachineSession {
        machine_id: host.id.clone(),
        machine_name: host.name.clone(),
        id: session.id,
        agent: session.agent,
        title: session
            .title
            .or_else(|| Some(session.last_prompt).filter(|prompt| !prompt.is_empty())),
        cwd: session.cwd,
        status,
        doing: None,
        started_at: unix(session.started_at),
        last_activity: unix(session.last_activity),
        external: true,
        question: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stopped_session_reads_as_ended_with_its_prompt_for_a_title() {
        let host = Host {
            id: "m".into(),
            name: "build-box".into(),
        };
        let session = ExternalSession {
            id: "s".into(),
            agent: "codex".into(),
            cwd: "/home/user/proj".into(),
            presence: Presence::Stopped,
            last_prompt: "Fix the parser".into(),
            last_activity: 10,
            ..ExternalSession::default()
        };
        let listed = past_session(&host, session.clone());
        assert_eq!(listed.status, AgentStatus::Ended {});
        assert_eq!(listed.title.as_deref(), Some("Fix the parser"));
        assert!(listed.external);
        let taken = ExternalSession {
            taken_over: Some("a3f9c1".into()),
            ..session
        };
        assert_eq!(past_session(&host, taken).status, AgentStatus::Running {});
    }
}
