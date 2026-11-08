use super::*;
use crate::{
    protocol::MemorySource,
    store::{Scope, Validity},
};
use chrono::Duration;
use std::slice;

fn row(id: &str, text: &str) -> MemoryRow {
    MemoryRow {
        id: id.into(),
        content: text.into(),
        category: "people".into(),
        source: MemorySource::Learned {},
        updated_at: "2026-01-01 10:00:00".into(),
        valid: Validity::default(),
        origin: None,
        core: false,
        scope: Scope::User,
    }
}

fn score(row: &MemoryRow) -> Scored {
    Scored {
        row: row.clone(),
        score: 0.8,
    }
}

#[test]
fn repeated_recall_is_kept_once_until_compaction() {
    let today = NaiveDate::from_ymd_opt(2026, 1, 2).unwrap();
    let row = row("a", "Maya manages the design team.");
    let mut chat = Conversation::new();
    let first = select(vec![score(&row)], &[]);
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].role(), "developer");
    restore(&mut chat, &first, true);
    let json = serde_json::to_string(&chat).unwrap();
    let mut chat: Conversation = serde_json::from_str(&json).unwrap();
    let active = activate(&mut chat, slice::from_ref(&row), today);
    assert_eq!(active.len(), 1);
    assert!(select(vec![score(&row)], &active).is_empty());
    chat.strip_memories();
    restore(&mut chat, &active, true);
    assert_eq!(chat.entries().len(), 1);
    discard_saved(&mut chat);
    assert_eq!(select(vec![score(&row)], &[]).len(), 1);
}

#[test]
fn changed_forgotten_and_expired_memories_leave_context() {
    let today = NaiveDate::from_ymd_opt(2026, 1, 2).unwrap();
    let old = row("a", "Maya manages the design team.");
    let mut chat = Conversation::new();
    restore(&mut chat, &select(vec![score(&old)], &[]), true);
    let changed = row("a", "Maya manages the product team.");
    assert!(activate(&mut chat, slice::from_ref(&changed), today).is_empty());
    assert!(chat.entries().is_empty());
    let fresh = select(vec![score(&changed)], &[]);
    assert_eq!(fresh.len(), 1);
    restore(&mut chat, &fresh, true);
    assert!(activate(&mut chat, &[], today).is_empty());
    restore(&mut chat, &fresh, true);
    let mut expired = changed.clone();
    expired.valid.until = Some(NaiveDate::from_ymd_opt(2026, 1, 1).unwrap());
    assert!(activate(&mut chat, &[expired], today).is_empty());
}

#[test]
fn restored_memory_keeps_its_place_in_the_history() {
    let today = NaiveDate::from_ymd_opt(2026, 1, 2).unwrap();
    let row = row("a", "Maya manages the design team.");
    let memory = select(vec![score(&row)], &[]);
    let mut before = ConversationEntry::user("Earlier question");
    before.timestamp = memory[0].timestamp - Duration::seconds(1);
    let mut after = ConversationEntry::user("Later question");
    after.timestamp = memory[0].timestamp + Duration::seconds(1);
    let mut chat = Conversation::new();
    chat.add(before);
    restore(&mut chat, &memory, true);
    chat.add(after);
    let active = activate(&mut chat, &[row], today);
    chat.strip_memories();
    restore(&mut chat, &active, true);
    assert_eq!(chat.entries()[0].content(), "Earlier question");
    assert!(chat.entries()[1].content().contains("Maya manages"));
    assert_eq!(chat.entries()[2].content(), "Later question");
}
