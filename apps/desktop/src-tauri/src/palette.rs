//! The command center's Cmd+K search: ranks actions, chats, memories,
//! routines and machines against what was typed.

use crate::{
    beep::Beep, center::section, commands::MemoryView, error::Result, machines::MachineView,
};
use beep_proto::{Cron, ListCrons, ListMachines, ListMemories, Session, SessionMatch};
use serde::Serialize;
use std::{
    cmp::Reverse,
    collections::HashMap,
    sync::{Arc, Mutex, PoisonError},
};
use strum::IntoEnumIterator;
use tauri::State;
use tokio::join;

type Service<'a> = State<'a, Arc<Beep>>;

/// Most hits of one kind.
const PER_KIND: usize = 8;
/// Chats listed before anything is typed.
const RECENT_CHATS: usize = 5;
/// Chat text is searched from this many characters.
const MIN_TEXT_QUERY: usize = 2;

/// The lists the palette searches in memory. A list is `None` when the
/// server could not give it, which hides its group.
#[derive(Default)]
pub(crate) struct PaletteLists {
    memories: Option<Vec<MemoryView>>,
    routines: Option<Vec<Cron>>,
    machines: Option<Vec<MachineView>>,
}

/// The lists `center_palette` fetched last.
#[derive(Default)]
pub(crate) struct PaletteCache(Mutex<PaletteLists>);

/// Chats for one query: every chat (matched by title) and the chats whose
/// messages hold the words.
pub(crate) struct ChatHits {
    titles: Vec<Session>,
    text: Vec<SessionMatch>,
}

/// A view the palette can go to without a search.
#[derive(Clone, Copy, strum::EnumIter, strum::IntoStaticStr, Serialize)]
#[strum(serialize_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub(crate) enum PaletteAction {
    NewChat,
    Home,
    AllChats,
    Memories,
    Routines,
    Skills,
    Apps,
    Settings,
}

impl PaletteAction {
    fn title(self) -> &'static str {
        match self {
            Self::NewChat => "New chat",
            Self::Home => "Home",
            Self::AllChats => "All chats",
            Self::Memories => "Memories",
            Self::Routines => "Routines",
            Self::Skills => "Skills",
            Self::Apps => "Apps",
            Self::Settings => "Settings",
        }
    }
}

/// The groups of hits, in the order the palette lists them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum HitKind {
    Action,
    Chat,
    Memory,
    Routine,
    Machine,
}

#[derive(Debug, Serialize)]
pub(crate) struct Hit {
    kind: HitKind,
    id: String,
    title: String,
    detail: String,
}

/// How well a text matches, best first.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Tier {
    Prefix,
    WordStart,
    Substring,
    /// Only found in a chat's messages.
    Text,
}

/// `query` must be lowercase.
fn tier(query: &str, text: &str) -> Option<Tier> {
    let text = text.to_lowercase();
    if text.starts_with(query) {
        return Some(Tier::Prefix);
    }
    let at_word_start = |at: usize| {
        text[..at]
            .chars()
            .next_back()
            .is_none_or(|c| !c.is_alphanumeric())
    };
    let mut found = text.match_indices(query).map(|(at, _)| at).peekable();
    found.peek()?;
    Some(if found.any(at_word_start) {
        Tier::WordStart
    } else {
        Tier::Substring
    })
}

/// Keeps the best `PER_KIND` of the matching hits, ties in list order.
fn best(mut matched: Vec<(Tier, Hit)>) -> impl Iterator<Item = Hit> {
    matched.sort_by_key(|(tier, _)| *tier);
    matched.into_iter().map(|(_, hit)| hit).take(PER_KIND)
}

fn chat_hit(session: &Session, snippet: &str) -> Hit {
    Hit {
        kind: HitKind::Chat,
        id: session.id.clone(),
        title: session.title.clone(),
        detail: snippet.to_owned(),
    }
}

fn action_hit(action: PaletteAction) -> Hit {
    Hit {
        kind: HitKind::Action,
        id: <&'static str>::from(action).to_owned(),
        title: action.title().to_owned(),
        detail: String::new(),
    }
}

