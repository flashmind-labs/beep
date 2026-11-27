//! Keeps the search index up to date: reads what the agents' stores and
//! sessions hold, one session at a time, while the mesh is open.

use super::{
    IN_FLIGHT, MOST_TEXT, PAGE, Page, Pass, SESSION_WAIT, SearchIndex, advance,
    schedule::{Conversation, Pacer},
    sooner,
    store::Indexed,
};
use crate::agents::Agents;
use beep_mesh::Mesh;
use beep_proto::{
    SessionId,
    node::agent::{EntryKind, SearchSession, SessionInfo},
};
use futures::{StreamExt, stream};
use std::{
    cmp::Reverse,
    collections::{HashMap, HashSet},
    sync::Arc,
};
use tokio::{
    sync::broadcast,
    task::spawn_blocking,
    time::{Instant, timeout},
};

/// Opens the index if the node has none yet, and keeps it up to date while
/// the mesh is open.
pub(crate) async fn follow(mesh: Mesh, agents: Arc<Agents>) {
    let Some(index) = agents.search.open().await else {
        return;
    };
    // Held for the node's life: its watch rings `agent` when a store changes.
    let _stores = match agents.watch_event_sources().await {
        Ok(sources) => Some(sources),
        Err(e) => {
            log::warn!("the agents' stores are not watched for search: {e:#}");
            None
        }
    };
    let mut pacer = Pacer::default();
    let mut wake = None;
    let wakes = wake_on_changes(&index, &agents, &mesh);
    tokio::pin!(wakes);

    loop {
        tokio::select! {
            () = pacer.next_pass(&index.changed, wake) => {}
            () = &mut wakes => return,
        }

        let sessions = conversations(&agents).await;
        let (due, next) = index.due(sessions);
        wake = next;
        let mut work = stream::iter(due)
            .map(|session| {
                let index = index.clone();
                let agents = agents.clone();
                async move {
                    let mut saved = false;
                    let read = timeout(
                        SESSION_WAIT,
                        index_new(&agents, &index, &session, &mut saved),
                    )
                    .await;
                    let pass = match read {
                        Ok(Ok(true)) => Pass::Indexed,
                        Ok(Ok(false)) => Pass::Current,
                        Ok(Err(e)) => {
                            log::debug!("indexing session {}: {e:#}", session.id);
                            Pass::Failed
                        }
                        Err(_) if saved => Pass::Partial,
                        Err(_) => {
                            log::debug!("indexing session {} saved nothing in time", session.id);
                            Pass::Failed
                        }
                    };
                    (session, pass)
                }
            })
            .buffer_unordered(IN_FLIGHT);

        while let Some((session, pass)) = work.next().await {
            match pass {
                Pass::Current | Pass::Indexed => index.schedule().indexed(session),
                Pass::Partial => {
                    index.schedule().progressed(&session.id);
                    wake = sooner(wake, Instant::now());
                }
                Pass::Failed => {
                    let at = index.schedule().failed(session.id, Instant::now());
                    wake = sooner(wake, at);
                }
            }
        }

        index.added().await;
    }
}

/// Whether a change under this topic name can mean new conversation text:
/// a store written (`agent`), the session list, or everything.
pub(super) fn reads_stores(topic: &str) -> bool {
    matches!(topic, "agent" | "agents" | "*")
}

/// Wakes a pass on session or mesh changes. Ends when the node's events
/// or the mesh close.
async fn wake_on_changes(index: &SearchIndex, agents: &Agents, mesh: &Mesh) {
    let mut events = agents.events.subscribe();
    let mut updates = mesh.updates();

    loop {
        tokio::select! {
            event = events.recv() => match event {
                Ok(change) if !reads_stores(change.topic.name()) => continue,
                Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => {}
                Err(broadcast::error::RecvError::Closed) => return,
            },
            changed = updates.changed() => {
                if changed.is_err() {
                    return;
                }
            }
        }

        index.changed.notify_one();
    }
}

