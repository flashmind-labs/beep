//! A coding-agent session as the apps show it.

use super::{
    super::{
        Reply,
        agent_sessions::{Host, NODE_LIMIT, beep_session, external_session, limited},
    },
    MAX_ENTRIES, live_host,
};
use crate::{
    api::router::Cx,
    protocol::{
        node::agent::{self, PlanStatus, SessionInfo},
        *,
    },
};
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
    let thoughts = cx.supports(Feature::THOUGHTS);
    match timeout(
        NODE_LIMIT,
        live.online.transcript(&get.session, last, thoughts),
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

pub(super) fn beep_view(host: &Host, info: SessionInfo) -> AgentSessionView {
    let state = info.state.clone();
    let last_message = info.last_message.clone();
    let turns = info.turns;
    AgentSessionView {
        session: beep_session(host, info),
        plan: state
            .plan
            .into_iter()
            .map(|step| AgentPlanStep {
                content: step.content,
                status: plan_status(step.status),
            })
            .collect(),
        files: state.files_touched,
        last_message,
        error: state.error,
        queued: state.queued,
        turns,
        entries: Vec::new(),
        earlier: 0,
    }
}

/// A status from a newer node reads as pending.
pub(super) fn plan_status(status: PlanStatus) -> AgentPlanStatus {
    match status {
        PlanStatus::InProgress => AgentPlanStatus::InProgress {},
        PlanStatus::Completed => AgentPlanStatus::Completed {},
        _ => AgentPlanStatus::Pending {},
    }
}

/// The entries an app reads: thoughts only for one that reads them.
pub(super) fn entries_for(entries: Vec<agent::Entry>, thoughts: bool) -> Vec<AgentEntry> {
    entries
        .into_iter()
        .filter(|entry| thoughts || entry.kind != agent::EntryKind::Thought)
        .map(entry)
        .collect()
}

pub(super) fn entry(entry: agent::Entry) -> AgentEntry {
    let kind = match entry.kind {
        agent::EntryKind::User => AgentEntryKind::User {},
        agent::EntryKind::Agent => AgentEntryKind::Agent {},
        agent::EntryKind::Tool => AgentEntryKind::Tool {},
        agent::EntryKind::Question => AgentEntryKind::Question {},
        agent::EntryKind::Thought => AgentEntryKind::Thought { ms: entry.ms },
        _ => AgentEntryKind::Other {},
    };
    AgentEntry {
        kind,
        text: entry.text,
    }
}