/// A chat found by title and by text is one hit, with the snippet.
fn chat_hits(query: &str, chats: &ChatHits) -> Vec<Hit> {
    struct Found<'a> {
        tier: Tier,
        session: &'a Session,
        snippet: &'a str,
    }
    let mut found: Vec<Found> = Vec::new();
    let mut at: HashMap<&str, usize> = HashMap::new();
    for session in &chats.titles {
        if let Some(tier) = tier(query, &session.title) {
            at.insert(&session.id, found.len());
            found.push(Found {
                tier,
                session,
                snippet: "",
            });
        }
    }
    for m in &chats.text {
        match at.get(m.session.id.as_str()) {
            Some(&i) => found[i].snippet = &m.snippet,
            None => {
                at.insert(&m.session.id, found.len());
                found.push(Found {
                    tier: Tier::Text,
                    session: &m.session,
                    snippet: &m.snippet,
                });
            }
        }
    }
    found.sort_by_key(|f| (f.tier, Reverse(f.session.updated_at)));
    found
        .into_iter()
        .take(PER_KIND)
        .map(|f| chat_hit(f.session, f.snippet))
        .collect()
}

/// Hits for `query`, a group at a time. Nothing typed lists every action
/// and the newest chats.
pub(crate) fn rank(query: &str, lists: &PaletteLists, chats: &ChatHits) -> Vec<Hit> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        let mut recent: Vec<&Session> = chats.titles.iter().collect();
        recent.sort_by_key(|s| Reverse(s.updated_at));
        return PaletteAction::iter()
            .map(action_hit)
            .chain(
                recent
                    .into_iter()
                    .take(RECENT_CHATS)
                    .map(|s| chat_hit(s, "")),
            )
            .collect();
    }
    let actions = PaletteAction::iter()
        .filter_map(|a| Some((tier(&query, a.title())?, action_hit(a))))
        .collect();
    let memories = lists.memories.iter().flatten().filter_map(|m| {
        let hit = Hit {
            kind: HitKind::Memory,
            id: m.id().to_owned(),
            title: m.content().to_owned(),
            detail: m.category().to_owned(),
        };
        Some((tier(&query, m.content())?, hit))
    });
    let routines = lists.routines.iter().flatten().filter_map(|r| {
        let hit = Hit {
            kind: HitKind::Routine,
            id: r.id.clone(),
            title: r.label.clone(),
            detail: r.schedule.clone(),
        };
        Some((tier(&query, &r.label)?, hit))
    });
    let machines = lists.machines.iter().flatten().filter_map(|m| {
        let hit = Hit {
            kind: HitKind::Machine,
            id: m.id().to_owned(),
            title: m.name().to_owned(),
            detail: if m.online() { "online" } else { "offline" }.to_owned(),
        };
        Some((tier(&query, m.name())?, hit))
    });
    best(actions)
        .chain(chat_hits(&query, chats))
        .chain(best(memories.collect()))
        .chain(best(routines.collect()))
        .chain(best(machines.collect()))
        .collect()
}

/// Fetches what the palette searches, when it opens. A list the server
/// cannot give hides its group.
#[tauri::command]
pub async fn center_palette(beep: Service<'_>, cache: State<'_, PaletteCache>) -> Result<()> {
    let (memories, routines, machines) = join!(
        beep.call(ListMemories {}),
        beep.call(ListCrons {}),
        beep.call(ListMachines {}),
    );
    let lists = PaletteLists {
        memories: section("memories", memories)
            .map(|all| all.into_iter().map(MemoryView::from).collect()),
        routines: section("routines", routines),
        machines: section("machines", machines)
            .map(|all| all.into_iter().map(MachineView::from).collect()),
    };
    *cache.0.lock().unwrap_or_else(PoisonError::into_inner) = lists;
    Ok(())
}

