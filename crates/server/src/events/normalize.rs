//! Turns Composio trigger payloads and the server's own polls into
//! [`AppEvent`]s. A happening gets the same key however it arrives, so a
//! Gmail message seen by its trigger and by a poll is stored once.

use super::model::{AppEvent, DedupeKey, EventApp, EventKind, Source};
use crate::feeds::mail::{BULK_LABELS, Email, Mailbox, timestamp};
use crate::memories::clip;
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

impl TriggerMessage {
    /// Reads Composio's V3 envelope, the older V2 and V1 ones, and the
    /// legacy realtime shape. `None` when it is none of them.
    pub fn parse(payload: &Value) -> Option<Self> {
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
            return Some(Self::new(user, slug, &payload["data"], at));
        }
        // V2: `{type: "<slug>", data: {user_id, ...payload}}`.
        if let (Some(slug), Some(user)) = (
            payload["type"].as_str(),
            payload["data"]["user_id"].as_str(),
        ) {
            return Some(Self::new(user, slug, &payload["data"], at));
        }
        // Legacy realtime: `{appName, payload, metadata: {triggerName, connection: {clientUniqueUserId}}}`.
        if let (Some(slug), Some(user)) = (
            metadata["triggerName"].as_str(),
            metadata["connection"]["clientUniqueUserId"].as_str(),
        ) {
            return Some(Self::new(user, slug, &payload["payload"], at));
        }
        // The SDK's own normalized shape: `{triggerSlug, userId, payload}`.
        if let (Some(slug), Some(user)) =
            (payload["triggerSlug"].as_str(), payload["userId"].as_str())
        {
            return Some(Self::new(user, slug, &payload["payload"], at));
        }
        None
    }

    fn new(user: &str, slug: &str, data: &Value, at: DateTime<Utc>) -> Self {
        Self {
            user: user.to_owned(),
            slug: slug.to_ascii_uppercase(),
            data: data.clone(),
            at,
        }
    }
}

