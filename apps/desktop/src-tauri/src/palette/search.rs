//! Cmd+K's search of what was said in sessions. Each node indexes its own
//! sessions; a phone asks every online machine, and a desktop searches the
//! copy it keeps.

use super::{
    lists::PaletteCache,
    projects::{Entry, Found, session_entry},
};
use crate::{error::Result, fleet::Fleet};
use beep_proto::{AgentStatus, ImportedProject, MachineSession, node::agent::folded_at};
use regex::Regex;
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, LazyLock},
    time::{Duration, UNIX_EPOCH},
};
use tauri::{AppHandle, Manager};

/// The most sessions one search shows.
const MOST_HITS: usize = 64;

#[tauri::command]
pub async fn center_find_prompts(app: AppHandle, query: String) -> Result<Vec<Entry>> {
    let cache = app.state::<PaletteCache>();
    let sessions = cache.sessions(&app);
    let fleet = app.state::<Arc<Fleet>>();
    let machines: HashMap<_, _> = match fleet.mesh() {
        Some(mesh) => mesh
            .code_hosts()
            .into_iter()
            .map(|host| (host.device.id.to_string(), host.device.name))
            .collect(),
        None => cache.machine_names(),
    };

    #[cfg(mobile)]
    let hits = on_machines(&fleet, &query).await;
    #[cfg(desktop)]
    let hits = super::copies::find(&app, &query).await?;

    Ok(entries(
        hits,
        &sessions,
        &cache.projects(),
        &machines,
        &query,
    ))
}

/// Asks each online machine at once; one that is slow or cannot search is
/// left out.
#[cfg(mobile)]
async fn on_machines(fleet: &Arc<Fleet>, query: &str) -> Vec<Found> {
    use beep_mesh::machine::sessions::{Reading, Sessions};
    use beep_proto::node::agent::{SessionsReply, SessionsRequest};
    use futures::{StreamExt, stream::FuturesUnordered};

    const WAIT: Duration = Duration::from_millis(1500);
    let mut asks: FuturesUnordered<_> = fleet
        .online()
        .into_iter()
        .map(|machine| {
            let request = SessionsRequest::Search {
                query: query.to_owned(),
                sessions: Vec::new(),
            };
            async move {
                let reply = tokio::time::timeout(WAIT, async {
                    let client = fleet.client(machine).await?;
                    Ok::<_, anyhow::Error>(client.sessions(request, Reading::Once, WAIT).await?)
                })
                .await;
                (machine, reply)
            }
        })
        .collect();
    let mut found = Vec::new();

    while let Some((machine, reply)) = asks.next().await {
        match reply {
            Ok(Ok(SessionsReply::SearchHits { hits })) => {
                found.extend(hits.into_iter().map(|hit| Found {
                    machine: machine.to_string(),
                    hit,
                }))
            }
            Ok(Ok(other)) => log::debug!("{machine} did not search: {other:?}"),
            Ok(Err(e)) => log::debug!("{machine} did not search: {e:#}"),
            Err(_) => log::debug!("{machine} did not search in time"),
        }
    }

    found
}

/// The hits of current mesh machines, newest activity first. Live metadata
/// takes precedence when the app already has it.
fn entries(
    found: Vec<Found>,
    sessions: &[MachineSession],
    projects: &[ImportedProject],
    machines: &HashMap<String, String>,
    query: &str,
) -> Vec<Entry> {
    let listed: HashMap<(&str, &str), &MachineSession> = sessions
        .iter()
        .map(|session| ((session.machine_id.as_str(), session.id.as_ref()), session))
        .collect();
    let mut shown: Vec<(MachineSession, &Found)> = found
        .iter()
        .filter_map(|found| {
            let machine_name = machines.get(&found.machine)?;
            let hit = &found.hit;
            let id = hit
                .metadata
                .as_ref()
                .and_then(|meta| meta.managed.as_ref())
                .unwrap_or(&hit.session);
            let session = match listed.get(&(found.machine.as_str(), id.as_ref())) {
                Some(session) => (*session).clone(),
                None => indexed_session(found, machine_name)?,
            };
            Some((session, found))
        })
        .collect();
    // The listed activity is the newest; an index's may be older.
    shown.sort_by_key(|(session, _)| std::cmp::Reverse(session.last_activity));

    let mut seen = HashSet::new();
    shown
        .into_iter()
        .filter_map(|(session, found)| {
            if !seen.insert((session.machine_id.clone(), session.id.clone())) {
                return None;
            }

            let mut entry = session_entry(&session, projects);
            entry.snippet = snippet(&found.hit.texts, query);
            Some(entry)
        })
        .take(MOST_HITS)
        .collect()
}

fn indexed_session(found: &Found, machine_name: &str) -> Option<MachineSession> {
    let hit = &found.hit;
    let metadata = hit.metadata.as_ref()?;
    Some(MachineSession {
        machine_id: found.machine.clone(),
        machine_name: machine_name.to_owned(),
        id: metadata
            .managed
            .clone()
            .unwrap_or_else(|| hit.session.clone()),
        agent: metadata.agent.clone(),
        title: metadata.title.clone(),
        cwd: metadata.cwd.clone(),
        status: AgentStatus::Ended {},
        doing: None,
        started_at: UNIX_EPOCH.checked_add(Duration::from_secs(metadata.started_at))?,
        last_activity: UNIX_EPOCH.checked_add(Duration::from_millis(hit.activity))?,
        external: metadata.managed.is_none(),
        question: None,
        subagents: 0,
        model: None,
        assistant: false,
        reply: None,
        changes: None,
        account: None,
        persona: None,
        place: None,
        continues: None,
    })
}

/// One line from the first text that holds a query word, without the files
/// it mentions.
fn snippet(texts: &[String], query: &str) -> Option<String> {
    let terms: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();

    texts
        .iter()
        .find_map(|text| excerpt(&MENTION.replace_all(text, " "), &terms))
}

/// A file mention as `@path` or `@"path with spaces"` writes it.
static MENTION: LazyLock<Regex> = LazyLock::new(|| {
    // A constant pattern, checked by the tests.
    Regex::new(r#"@"[^"]*"|@\S*[/\\]\S*"#).expect("valid mention pattern")
});

/// A short plain-text excerpt from just before the earliest query word, with
/// Unicode offsets measured in characters so case folding cannot split a
/// UTF-8 character.
fn excerpt(text: &str, terms: &[String]) -> Option<String> {
    let (at, term) = terms
        .iter()
        .filter_map(|term| folded_at(text, term).map(|at| (at, term)))
        .min()?;

    let chars: Vec<char> = text.chars().collect();
    let start = at.saturating_sub(24);
    let end = (at + term.chars().count() + 120).min(chars.len());
    let excerpt: String = chars[start.min(chars.len())..end].iter().collect();

    Some(format!(
        "{}{}{}",
        if start > 0 { "…" } else { "" },
        excerpt.split_whitespace().collect::<Vec<_>>().join(" "),
        if end < chars.len() { "…" } else { "" }
    ))
}

#[cfg(test)]
#[path = "../tests/palette_search.rs"]
mod tests;
