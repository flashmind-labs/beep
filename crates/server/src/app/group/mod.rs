//! Group chats: an @beep message, or one Jev thinks Beep can help with, starts a run whose reply
//! passes the leak gate.

mod answer;
mod send;

use super::{App, Reach, UserNotice};
use crate::{
    agent::decision::Criteria,
    protocol::{Group, GroupAuthor, GroupMember, GroupMessage, Notice},
    store::{JoinedGroup, MemberRow, MessageRow},
};
use chrono::{DateTime, Utc};
use std::{
    fmt::Write,
    sync::{Arc, PoisonError},
    time::Duration,
};
use uuid::Uuid;

/// Messages the model sees before the one that mentions it.
const HISTORY: i64 = 40;
/// The longest a group reply may take.
const MAX_RUN: Duration = Duration::from_secs(5 * 60);
/// How long the run may go without an event.
const IDLE_TIMEOUT: Duration = Duration::from_secs(150);
/// What Beep posts when the run fails.
const FAILED: &str = "Sorry, I couldn't answer that just now. Try again in a bit.";
/// What Beep posts when an app call needs the sender's approval.
const NEEDS_APPROVAL: &str =
    "That needs your OK first. Ask me in our own chat and you can approve it there.";

/// How long Beep waits after a message without @beep, so a burst finishes first.
const SETTLE: Duration = Duration::from_secs(3);
/// Messages Jev reads to judge whether Beep should answer.
const CALL_CONTEXT: i64 = 15;
/// How sure Jev must be before Beep answers without being mentioned.
const CALL_THRESHOLD: f64 = 0.5;
const CALL_QUESTION: &str =
    "Nobody wrote @beep. Should Beep, the group's assistant, answer the latest message on its own?";
const CALL_CRITERIA: Criteria = Criteria {
    yes: "The latest message asks Beep, or asks the group something Beep can answer, find or do now: where to eat or stay, what to do on a day, opening hours, how to get somewhere, prices, weather, a fact, a plan, an itinerary or a list; or a recent question like that is still open because nobody knows the answer; or the group is planning something Beep could help with; or it asks Beep to remember, note or write down something, or asks for the group or everyone to be reminded of something (\"remind everyone\", \"recuérdanoslo\", \"avísanos a las 10\") (\"apúntalo\", \"remember this\"); or it replies to what Beep said, including a correction, a complaint that Beep was wrong, an objection or a new constraint on Beep's suggestion (an allergy, a diet, a date, a budget) that calls for a better answer.",
    no: "People talking to each other: greetings, jokes, news, feelings, a question only another member can answer (their plans, opinions or whereabouts), a reply to another member that does not need Beep, members deciding among themselves or discussing Beep's suggestion with each other, a rhetorical question or a complaint about the world, thanks, or something already answered.",
};
/// Messages the learner reads around a new one.
const LEARN_CONTEXT: i64 = 10;
const MEMORY_QUESTION: &str = "Does the latest message reveal something lasting about one of the members that Beep should remember about them?";
const MEMORY_CRITERIA: Criteria = Criteria {
    yes: "A lasting fact about a member, said by them or by someone else about them: diet, allergies and intolerances, \
health, preferences and dislikes, people in their life, where they live, work, hobbies, routines, or plans they take part in, \
like a trip; or a request for Beep to remember or note something said earlier in the chat (\"apúntalo\", \"remember this\")",
    no: "Greetings, jokes, logistics of the moment, questions, Beep's own replies, opinions about the world, or anything \
that says nothing lasting about a member",
};

/// Whether `text` calls Beep.
pub(crate) fn mentions_beep(text: &str) -> bool {
    // ASCII lowercasing keeps byte offsets, so `at` indexes `text` too.
    text.to_ascii_lowercase()
        .match_indices("@beep")
        .any(|(at, _)| {
            text[at + "@beep".len()..]
                .chars()
                .next()
                .is_none_or(|next| !next.is_alphanumeric())
        })
}

fn millis(time: DateTime<Utc>) -> i64 {
    time.timestamp_millis()
}

/// `row` as `user_id` sees it.
pub(crate) fn group_message(row: &MessageRow, user_id: &str) -> GroupMessage {
    GroupMessage {
        seq: row.id,
        author: match &row.author_id {
            Some(author) => GroupAuthor::Member {
                name: row.author_name.clone(),
                you: author == user_id,
            },
            None => GroupAuthor::Beep {},
        },
        content: row.content.clone(),
        timestamp: millis(row.sent_at),
    }
}

fn group_member(row: &MemberRow, user_id: &str) -> GroupMember {
    GroupMember {
        name: row.name.clone(),
        you: row.user_id == user_id,
        creator: row.creator,
        joined_at: millis(row.joined_at),
    }
}

