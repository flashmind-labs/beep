//! A chat about one item in the Updates list, opened by Beep and carrying its events as context.

use super::pipeline::{clipped, write_event};
use crate::{
    account::user::User,
    memory::memories::clip,
    store::{UpdateChatSeed, UpdateDetail},
};
use anyhow::Result;
use flashmind::core::{Conversation, ConversationEntry};
use std::fmt::Write;
use uuid::Uuid;

/// How much of each event's source record the agent reads, in characters.
const RAW_CHARS: usize = 4000;

/// The chat about update `id`, made on first ask; `None` when the user has no such update.
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

/// What the agent reads: the update and its events, the user opening it, and Beep's `message`.
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

/// The update as triage passed it on, its events with ids, and the apps' records.
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
#[path = "../../tests/routines_events_update_chat.rs"]
mod tests;