/// Keep the agent's id as the index key so taking over a conversation
/// changes how it opens without creating another copy of its messages.
pub(super) async fn conversations(agents: &Agents) -> Vec<Conversation> {
    let stores = agents.listings.stores().clone();
    let archived = spawn_blocking(move || stores.archived_codex());
    let (sightings, archived) = tokio::join!(agents.listings.sightings(), archived);
    let mut seen = match sightings {
        Ok(seen) => seen,
        Err(e) => {
            log::warn!("listing sessions to index: {e:#}");
            Vec::new()
        }
    };
    match archived {
        Ok(archived) => seen.extend(archived),
        Err(e) => log::warn!("listing archived sessions to index: {e:#}"),
    }

    let owned = agents.sessions.owned();
    let mut live: HashMap<SessionId, SessionInfo> = agents
        .sessions.listed(true)
        .into_iter()
        .map(|info| (info.id.clone(), info))
        .collect();
    let mut sessions = Vec::new();

    for seen in seen {
        let managed = owned
            .get(&seen.id)
            .and_then(|id| live.remove(id))
            .or_else(|| live.remove(&seen.id));
        let last_activity = managed.as_ref().map_or(seen.last_activity, |info| {
            info.last_activity.max(seen.last_activity)
        });
        let metadata = SearchSession {
            agent: seen.agent.to_string(),
            cwd: seen.cwd.to_string_lossy().into_owned(),
            title: managed
                .as_ref()
                .and_then(|info| info.title.clone())
                .or(seen.title),
            started_at: seen.started_at,
            managed: managed.map(|info| info.id),
        };
        sessions.push(Conversation {
            id: seen.id,
            last_activity,
            metadata,
        });
    }

    sessions.extend(live.into_values().map(|info| Conversation {
        id: info.id.clone(),
        last_activity: info.last_activity.max(info.started_at),
        metadata: SearchSession {
            agent: info.agent,
            cwd: info.cwd,
            title: info.title,
            started_at: info.started_at,
            managed: Some(info.id),
        },
    }));
    sessions.sort_by_key(|session| Reverse(session.last_activity));
    let mut known = HashSet::new();
    sessions.retain(|session| known.insert(session.id.clone()));
    sessions
}

/// Reads the entries of `session` added since it was last indexed, newest
/// page first, and saves each page as it arrives, setting `saved`. False
/// when it was already current.
pub(super) async fn index_new(
    agents: &Agents,
    index: &Arc<SearchIndex>,
    session: &Conversation,
    saved: &mut bool,
) -> anyhow::Result<bool> {
    let stamp = session.last_activity.to_string();
    let activity = session.last_activity.saturating_mul(1000);
    let store = index.clone();
    let (id, at) = (session.id.clone(), stamp.clone());
    let metadata = session.metadata.clone();
    let prepared = spawn_blocking(move || -> anyhow::Result<_> {
        let mut held = store.store();
        if crate::lock(&store.deleted).contains(&id) {
            return Ok((true, false));
        }

        let changed = held.set_metadata(&id, activity, &metadata)?;
        Ok((held.current(&id, &at)?, changed))
    })
    .await??;

    let (current, changed) = prepared;
    if changed {
        index.added().await;
    }
    if current {
        return Ok(false);
    }

    let store = index.clone();
    let id = session.id.clone();
    let mut progress = spawn_blocking(move || store.store().progress(&id)).await??;
    // A walk left by an earlier read finishes first; the newest page is
    // read after it.
    let mut newest = false;

    loop {
        let before = progress.walk.map(|walk| walk.low);
        let (texts, page) = read_page(agents, &session.id, before).await?;
        newest |= before.is_none();
        progress = advance(progress, page);
        let current = newest && progress.walk.is_none();
        let indexed = Indexed {
            id: session.id.clone(),
            activity,
            stamp: current.then(|| stamp.clone()),
        };

        let held = index.clone();
        let kept = spawn_blocking(move || {
            let mut store = held.store();
            // Checked with the store held, so a delete is wholly before or
            // after this page.
            let deleted = crate::lock(&held.deleted).contains(&indexed.id);
            (!deleted)
                .then(|| store.save(&indexed, &texts, progress))
                .transpose()
        })
        .await??;

        if kept.is_none() {
            return Ok(false);
        }

        *saved = true;
        index.added().await;

        if current {
            return Ok(true);
        }
    }
}

/// The user's prompts and the agent's replies on the page before entry
/// `before` (the newest page when `None`).
async fn read_page(
    agents: &Agents,
    session: &SessionId,
    before: Option<u64>,
) -> anyhow::Result<(Vec<String>, Page)> {
    let Some((entries, earlier)) = agents.read_transcript(session, PAGE, before, false).await?
    else {
        anyhow::bail!("the session is gone");
    };
    let page = Page {
        earlier,
        entries: entries.len() as u64,
    };
    let texts = entries
        .into_iter()
        .filter_map(|entry| match entry.kind {
            EntryKind::User => Some(crate::tags::own_words(&entry.text)),
            EntryKind::Agent => Some(entry.text),
            _ => None,
        })
        .filter(|text| !text.trim().is_empty())
        .map(|mut text| {
            text.truncate(text.floor_char_boundary(MOST_TEXT));
            text
        })
        .collect();

    Ok((texts, page))
}
