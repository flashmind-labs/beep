//! A coding-agent session as the apps show it.

use super::{
    super::{
        Reply,
        agent_sessions::{NODE_LIMIT, external_session, limited},
    },
    MAX_ENTRIES, live_host,
};
use crate::{api::router::Cx, protocol::*};
pub(super) use beep_mesh::machine::{beep_view, entries_for};
use std::time::{Duration, Instant};
use tokio::time::timeout;

/// A transcript that cannot be read leaves `entries` empty.
pub(in crate::api) async fn get_agent_session(
    cx: Cx,
    get: GetAgentSession,
) -> Reply<AgentSessionView> {
    let started = Instant::now();
    let live = live_host(&cx, &get.machine_id).await?;
    let host = live.host();
    let found_host = started.elapsed();
    let listed = limited(live.online.agent_sessions()).await?;
    let listed_at = started.elapsed();
    let mut view = match listed
        .into_iter()
        .find(|info| info.id == get.session && info.agent == get.agent)
    {
        Some(info) => beep_view(&host, info),
        None => {
            // A node that cannot list outside sessions has none to find.
            let outside = limited(live.online.external_sessions(
                &get.agent,
                1,
                Some(get.session.clone()),
            ))
            .await
            .unwrap_or_else(|e| {
                log::debug!("outside sessions not listed: {e:#}");
                Vec::new()
            });
            let found = outside
                .into_iter()
                .find(|session| session.id == get.session)
                .ok_or(ApiError::NotFound {})?;
            let last_message = found.last_message.clone();
            AgentSessionView {
                session: external_session(&host, found),
                plan: Vec::new(),
                files: Vec::new(),
                last_message,
                error: None,
                queued: 0,
                turns: 0,
                entries: Vec::new(),
                earlier: 0,
            }
        }
    };
    let last = get.last.clamp(1, MAX_ENTRIES);
    let read_at = started.elapsed();
    let thoughts = true;
    match timeout(
        NODE_LIMIT,
        live.online
            .transcript_before(&get.session, last, thoughts, get.before),
    )
    .await
    {
        Ok(Ok((entries, earlier))) => {
            view.entries = entries_for(entries, thoughts);
            view.earlier = earlier;
        }
        Ok(Err(e)) => log::warn!("transcript not read; session={} error={e:#}", get.session),
        Err(_) => log::warn!("transcript not read in time; session={}", get.session),
    }
    let took = started.elapsed();
    if took >= SLOW_READ {
        log::info!(
            "agent session read slowly; session={} host_ms={} list_ms={} outside_ms={} transcript_ms={}",
            get.session,
            found_host.as_millis(),
            (listed_at - found_host).as_millis(),
            (read_at - listed_at).as_millis(),
            (took - read_at).as_millis(),
        );
    }
    Ok(view)
}

/// A session read slower than this is logged with where the time went.
pub(super) const SLOW_READ: Duration = Duration::from_millis(500);

pub(in crate::api) async fn get_agent_session_usage(
    cx: Cx,
    get: GetAgentSessionUsage,
) -> Reply<AgentSessionUsage> {
    let live = live_host(&cx, &get.machine_id).await?;
    let info = limited(live.online.agent_sessions())
        .await?
        .into_iter()
        .find(|info| info.id == get.session && info.agent == get.agent)
        .ok_or(ApiError::NotFound {})?;
    Ok(AgentSessionUsage {
        context: info.context,
        tokens: info.tokens,
        cost_usd: info.cost_usd,
    })
}

pub(in crate::api) async fn get_agent_session_modes(
    cx: Cx,
    get: GetAgentSessionModes,
) -> Reply<AgentSessionModes> {
    let live = live_host(&cx, &get.machine_id).await?;
    let info = limited(live.online.agent_sessions())
        .await?
        .into_iter()
        .find(|info| info.id == get.session && info.agent == get.agent)
        .ok_or(ApiError::NotFound {})?;
    Ok(AgentSessionModes {
        current: info.mode,
        modes: info
            .modes
            .into_iter()
            .map(|mode| AgentMode {
                id: mode.id,
                name: mode.name,
            })
            .collect(),
        plan: info.plan_ready,
    })
}

pub(in crate::api) async fn get_agent_session_tasks(
    cx: Cx,
    get: GetAgentSessionTasks,
) -> Reply<AgentSessionTasks> {
    let live = live_host(&cx, &get.machine_id).await?;
    let info = limited(live.online.agent_sessions())
        .await?
        .into_iter()
        .find(|info| info.id == get.session && info.agent == get.agent)
        .ok_or(ApiError::NotFound {})?;
    Ok(beep_mesh::machine::session_tasks(info.state.tasks))
}