/// Marks a group busy while alive, so a run that fails or panics still frees it.
struct Typing {
    app: Arc<App>,
    group_id: Uuid,
}

impl Drop for Typing {
    fn drop(&mut self) {
        self.app.stop_typing(self.group_id);
    }
}

/// What Jev reads to judge the last of `history`: the group and its recent messages.
fn call_state(joined: &JoinedGroup, history: &[MessageRow]) -> String {
    let names: Vec<&str> = joined.members.iter().map(|m| m.name.as_str()).collect();
    let mut state = format!(
        "Group chat \"{}\" with {} and Beep, an assistant that searches the web and knows \
         the members.\n\nMessages, oldest first:",
        joined.group.name,
        names.join(", ")
    );
    for (i, row) in history.iter().enumerate() {
        let latest = if i + 1 == history.len() {
            " (latest)"
        } else {
            ""
        };
        let _ = write!(state, "\n{}{latest}: {}", row.author_name, row.content);
    }
    state
}

/// The chat as the learner reads it for `member`, their own messages marked.
fn learn_text(joined: &JoinedGroup, history: &[MessageRow], member: &MemberRow) -> String {
    let others: Vec<&str> = joined
        .members
        .iter()
        .filter(|m| m.user_id != member.user_id)
        .map(|m| m.name.as_str())
        .collect();
    let mut text = format!(
        "Group chat \"{}\". The user is {}; the other members are {}.\n\nMessages, oldest first:",
        joined.group.name,
        member.name,
        others.join(", ")
    );
    for (i, row) in history.iter().enumerate() {
        let author = match &row.author_id {
            Some(id) if *id == member.user_id => format!("{} (the user)", row.author_name),
            Some(_) => row.author_name.clone(),
            None => "Beep".to_owned(),
        };
        let new = if i + 1 == history.len() { " (new)" } else { "" };
        let _ = write!(text, "\n{author}{new}: {}", row.content);
    }
    text
}

pub(super) fn said(name: &str, text: &str) -> String {
    format!("{name}: {text}")
}

impl App {
    /// `joined` as `user_id` sees it.
    pub(crate) fn group_view(&self, joined: &JoinedGroup, user_id: &str) -> Group {
        Group {
            id: joined.group.id.to_string(),
            name: joined.group.name.clone(),
            members: joined
                .members
                .iter()
                .map(|member| group_member(member, user_id))
                .collect(),
            last: joined.last.as_ref().map(|row| group_message(row, user_id)),
            updated_at: millis(joined.group.updated_at),
            beep_typing: self.beep_typing(joined.group.id),
            pinned: joined
                .members
                .iter()
                .any(|member| member.user_id == user_id && member.pinned),
            apps_shared: joined
                .members
                .iter()
                .any(|member| member.user_id == user_id && member.shares_apps),
        }
    }

    /// Beep is writing in group `id`.
    pub(crate) fn beep_typing(&self, id: Uuid) -> bool {
        self.groups_busy
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .contains(&id)
    }

    /// Marks group `id` busy; `false` when Beep is already answering there.
    fn start_typing(&self, id: Uuid) -> bool {
        self.groups_busy
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(id)
    }

    fn stop_typing(&self, id: Uuid) {
        self.groups_busy
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&id);
    }

    /// Tells every member that group `id` changed.
    fn group_changed(&self, members: &[MemberRow], id: Uuid) {
        for member in members {
            let _ = self
                .notifications
                .send(UserNotice::group_changed(&member.user_id, &id.to_string()));
        }
    }

    /// Wakes every member but `except` with `body`, titled with the group.
    pub(super) fn group_notice(
        &self,
        members: &[MemberRow],
        joined: &JoinedGroup,
        except: Option<&str>,
        body: &str,
    ) {
        for member in members {
            if except == Some(member.user_id.as_str()) {
                let _ = self.notifications.send(UserNotice::group_changed(
                    &member.user_id,
                    &joined.group.id.to_string(),
                ));
                continue;
            }
            let _ = self.notifications.send(UserNotice {
                user: member.user_id.clone(),
                notice: Notice {
                    title: joined.group.name.clone(),
                    body: body.to_owned(),
                    session_id: joined.group.id.to_string(),
                    app: None,
                    approval: None,
                    changed: false,
                    group: true,
                },
                desktop: None,
                to: Reach::Everyone,
            });
        }
    }
}

#[cfg(test)]
mod eval;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_whole_mention_calls_beep() {
        assert!(mentions_beep("@beep where should we eat?"));
        assert!(mentions_beep("what do you think @Beep?"));
        assert!(mentions_beep("hey @BEEP"));
        assert!(!mentions_beep("beep me later"));
        assert!(!mentions_beep("@beeper is a phone"));
    }
}
