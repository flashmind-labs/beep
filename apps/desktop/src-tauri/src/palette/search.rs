//! Cmd+K's search of what was said in sessions. Each node indexes its own
//! sessions; a phone asks every online machine, and a desktop searches the
//! copy it keeps.

use super::{index::Entry, lists::PaletteCache, projects::session_entry};
use crate::error::Result;
use beep_proto::{ImportedProject, MachineSession, node::agent::SearchHit};
use regex::Regex;
use std::{collections::HashMap, sync::LazyLock};
use tauri::{AppHandle, Manager};

/// The most sessions one search shows.
const MOST_HITS: usize = 64;

#[tauri::command]
pub async fn center_find_prompts(app: AppHandle, query: String) -> Result<Vec<Entry>> {
    let cache = app.state::<PaletteCache>();
    let sessions = cache.sessions();
    let listed = listed_by_machine(&sessions);

    #[cfg(mobile)]
    let hits = on_machines(&app, &query, &listed).await;
    #[cfg(desktop)]
    let hits = super::copies::find(&app, &query, listed).await?;

    Ok(entries(hits, &sessions, &cache.projects(), &query))
}

/// The ids of the sessions the menu lists, by machine. Only these are
/// searched, so a machine's limit of hits is not spent on others.
pub(super) type Listed = HashMap<String, Vec<String>>;

fn listed_by_machine(sessions: &[MachineSession]) -> Listed {
    let mut listed = Listed::new();

    for session in sessions {
        listed
            .entry(session.machine_id.clone())
            .or_default()
            .push(session.id.clone());
    }

    listed
}

/// Asks each online machine at once; one that is slow or cannot search is
/// left out.
#[cfg(mobile)]
async fn on_machines(app: &AppHandle, query: &str, listed: &Listed) -> Vec<Found> {
    use crate::fleet::Fleet;
    use beep_mesh::machine::sessions::{Reading, Sessions};
    use beep_proto::node::agent::{SessionsReply, SessionsRequest};
    use futures::{StreamExt, stream::FuturesUnordered};
    use std::{sync::Arc, time::Duration};

    const WAIT: Duration = Duration::from_millis(1500);
    let fleet = app.state::<Arc<Fleet>>();
    let mut asks: FuturesUnordered<_> = fleet
        .online()
        .into_iter()
        .filter_map(|machine| Some((machine, listed.get(&machine.to_string())?.clone())))
        .map(|(machine, sessions)| {
            let fleet = fleet.inner().clone();
            let request = SessionsRequest::Search {
                query: query.to_owned(),
                sessions,
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

/// A session a machine's index found.
pub(super) struct Found {
    /// The `Machine::id` it is on.
    pub machine: String,
    pub hit: SearchHit,
}

/// The hits of sessions the menu lists, newest activity first, each with
/// an excerpt. A hit of a session the menu does not list is left out.
fn entries(
    found: Vec<Found>,
    sessions: &[MachineSession],
    projects: &[ImportedProject],
    query: &str,
) -> Vec<Entry> {
    let listed: HashMap<(&str, &str), &MachineSession> = sessions
        .iter()
        .map(|session| ((session.machine_id.as_str(), session.id.as_str()), session))
        .collect();
    let mut shown: Vec<(&MachineSession, &Found)> = found
        .iter()
        .filter_map(|found| {
            let session = listed.get(&(found.machine.as_str(), found.hit.session.as_str()))?;
            Some((*session, found))
        })
        .collect();
    // The listed activity is the newest; an index's may be older.
    shown.sort_by_key(|(session, _)| std::cmp::Reverse(session.last_activity));

    shown
        .into_iter()
        .take(MOST_HITS)
        .map(|(session, found)| {
            let mut entry = session_entry(session, projects);
            entry.snippet = snippet(&found.hit.texts, query);
            entry
        })
        .collect()
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
    let folded = text.to_lowercase();
    let (folded_at, term) = terms
        .iter()
        .filter_map(|term| folded.find(term.as_str()).map(|at| (at, term)))
        .min()?;

    let chars: Vec<char> = text.chars().collect();
    let mut bytes = 0;
    let at = chars
        .iter()
        .take_while(|ch| {
            let before = bytes;
            bytes += ch.to_lowercase().map(char::len_utf8).sum::<usize>();
            before < folded_at
        })
        .count();
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
