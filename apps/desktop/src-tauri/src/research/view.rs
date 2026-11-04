//! A research as the window draws it: the team's agents, entries, sources
//! and paper, cached here so a window asks only for newer entries.

use crate::{beep::Service, chat::markdown, error::Result};
use beep_proto::{GetResearch, Research, ResearchEntryBody};
use serde::Serialize;
use std::{
    collections::HashMap,
    sync::{Mutex, PoisonError},
};
use tauri::State;
use uuid::Uuid;

/// What this Mac last heard of each research, every entry included, so a
/// window opens from it and asks the server only for entries it lacks.
#[derive(Default)]
pub struct Cache(Mutex<HashMap<Uuid, Cached>>);

struct Cached {
    research: Research,
    /// `research.draft` rendered, kept until the draft changes.
    draft_html: String,
}

impl Cache {
    /// The newest entry held for `session`, or 0 for none.
    fn last_seq(&self, session: Uuid) -> i64 {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&session)
            .and_then(|cached| cached.research.entries.last())
            .map_or(0, |entry| entry.seq)
    }

    /// Adds `fresh`, fetched after the newest entry held, and returns the
    /// research with the entries after `after`.
    fn merge(&self, session: Uuid, mut fresh: Research, after: i64) -> ResearchView {
        let mut all = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        let cached = match all.remove(&session) {
            Some(mut old) => {
                old.research.entries.append(&mut fresh.entries);
                fresh.entries = old.research.entries;
                let draft_html = if old.research.draft == fresh.draft {
                    old.draft_html
                } else {
                    markdown::to_html(&fresh.draft)
                };
                Cached {
                    research: fresh,
                    draft_html,
                }
            }
            None => Cached {
                draft_html: markdown::to_html(&fresh.draft),
                research: fresh,
            },
        };
        let view = view(&cached, after);
        all.insert(session, cached);
        view
    }

    fn view(&self, session: Uuid) -> Option<ResearchView> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&session)
            .map(|cached| view(cached, 0))
    }
}

/// A research as the window draws it.
#[derive(Serialize)]
pub struct ResearchView {
    topic: String,
    status: &'static str,
    agents: Vec<AgentView>,
    entries: Vec<EntryView>,
    sources: Vec<SourceView>,
    /// The paper as Markdown, for copying and export.
    draft: String,
    /// The paper as sanitized HTML.
    draft_html: String,
    started_at: i64,
    finished_at: Option<i64>,
    /// The newest round, from 1.
    round: u32,
}

#[derive(Serialize)]
struct AgentView {
    id: String,
    name: String,
    role: String,
    model: String,
    /// A scout's round; 0 for agents in every round.
    round: u32,
}

#[derive(Serialize)]
struct SourceView {
    number: u32,
    url: String,
    title: String,
    quote: String,
    agent: String,
}

#[derive(Serialize)]
struct EntryView {
    seq: i64,
    agent: String,
    at: i64,
    #[serde(flatten)]
    body: EntryBody,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
enum EntryBody {
    Said { to: Option<String>, content: String },
    Step { label: String, success: bool },
    Note { content: String, sources: Vec<u32> },
}

impl From<ResearchEntryBody> for EntryBody {
    fn from(body: ResearchEntryBody) -> Self {
        match body {
            ResearchEntryBody::Said { to, content } => Self::Said { to, content },
            ResearchEntryBody::Step { label, success } => Self::Step { label, success },
            ResearchEntryBody::Note { content, sources } => Self::Note { content, sources },
        }
    }
}

/// The research in `cached` with its entries after `after`.
fn view(cached: &Cached, after: i64) -> ResearchView {
    let research = &cached.research;
    ResearchView {
        topic: research.topic.clone(),
        status: research.status.as_str(),
        agents: research
            .agents
            .iter()
            .map(|agent| AgentView {
                id: agent.id.clone(),
                name: agent.name.clone(),
                role: agent.role.clone(),
                model: agent.model.clone(),
                round: agent.round,
            })
            .collect(),
        entries: research
            .entries
            .iter()
            .filter(|entry| entry.seq > after)
            .map(|entry| EntryView {
                seq: entry.seq,
                agent: entry.agent.clone(),
                at: entry.timestamp,
                body: entry.body.clone().into(),
            })
            .collect(),
        sources: research
            .sources
            .iter()
            .map(|source| SourceView {
                number: source.number,
                url: source.url.clone(),
                title: source.title.clone(),
                quote: source.quote.clone(),
                agent: source.agent.clone(),
            })
            .collect(),
        draft: research.draft.clone(),
        draft_html: cached.draft_html.clone(),
        started_at: research.started_at,
        finished_at: research.finished_at,
        round: research.round,
    }
}

/// What the team did, from entry `after` on. `None` until the server has
/// made the chat a research.
#[tauri::command]
pub async fn get_research(
    beep: Service<'_>,
    cache: State<'_, Cache>,
    session: Uuid,
    after: i64,
) -> Result<Option<ResearchView>> {
    let research = beep
        .call(GetResearch {
            session_id: session,
            after: cache.last_seq(session),
        })
        .await?;
    Ok(research.map(|fresh| cache.merge(session, fresh, after)))
}

/// The research as last fetched, for a window to draw before it asks.
#[tauri::command]
pub fn cached_research(cache: State<'_, Cache>, session: Uuid) -> Option<ResearchView> {
    cache.view(session)
}

#[cfg(test)]
mod tests {
    use super::*;
    use beep_proto::{ResearchEntry, ResearchStatus};

    fn research(seqs: &[i64], draft: &str) -> Research {
        Research {
            id: Uuid::nil(),
            topic: "Solid-state batteries".into(),
            status: ResearchStatus::Researching {},
            agents: Vec::new(),
            entries: seqs
                .iter()
                .map(|&seq| ResearchEntry {
                    seq,
                    agent: "a".into(),
                    body: ResearchEntryBody::Said {
                        to: None,
                        content: format!("entry {seq}"),
                    },
                    timestamp: seq,
                })
                .collect(),
            sources: Vec::new(),
            draft: draft.into(),
            started_at: 0,
            finished_at: None,
            round: 1,
        }
    }

    fn seqs(view: &ResearchView) -> Vec<i64> {
        view.entries.iter().map(|entry| entry.seq).collect()
    }

    #[test]
    fn keeps_every_entry_and_asks_only_for_newer_ones() {
        let cache = Cache::default();
        let session = Uuid::new_v4();
        assert_eq!(cache.last_seq(session), 0);

        let first = cache.merge(session, research(&[1, 2], ""), 0);
        assert_eq!(seqs(&first), [1, 2]);
        assert_eq!(cache.last_seq(session), 2);

        // The next poll brings only entry 3; the window already has 1 and 2.
        let next = cache.merge(session, research(&[3], "# Paper"), 2);
        assert_eq!(seqs(&next), [3]);
        assert!(next.draft_html.contains("Paper"));

        // A window opened later gets all of it without asking.
        let reopened = cache.view(session).expect("the research was cached");
        assert_eq!(seqs(&reopened), [1, 2, 3]);
        assert_eq!(reopened.draft, "# Paper");
    }
}