/// The event a trigger message reports, or `None` for a trigger this
/// module does not read or a message not worth an event, such as mail the
/// user sent or a promotion.
pub fn from_trigger(message: &TriggerMessage) -> Option<AppEvent> {
    let data = &message.data;
    let mut event = match message.slug.as_str() {
        GMAIL_NEW_MESSAGE => gmail_message(data)?,
        OUTLOOK_NEW_MESSAGE => outlook_message(data)?,
        OUTLOOK_EVENT_CHANGE => outlook_calendar(data)?,
        GCAL_CREATED | GCAL_UPDATED | GCAL_CANCELLED | GCAL_SOON => {
            google_calendar(&message.slug, data)?
        }
        _ => return None,
    };
    // A payload without a time of its own takes the delivery's.
    if event.occurred == DateTime::UNIX_EPOCH {
        event.occurred = message.at;
    }
    Some(event)
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

/// A GitHub notification thread from `GITHUB_LIST_NOTIFICATIONS`, or `None`
/// when it has no id or is a CI run that passed.
pub fn from_github_notification(notification: &Value) -> Option<AppEvent> {
    let thread = str_of(notification, &["id"])?;
    let reason = str_of(notification, &["reason"]).unwrap_or_default();
    let subject = &notification["subject"];
    let title = str_of(subject, &["title"]).unwrap_or("(untitled)");
    let kind_of_thread = str_of(subject, &["type"]).unwrap_or_default();
    let repo = str_of(&notification["repository"], &["full_name"]).unwrap_or("a repository");
    let updated = str_of(notification, &["updated_at"]).unwrap_or_default();
    let kind = match reason {
        "review_requested" => EventKind::ReviewRequested,
        "mention" | "team_mention" => EventKind::Mention,
        "assign" => EventKind::Assigned,
        "ci_activity" => EventKind::CiFailed,
        "author" | "comment" => EventKind::ThreadActivity,
        "subscribed" if kind_of_thread == "PullRequest" => EventKind::PullRequest,
        _ => EventKind::GithubOther,
    };
    if kind == EventKind::CiFailed && title.to_ascii_lowercase().contains("succeeded") {
        return None;
    }
    let number = subject["url"]
        .as_str()
        .and_then(|url| url.rsplit('/').next())
        .and_then(|last| last.parse::<u64>().ok());
    let what = match (kind_of_thread, number) {
        ("PullRequest", Some(n)) => format!("PR #{n}"),
        ("Issue", Some(n)) => format!("issue #{n}"),
        ("CheckSuite", _) => "CI".to_owned(),
        (other, _) if !other.is_empty() => other.to_owned(),
        _ => "thread".to_owned(),
    };
    let happened = match kind {
        EventKind::ReviewRequested => "review requested on",
        EventKind::Mention => "mentioned in",
        EventKind::Assigned => "assigned",
        EventKind::CiFailed => "failed",
        EventKind::ThreadActivity => "new activity on",
        EventKind::PullRequest => "activity on",
        _ => "notification on",
    };
    let link = match (kind_of_thread, number) {
        ("PullRequest", Some(n)) => Some(format!("https://github.com/{repo}/pull/{n}")),
        ("Issue", Some(n)) => Some(format!("https://github.com/{repo}/issues/{n}")),
        _ => None,
    };
    let mut ids = json!({"repo": repo, "thread_id": thread, "type": kind_of_thread});
    if let Some(n) = number {
        ids["number"] = json!(n);
    }
    if let Some(link) = link {
        ids["url"] = json!(link);
    }
    Some(AppEvent {
        app: EventApp::Github,
        kind,
        // A thread comes back each time it gets new activity.
        key: DedupeKey::new(&["github", thread, updated]),
        summary: clip(
            &format!("{repo}: {happened} {what} \"{title}\""),
            SUMMARY_CHARS,
        ),
        detail: None,
        ids,
        raw: notification.clone(),
        source: Source::Poll,
        occurred: timestamp(updated).unwrap_or_else(Utc::now),
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
        key: DedupeKey::new(&[mail.app.slug(), mail.id]),
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
mod tests {
    use super::*;

    fn gmail_payload(id: &str, labels: &[&str]) -> Value {
        json!({
            "id": "log_1",
            "timestamp": "2026-10-20T10:00:00Z",
            "type": "composio.trigger.message",
            "metadata": {
                "log_id": "log_1",
                "trigger_slug": "GMAIL_NEW_GMAIL_MESSAGE",
                "trigger_id": "ti_1",
                "connected_account_id": "ca_1",
                "auth_config_id": "ac_1",
                "user_id": "outside-1"
            },
            "data": {
                "message_id": id,
                "thread_id": "t1",
                "sender": "Ana <ana@example.com>",
                "subject": "Lease renewal",
                "preview": {"body": "Can you sign\n by Friday?"},
                "label_ids": labels,
                "message_timestamp": "2026-10-20T09:59:00Z"
            }
        })
    }

    #[test]
    fn reads_every_composio_envelope() {
        let v3 = TriggerMessage::parse(&gmail_payload("m1", &["INBOX"])).unwrap();
        assert_eq!(v3.user, "outside-1");
        assert_eq!(v3.slug, GMAIL_NEW_MESSAGE);
        assert_eq!(v3.data["message_id"], "m1");

        let v2 = TriggerMessage::parse(&json!({
            "type": "gmail_new_gmail_message",
            "timestamp": "2026-10-20T10:00:00Z",
            "log_id": "log_2",
            "data": {"user_id": "outside-2", "message_id": "m2"}
        }))
        .unwrap();
        assert_eq!(v2.user, "outside-2");
        assert_eq!(v2.slug, GMAIL_NEW_MESSAGE);

        let legacy = TriggerMessage::parse(&json!({
            "appName": "gmail",
            "payload": {"message_id": "m3"},
            "metadata": {
                "nanoId": "ti_3",
                "triggerName": "GMAIL_NEW_GMAIL_MESSAGE",
                "connection": {"clientUniqueUserId": "outside-3"}
            }
        }))
        .unwrap();
        assert_eq!(legacy.user, "outside-3");
        assert_eq!(legacy.data["message_id"], "m3");

        assert!(TriggerMessage::parse(&json!({"hello": "world"})).is_none());
    }

    #[test]
    fn a_gmail_trigger_and_a_poll_of_the_same_email_share_a_key() {
        let message = TriggerMessage::parse(&gmail_payload("m1", &["INBOX"])).unwrap();
        let triggered = from_trigger(&message).unwrap();
        assert_eq!(triggered.kind, EventKind::NewMail);
        assert_eq!(
            triggered.summary,
            "From Ana <ana@example.com>: Lease renewal"
        );
        assert_eq!(triggered.detail.as_deref(), Some("Can you sign by Friday?"));
        assert_eq!(
            triggered.ids,
            json!({"message_id": "m1", "thread_id": "t1"})
        );
        assert_eq!(triggered.occurred.to_rfc3339(), "2026-10-20T09:59:00+00:00");

        let email = Email {
            id: "m1".into(),
            received: Utc::now(),
            from: "Ana <ana@example.com>".into(),
            to: "user@host".into(),
            subject: "Lease renewal".into(),
            sent: false,
            bulk: false,
            thread: Some("t1".into()),
            body: "Can you sign by Friday?".into(),
        };
        let polled = from_email(Mailbox::Gmail, &email).unwrap();
        assert_eq!(polled.key, triggered.key);
        assert_eq!(polled.source, Source::Poll);
        assert!(
            from_email(
                Mailbox::Gmail,
                &Email {
                    sent: true,
                    ..email.clone()
                }
            )
            .is_none()
        );
        assert!(
            from_email(
                Mailbox::Gmail,
                &Email {
                    bulk: true,
                    ..email
                }
            )
            .is_none()
        );
    }

    #[test]
    fn outlook_mail_reads_both_field_styles_and_skips_other() {
        let message = |data: Value| TriggerMessage {
            user: "outside-1".into(),
            slug: OUTLOOK_NEW_MESSAGE.into(),
            data,
            at: Utc::now(),
        };
        let snake = from_trigger(&message(json!({
            "event_type": "created",
            "id": "AAMk1",
            "outlook_message": {
                "subject": "Invoice",
                "from_address": {"email_address": {"name": "Billing", "address": "billing@example.com"}},
                "body_preview": "Due soon",
                "conversation_id": "c1",
                "inference_classification": "focused"
            }
        })))
        .unwrap();
        assert_eq!(snake.summary, "From Billing: Invoice");
        assert_eq!(snake.ids["thread_id"], "c1");
        assert_eq!(snake.key.as_str(), "outlook:AAMk1");

        let camel = from_trigger(&message(json!({
            "id": "AAMk2",
            "message": {"subject": "Hi", "from": {"emailAddress": {"address": "a@example.com"}}}
        })))
        .unwrap();
        assert_eq!(camel.summary, "From a@example.com: Hi");

        assert!(
            from_trigger(&message(json!({
                "id": "AAMk3",
                "outlook_message": {"subject": "Sale", "inference_classification": "other"}
            })))
            .is_none()
        );
    }

    fn notification(reason: &str, kind: &str, title: &str, updated: &str) -> Value {
        json!({
            "id": "123",
            "reason": reason,
            "unread": true,
            "updated_at": updated,
            "repository": {"full_name": "org/repo"},
            "subject": {
                "title": title,
                "type": kind,
                "url": "https://api.github.com/repos/org/repo/pulls/42"
            }
        })
    }

    #[test]
    fn github_notifications_are_sorted_by_reason() {
        let review = from_github_notification(&notification(
            "review_requested",
            "PullRequest",
            "Add caching",
            "2026-10-20T10:00:00Z",
        ))
        .unwrap();
        assert_eq!(review.kind, EventKind::ReviewRequested);
        assert_eq!(
            review.summary,
            "org/repo: review requested on PR #42 \"Add caching\""
        );
        assert_eq!(review.ids["number"], 42);
        assert_eq!(review.ids["url"], "https://github.com/org/repo/pull/42");

        let cases = [
            ("mention", EventKind::Mention),
            ("team_mention", EventKind::Mention),
            ("assign", EventKind::Assigned),
            ("author", EventKind::ThreadActivity),
            ("subscribed", EventKind::PullRequest),
            ("state_change", EventKind::GithubOther),
        ];
        for (reason, kind) in cases {
            let event = from_github_notification(&notification(
                reason,
                "PullRequest",
                "t",
                "2026-10-20T10:00:00Z",
            ))
            .unwrap();
            assert_eq!(event.kind, kind, "{reason}");
        }

        // New activity on the same thread is a new event.
        let later = from_github_notification(&notification(
            "review_requested",
            "PullRequest",
            "Add caching",
            "2026-10-20T11:00:00Z",
        ))
        .unwrap();
        assert_ne!(later.key, review.key);

        let failed = from_github_notification(&notification(
            "ci_activity",
            "CheckSuite",
            "CI workflow run failed for main branch",
            "2026-10-20T10:00:00Z",
        ))
        .unwrap();
        assert_eq!(failed.kind, EventKind::CiFailed);
        assert!(
            from_github_notification(&notification(
                "ci_activity",
                "CheckSuite",
                "CI workflow run succeeded for main branch",
                "2026-10-20T10:00:00Z",
            ))
            .is_none()
        );
    }
}
