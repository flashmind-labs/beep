//! Turns Composio trigger payloads and polls into [`AppEvent`]s, keyed so both paths dedupe.

mod github;

pub use github::GitHubNotification;

use super::model::{AppEvent, DedupeKey, EventApp, EventKind, Source};
use crate::{
    apps::feeds::mail::{BULK_LABELS, Email, Mailbox, timestamp},
    memory::memories::clip,
};
use chrono::{DateTime, Utc};
use serde_json::{Value, json};

/// What the agent sees of an email or event body.
const DETAIL_CHARS: usize = 300;
/// The longest summary line.
const SUMMARY_CHARS: usize = 200;

/// The Composio trigger slugs this module reads.
pub(crate) const GMAIL_NEW_MESSAGE: &str = "GMAIL_NEW_GMAIL_MESSAGE";
pub(crate) const OUTLOOK_NEW_MESSAGE: &str = "OUTLOOK_MESSAGE_TRIGGER";
pub(crate) const OUTLOOK_EVENT_CHANGE: &str = "OUTLOOK_EVENT_CHANGE_TRIGGER";
pub(crate) const GCAL_CREATED: &str = "GOOGLECALENDAR_GOOGLE_CALENDAR_EVENT_CREATED_TRIGGER";
pub(crate) const GCAL_UPDATED: &str = "GOOGLECALENDAR_GOOGLE_CALENDAR_EVENT_UPDATED_TRIGGER";
pub(crate) const GCAL_CANCELLED: &str = "GOOGLECALENDAR_EVENT_CANCELED_DELETED_TRIGGER";
pub(crate) const GCAL_SOON: &str = "GOOGLECALENDAR_EVENT_STARTING_SOON_TRIGGER";

#[derive(Debug, thiserror::Error)]
#[error("the payload does not describe a supported event")]
pub struct UnsupportedEvent;

/// One trigger event as Composio delivers it, whichever payload version.
#[derive(Debug, Clone, PartialEq)]
pub struct TriggerMessage {
    /// The Composio user id, which is the account's outside id.
    pub user: String,
    /// The trigger type, such as `GMAIL_NEW_GMAIL_MESSAGE`.
    pub slug: String,
    pub data: Value,
    pub at: DateTime<Utc>,
}

impl TryFrom<&Value> for TriggerMessage {
    type Error = UnsupportedEvent;

    fn try_from(payload: &Value) -> Result<Self, Self::Error> {
        let at = payload["timestamp"]
            .as_str()
            .and_then(timestamp)
            .unwrap_or_else(Utc::now);
        // V3: `{type: "composio.trigger.message", metadata: {trigger_slug, user_id}, data}`.
        let metadata = &payload["metadata"];
        if let (Some(slug), Some(user)) = (
            metadata["trigger_slug"].as_str(),
            metadata["user_id"].as_str(),
        ) {
            return Ok(Self::new(user, slug, &payload["data"], at));
        }
        // V2: `{type: "<slug>", data: {user_id, ...payload}}`.
        if let (Some(slug), Some(user)) = (
            payload["type"].as_str(),
            payload["data"]["user_id"].as_str(),
        ) {
            return Ok(Self::new(user, slug, &payload["data"], at));
        }
        // Legacy realtime: `{appName, payload, metadata: {triggerName, connection: {clientUniqueUserId}}}`.
        if let (Some(slug), Some(user)) = (
            metadata["triggerName"].as_str(),
            metadata["connection"]["clientUniqueUserId"].as_str(),
        ) {
            return Ok(Self::new(user, slug, &payload["payload"], at));
        }
        // The SDK's own normalized shape: `{triggerSlug, userId, payload}`.
        if let (Some(slug), Some(user)) =
            (payload["triggerSlug"].as_str(), payload["userId"].as_str())
        {
            return Ok(Self::new(user, slug, &payload["payload"], at));
        }

        Err(UnsupportedEvent)
    }
}

impl TriggerMessage {
    fn new(user: &str, slug: &str, data: &Value, at: DateTime<Utc>) -> Self {
        Self {
            user: user.to_owned(),
            slug: slug.to_ascii_uppercase(),
            data: data.clone(),
            at,
        }
    }
}

/// Supported trigger events, excluding sent mail and promotions.
impl TryFrom<&TriggerMessage> for AppEvent {
    type Error = UnsupportedEvent;