#[tauri::command]
pub async fn center_find(
    beep: Service<'_>,
    cache: State<'_, PaletteCache>,
    query: String,
) -> Result<Vec<Hit>> {
    let titles = section("chats", beep.sessions().await).unwrap_or_default();
    let text = if query.trim().chars().count() >= MIN_TEXT_QUERY {
        section("chat text", beep.search(query.trim().to_owned()).await).unwrap_or_default()
    } else {
        Vec::new()
    };
    let lists = cache.0.lock().unwrap_or_else(PoisonError::into_inner);
    Ok(rank(&query, &lists, &ChatHits { titles, text }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use beep_proto::{Machine, Memory, MemorySource};
    use std::time::SystemTime;

    fn routine(label: &str) -> Cron {
        Cron {
            id: label.to_owned(),
            label: label.to_owned(),
            prompt: String::new(),
            schedule: "0 8 * * *".to_owned(),
            enabled: true,
            created_at: String::new(),
            run_once: false,
            last_run_at: None,
            next_run_at: None,
        }
    }

    fn memory(content: &str) -> MemoryView {
        MemoryView::from(Memory {
            id: content.to_owned(),
            content: content.to_owned(),
            category: "facts".to_owned(),
            source: MemorySource::User {},
            updated: "2026-01-01".to_owned(),
            valid_from: None,
            valid_until: None,
            quote: None,
        })
    }

    fn machine(name: &str) -> MachineView {
        MachineView::from(Machine {
            id: name.to_owned(),
            name: name.to_owned(),
            online: true,
            seen_at: SystemTime::UNIX_EPOCH,
            version: String::new(),
            env: None,
        })
    }

    fn session(id: &str, title: &str, updated_at: i64) -> Session {
        Session {
            id: id.to_owned(),
            title: title.to_owned(),
            pinned: false,
            updated_at,
        }
    }

    fn none() -> ChatHits {
        ChatHits {
            titles: Vec::new(),
            text: Vec::new(),
        }
    }

    fn of_kind(hits: &[Hit], kind: HitKind) -> Vec<&Hit> {
        hits.iter().filter(|h| h.kind == kind).collect()
    }

    #[test]
    fn prefix_beats_substring() {
        let lists = PaletteLists {
            routines: Some(vec![routine("Morning brief"), routine("Brief review")]),
            ..PaletteLists::default()
        };
        let hits = rank("bri", &lists, &none());
        let routines = of_kind(&hits, HitKind::Routine);
        assert_eq!(routines[0].title, "Brief review");
        assert_eq!(routines[1].title, "Morning brief");
    }

    #[test]
    fn chat_found_twice_shows_once() {
        let found = session("a", "Trip plan", 1);
        let chats = ChatHits {
            titles: vec![found.clone()],
            text: vec![SessionMatch {
                session: found,
                snippet: "...a trip to Lisbon...".to_owned(),
                from_user: true,
            }],
        };
        let hits = rank("trip", &PaletteLists::default(), &chats);
        let chats = of_kind(&hits, HitKind::Chat);
        assert_eq!(chats.len(), 1);
        assert_eq!(chats[0].detail, "...a trip to Lisbon...");
    }

    #[test]
    fn caps_each_kind_at_eight() {
        let lists = PaletteLists {
            memories: Some((0..20).map(|i| memory(&format!("likes tea {i}"))).collect()),
            ..PaletteLists::default()
        };
        let hits = rank("tea", &lists, &none());
        assert_eq!(of_kind(&hits, HitKind::Memory).len(), 8);
    }

    #[test]
    fn empty_query_lists_actions_then_recent_chats() {
        let chats = ChatHits {
            titles: (0..7)
                .map(|i| session(&i.to_string(), &format!("chat {i}"), i))
                .collect(),
            text: Vec::new(),
        };
        let hits = rank("", &PaletteLists::default(), &chats);
        assert_eq!(hits.len(), 13);
        assert!(hits[..8].iter().all(|h| h.kind == HitKind::Action));
        let ids: Vec<&str> = hits[8..].iter().map(|h| h.id.as_str()).collect();
        assert_eq!(ids, ["6", "5", "4", "3", "2"]);
    }

    #[test]
    fn failed_lists_hide_their_group() {
        let chats = ChatHits {
            titles: vec![session("a", "Tea notes", 1)],
            text: Vec::new(),
        };
        let lists = PaletteLists {
            memories: None,
            machines: Some(vec![machine("tea-box")]),
            ..PaletteLists::default()
        };
        let hits = rank("tea", &lists, &chats);
        assert!(of_kind(&hits, HitKind::Memory).is_empty());
        assert_eq!(of_kind(&hits, HitKind::Chat).len(), 1);
        assert_eq!(of_kind(&hits, HitKind::Machine).len(), 1);
    }
}
