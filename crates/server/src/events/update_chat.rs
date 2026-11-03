//! A chat about one update from the Updates list. It opens with Beep telling
//! the user the update, and keeps the update's events in what the agent
//! reads, so Beep answers the user's reply knowing what came in.

use super::pipeline::{clipped, write_event};
use crate::{
    feeds::mail::clip,
    store::{UpdateChatSeed, UpdateDetail},
    user::User,
};
use anyhow::Result;
use flashmind::core::{Conversation, ConversationEntry};
use std::fmt::Write;
use uuid::Uuid;

/// How much of each event's source record the agent reads, in characters.
const RAW_CHARS: usize = 4000;

/// The chat about the user's update `id`, made on the first ask. `None`
/// when the user has no such update.
pub async fn open(user: &User, id: i64) -> Result<Option<Uuid>> {
    let Some(detail) = user.update_detail(id).await? else {
        return Ok(None);
    };
    let message = detail.update.summary.clone().unwrap_or_default();
    let title = clipped(&message);
    let conversation = conversation(&detail, &message);
    let seed = UpdateChatSeed {
        title: &title,
        conversation: &conversation,
        message: &message,
    };
    Ok(user
        .open_update_chat(id, &seed)
        .await?
        .map(|opened| opened.chat))
}

/// What the agent reads: the update and its events, the user opening it,
/// and Beep's `message` telling them.
fn conversation(detail: &UpdateDetail, message: &str) -> Conversation {
    let mut chat = Conversation::new();
    chat.add(ConversationEntry::system_message(context(detail)));
    chat.add(ConversationEntry::user(match app_name(detail) {
        Some(app) => format!("I opened this update from {app}."),
        None => "I opened this update.".to_owned(),
    }));
    chat.add(ConversationEntry::assistant(message));
    chat
}

fn app_name(detail: &UpdateDetail) -> Option<&'static str> {
    detail.events.first().map(|event| event.row.app.name())
}

/// The update as the triage agent passed it on, its events with their ids,
/// and the records the apps sent.
fn context(detail: &UpdateDetail) -> String {
    let mut text = format!(
        "This chat is about an update from the user's apps, which they opened from their \
         Updates list. Your event triage agent passed it on because: {}\n\
         Your first message told them about it. Answer their reply with this in mind; use \
         the app's read tools with the ids below to look further.",
        detail.update.reason
    );
    if detail.events.is_empty() {
        text.push_str("\n\nIts events are no longer stored.");
        return text;
    }
    text.push_str("\n\nThe events it covers:");
    for event in &detail.events {
        write_event(&mut text, &event.row);
        if !event.raw.is_null() {
            let raw = event.raw.to_string();
            let cut = clip(&raw, RAW_CHARS);
            let more = if cut.len() < raw.len() { " (cut)" } else { "" };
            let _ = write!(text, "\nThe record the app sent{more}: {cut}");
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        events::{AppEvent, DedupeKey, EventApp, EventKind, Source, Urgency, Verdict},
        store::{
            EventRow, EventState, NewTriage, Store, StoredBody, TestDb, TriageRow, UpdateEvent,
        },
        user::UserKey,
    };
    use chrono::Utc;
    use flashmind::core::EntryKind;
    use serde_json::json;

    fn detail(events: Vec<UpdateEvent>) -> UpdateDetail {
        UpdateDetail {
            update: TriageRow {
                id: 7,
                decided_at: Utc::now(),
                verdict: Verdict::Inform,
                urgency: None,
                summary: Some("Ana asked you for a review.".to_owned()),
                reason: "A person is waiting.".to_owned(),
                refs: vec![3],
                delivered: false,
            },
            events,
        }
    }

    fn mail() -> UpdateEvent {
        UpdateEvent {
            row: EventRow {
                id: 3,
                app: EventApp::Gmail,
                kind: EventKind::NewMail,
                summary: "From Ana: Review please".to_owned(),
                detail: None,
                ids: json!({"message_id": "m1"}),
                occurred: Utc::now(),
                state: EventState::Triaged,
            },
            raw: json!({"id": "m1", "subject": "Review please"}),
        }
    }

    #[test]
    fn the_agent_reads_the_events_and_beep_speaks_last() {
        let chat = conversation(&detail(vec![mail()]), "Ana asked you for a review.");
        let entries = chat.entries();
        assert_eq!(entries.len(), 3);
        let context = entries[0].content();
        assert!(context.contains("A person is waiting."));
        assert!(context.contains("[#3] Gmail"));
        assert!(context.contains(r#""subject":"Review please""#));
        assert_eq!(entries[1].content(), "I opened this update from Gmail.");
        assert!(matches!(entries[2].kind, EntryKind::Assistant { .. }));
        assert_eq!(entries[2].content(), "Ana asked you for a review.");
    }

    /// An update for `user` about one new mail, as the triage agent logs it.
    async fn inform(store: &Store, user: &UserKey, summary: &str) -> i64 {
        let event = AppEvent {
            app: EventApp::Gmail,
            kind: EventKind::NewMail,
            key: DedupeKey::new(&["gmail", "m1"]),
            summary: "From Ana: Review please".to_owned(),
            detail: None,
            ids: json!({"message_id": "m1"}),
            raw: json!({"id": "m1", "subject": "Review please"}),
            source: Source::Trigger,
            occurred: Utc::now(),
        };
        let id = store.insert_event(user, &event).await.unwrap().unwrap();
        store
            .move_events(user, &[id], EventState::Pending, EventState::Triaging, None)
            .await
            .unwrap();
        let triage = NewTriage {
            verdict: Verdict::Inform,
            urgency: Some(Urgency::Later),
            summary: Some(summary),
            reason: "A person is waiting.",
            refs: &[id],
        };
        store.insert_triage(user, &triage, &[id]).await.unwrap()
    }

    #[tokio::test]
    async fn an_update_opens_one_chat_that_starts_with_beep() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let key = UserKey::for_test("alice");
        let alice = User::for_test(store.clone(), "alice");
        let summary = "Ana asked you to review PR 42 by Friday.";
        let id = inform(&store, &key, summary).await;

        let chat = open(&alice, id).await.unwrap().expect("alice's update");
        assert_eq!(open(&alice, id).await.unwrap(), Some(chat), "reopened");
        let sessions = alice.sessions().await.unwrap();
        let listed: Vec<&str> = sessions.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(listed, [chat.to_string()], "made once");

        let messages = alice.session_messages(chat).await.unwrap().unwrap();
        let bodies: Vec<&StoredBody> = messages.iter().map(|m| &m.body).collect();
        assert_eq!(
            bodies,
            [&StoredBody::Assistant {
                content: summary.to_owned()
            }]
        );
        // Beep answers a reply with the event in what it reads.
        let context = store.context(&key, chat).await.unwrap().unwrap();
        assert!(
            context.conversation.entries()[0]
                .content()
                .contains("Review please")
        );
        // Opening it took it off the Updates list.
        let since = Utc::now() - chrono::Duration::days(2);
        assert!(alice.waiting_updates(since, 20).await.unwrap().is_empty());

        let bob = User::for_test(store.clone(), "bob");
        assert_eq!(open(&bob, id).await.unwrap(), None, "not bob's");
    }

    #[test]
    fn an_update_whose_events_are_gone_says_so() {
        let chat = conversation(&detail(Vec::new()), "x");
        assert!(chat.entries()[0].content().contains("no longer stored"));
        assert_eq!(chat.entries()[1].content(), "I opened this update.");
    }
}