    fn try_from(message: &TriggerMessage) -> Result<Self, Self::Error> {
        let data = &message.data;
        let mut event = match message.slug.as_str() {
            GMAIL_NEW_MESSAGE => gmail_message(data).ok_or(UnsupportedEvent)?,
            OUTLOOK_NEW_MESSAGE => outlook_message(data).ok_or(UnsupportedEvent)?,
            OUTLOOK_EVENT_CHANGE => outlook_calendar(data).ok_or(UnsupportedEvent)?,
            GCAL_CREATED | GCAL_UPDATED | GCAL_CANCELLED | GCAL_SOON => {
                google_calendar(&message.slug, data).ok_or(UnsupportedEvent)?
            }
            _ => return Err(UnsupportedEvent),
        };
        // A payload without a time of its own takes the delivery's.
        if event.occurred == DateTime::UNIX_EPOCH {
            event.occurred = message.at;
        }
        Ok(event)
    }
}

fn gmail_message(data: &Value) -> Option<AppEvent> {
    let id = str_of(data, &["message_id", "messageId", "id"])?;
    let labels: Vec<&str> = data["label_ids"]
        .as_array()
        .or_else(|| data["labelIds"].as_array())
        .map(|labels| labels.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let has = |label: &str| labels.contains(&label);

    if has("SENT") || has("DRAFT") || BULK_LABELS.iter().any(|label| has(label)) {
        return None;
    }

    let from = str_of(data, &["sender", "from"]).unwrap_or("someone");
    let subject = str_of(data, &["subject"]).unwrap_or("(no subject)");
    let preview = data["preview"]["body"]
        .as_str()
        .or_else(|| data["preview"].as_str())
        .or_else(|| data["message_text"].as_str());
    let thread = str_of(data, &["thread_id", "threadId"]);
    Some(mail_event(MailFields {
        app: EventApp::Gmail,
        id,
        thread,
        from,
        subject,
        preview,
        occurred: str_of(data, &["message_timestamp", "messageTimestamp"]).and_then(timestamp),
        source: Source::Trigger,
        raw: data,
    }))
}

fn outlook_message(data: &Value) -> Option<AppEvent> {
    // `outlook_message` has snake_case fields; the older `message` camelCase.
    let message = if data["outlook_message"].is_object() {
        &data["outlook_message"]
    } else {
        &data["message"]
    };
    let id = str_of(data, &["id"]).or_else(|| str_of(message, &["id"]))?;

    if message["is_draft"]
        .as_bool()
        .or(message["isDraft"].as_bool())
        == Some(true)
    {
        return None;
    }
    // Outlook's Focused Inbox files mail nobody wrote in person under Other.
    let classified = str_of(
        message,
        &["inference_classification", "inferenceClassification"],
    );

    if classified.is_some_and(|class| class.eq_ignore_ascii_case("other")) {
        return None;
    }

    let sender = if message["from_address"].is_object() {
        &message["from_address"]["email_address"]
    } else if message["from"].is_object() {
        &message["from"]["emailAddress"]
    } else {
        &message["sender"]["email_address"]
    };
    let from = str_of(sender, &["name"])
        .filter(|name| !name.is_empty())
        .or_else(|| str_of(sender, &["address"]))
        .unwrap_or("someone");
    Some(mail_event(MailFields {
        app: EventApp::Outlook,
        id,
        thread: str_of(message, &["conversation_id", "conversationId"]),
        from,
        subject: str_of(message, &["subject"]).unwrap_or("(no subject)"),
        preview: str_of(message, &["body_preview", "bodyPreview"]),
        occurred: str_of(message, &["received_date_time", "receivedDateTime"]).and_then(timestamp),
        source: Source::Trigger,
        raw: data,
    }))
}

fn outlook_calendar(data: &Value) -> Option<AppEvent> {
    let id = str_of(data, &["id"])?;
    let change = str_of(data, &["event_type"]).unwrap_or("changed");
    let kind = match change.to_ascii_lowercase().as_str() {
        "created" => EventKind::CalendarCreated,
        "deleted" => EventKind::CalendarCancelled,
        _ => EventKind::CalendarChanged,
    };
    Some(AppEvent {
        app: EventApp::Outlook,
        kind,
        // Outlook sends no change time, so one id and change type is one event.
        key: DedupeKey::new(&["outlook-event", id, change]),
        summary: format!("Outlook calendar event {change}"),
        detail: None,
        ids: json!({"event_id": id}),
        raw: data.clone(),
        source: Source::Trigger,
        occurred: DateTime::UNIX_EPOCH,
    })
}

fn google_calendar(slug: &str, data: &Value) -> Option<AppEvent> {
    let id = str_of(data, &["event_id"])?;
    let title = str_of(data, &["summary"]).unwrap_or("an event");
    let calendar = str_of(data, &["calendar_id"]).unwrap_or("primary");
    let (kind, key, summary, detail) = match slug {
        GCAL_CREATED => {
            let start = str_of(data, &["start_time"]).unwrap_or("an unknown time");
            let organizer = str_of(data, &["organizer_name", "organizer_email"]);
            let by = organizer
                .map(|who| format!(" by {who}"))
                .unwrap_or_default();
            (
                EventKind::CalendarCreated,
                DedupeKey::new(&["gcal", "created", id]),
                format!("New calendar event \"{title}\" at {start}{by}"),
                None,
            )
        }
        GCAL_UPDATED => {
            let changed: Vec<&str> = data["updated_fields"]
                .as_object()
                .map(|fields| fields.keys().map(String::as_str).collect())
                .unwrap_or_default();
            let at = str_of(data, &["updated_at"]).unwrap_or_default();
            let what = if changed.is_empty() {
                String::new()
            } else {
                format!(": {}", changed.join(", "))
            };
            (
                EventKind::CalendarChanged,
                DedupeKey::new(&["gcal", "updated", id, at]),
                format!("Calendar event changed{what}"),
                Some(clip(&data["updated_fields"].to_string(), DETAIL_CHARS)),
            )
        }
        GCAL_CANCELLED => (
            EventKind::CalendarCancelled,
            DedupeKey::new(&["gcal", "cancelled", id]),
            format!("Calendar event \"{title}\" was cancelled"),
            None,
        ),
        _ => {
            let start = str_of(data, &["start_time"]).unwrap_or_default();
            let minutes = data["minutes_until_start"]
                .as_f64()
                .map(|minutes| format!(" in {} minutes", minutes.round()))
                .unwrap_or_default();
            let place = [str_of(data, &["location"]), str_of(data, &["hangout_link"])]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(", ");
            (
                EventKind::CalendarSoon,
                DedupeKey::new(&["gcal", "soon", id, start]),
                format!("\"{title}\" starts{minutes}"),
                (!place.is_empty()).then_some(place),
            )
        }
    };
    Some(AppEvent {
        app: EventApp::GoogleCalendar,
        kind,
        key,
        summary: clip(&summary, SUMMARY_CHARS),
        detail,
        ids: json!({"event_id": id, "calendar_id": calendar}),
        raw: data.clone(),
        source: Source::Trigger,
        occurred: DateTime::UNIX_EPOCH,
    })
}

/// An unread email a poll found, or `None` for mail the user sent or bulk mail.
pub fn from_email(mailbox: Mailbox, email: &Email) -> Option<AppEvent> {
    if email.sent || email.bulk {
        return None;
    }

    let app = match mailbox {
        Mailbox::Gmail => EventApp::Gmail,
        Mailbox::Outlook => EventApp::Outlook,
        Mailbox::Fastmail => EventApp::Fastmail,
    };
    let raw = json!({
        "id": email.id,
        "from": email.from,
        "subject": email.subject,
        "received": email.received.to_rfc3339(),
    });
    Some(mail_event(MailFields {
        app,
        id: &email.id,
        thread: email.thread.as_deref(),
        from: if email.from.is_empty() {
            "someone"
        } else {
            &email.from
        },
        subject: &email.subject,
        preview: Some(&email.body),
        occurred: Some(email.received),
        source: Source::Poll,
        raw: &raw,
    }))
}

/// What a mail event is made of.
struct MailFields<'a> {
    app: EventApp,
    id: &'a str,
    thread: Option<&'a str>,
    from: &'a str,
    subject: &'a str,
    preview: Option<&'a str>,
    occurred: Option<DateTime<Utc>>,
    source: Source,
    raw: &'a Value,
}

fn mail_event(mail: MailFields<'_>) -> AppEvent {
    let mut ids = json!({"message_id": mail.id});

    if let Some(thread) = mail.thread {
        ids["thread_id"] = json!(thread);
    }

    let detail = mail
        .preview
        .map(|preview| {
            clip(
                preview
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
                    .as_str(),
                DETAIL_CHARS,
            )
        })
        .filter(|preview| !preview.is_empty());
    AppEvent {
        app: mail.app,
        kind: EventKind::NewMail,
        key: DedupeKey::new(&[mail.app.as_ref(), mail.id]),
        summary: clip(
            &format!("From {}: {}", mail.from, mail.subject),
            SUMMARY_CHARS,
        ),
        detail,
        ids,
        raw: mail.raw.clone(),
        source: mail.source,
        occurred: mail.occurred.unwrap_or(DateTime::UNIX_EPOCH),
    }
}

/// The first of `keys` in `value` that holds a non-empty string.
fn str_of<'a>(value: &'a Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .find_map(|key| value[*key].as_str().filter(|text| !text.is_empty()))
}

#[cfg(test)]
#[path = "../../tests/routines_events_normalize.rs"]
mod tests;
