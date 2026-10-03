//! Learns about users from their email.
//!
//! Once a day each connected mailbox is read from where the last run stopped,
//! or six months back on the first run. Jev scores every email, in parallel,
//! for whether it says something lasting about the user; the likely ones go to
//! the memory learner one at a time, oldest first, so newer mail corrects what
//! older mail taught. No agent decides what to read.
//!
//! The cursor is the newest email processed. Each run reads from a day before
//! it and skips the message ids it already processed, so mail that arrives
//! late is not lost and a run that stops halfway resumes where it was.
//! Email content is never logged.

use crate::{
    app::App,
    decision::{Criteria, DecisionTool},
};
use anyhow::{Context, Result, bail};
use chrono::{DateTime, Duration as Days, Utc};
use flashmind::tools::composio::ComposioClient;
use futures::{StreamExt, stream};
use serde_json::{Value, json};
use std::{collections::HashSet, fmt::Write, sync::Arc, time::Duration};

/// How far back the first run reads.
const BACKFILL_DAYS: i64 = 183;
/// Each run starts this long before the cursor; seen ids skip the overlap.
const OVERLAP_DAYS: i64 = 1;
const SYNC_EVERY: Duration = Duration::from_secs(24 * 60 * 60);
const SYNC_SWEEP: Duration = Duration::from_secs(60 * 60);
/// Jev calls, and Fastmail body reads, in flight at once.
const CONCURRENCY: usize = 8;
const PAGE_SIZE: usize = 50;
/// Stops a provider that keeps handing out next pages.
const MAX_PAGES: usize = 400;
/// What Jev sees of each email: headers and the start of the body.
const JEV_EMAIL_CHARS: usize = 2500;
/// Above this Jev score the learner reads the email.
const MEMORY_THRESHOLD: f64 = 0.8;
const EMAIL_QUESTION: &str = "Does this email from the user's mailbox show a fact about the user still worth remembering today?";
const EMAIL_CRITERIA: Criteria = Criteria {
    yes: "A lasting fact about the user: their name, people in their life, where they live or work, \
what they do, what they like, or a trip or appointment still ahead",
    no: "Newsletters, marketing, notifications, receipts, one-off errands and admin, events already over, \
or facts only about other people",
};

/// A mailbox the user can connect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mailbox {
    Gmail,
    Fastmail,
}

impl Mailbox {
    /// The app slug, also the key of its sync state.
    pub fn slug(self) -> &'static str {
        match self {
            Mailbox::Gmail => "gmail",
            Mailbox::Fastmail => crate::mcp::FASTMAIL,
        }
    }
}

/// One email, as the learner and Jev read it.
#[derive(Debug, Clone)]
pub struct Email {
    pub id: String,
    pub received: DateTime<Utc>,
    pub from: String,
    pub to: String,
    pub subject: String,
    /// Set for mail the user sent.
    pub sent: bool,
    /// Plain text; only a preview until `fill_body` reads the rest.
    pub body: String,
}

impl Email {
    /// Headers and the body clipped to `chars`.
    pub fn render(&self, chars: usize) -> String {
        let mut text = String::new();
        let _ = writeln!(
            text,
            "Date: {}",
            self.received.format("%A %Y-%m-%d %H:%M UTC")
        );
        if self.sent {
            let _ = writeln!(text, "Sent by the user");
        }
        let _ = writeln!(text, "From: {}", self.from);
        let _ = writeln!(text, "To: {}", self.to);
        let _ = writeln!(text, "Subject: {}", self.subject);
        let _ = write!(text, "\n{}", clip(self.body.trim(), chars));
        text
    }
}

/// One page of a mailbox listing.
struct Page {
    emails: Vec<Email>,
    next: Option<String>,
}

/// Where a user's mail is read from.
enum Source<'a> {
    Gmail(Arc<ComposioClient>),
    Fastmail { app: &'a App, user: &'a str },
}

impl<'a> Source<'a> {
    fn new(app: &'a App, user: &'a str, mailbox: Mailbox) -> Result<Self> {
        Ok(match mailbox {
            Mailbox::Gmail => Source::Gmail(
                app.apps
                    .client(user)
                    .context("Composio is not configured")?,
            ),
            Mailbox::Fastmail => Source::Fastmail { app, user },
        })
    }

    async fn page(&self, after: DateTime<Utc>, cursor: Option<&str>) -> Result<Page> {
        match self {
            Source::Gmail(client) => {
                // Gmail reads Unix seconds after `after:`.
                let mut arguments = json!({
                    "query": format!("after:{}", after.timestamp()),
                    "max_results": PAGE_SIZE,
                    "include_payload": true,
                });
                if let Some(cursor) = cursor {
                    arguments["page_token"] = json!(cursor);
                }
                let response = client.execute_tool("GMAIL_FETCH_EMAILS", arguments).await?;
                if response.successful == Some(false) {
                    bail!("Gmail fetch failed");
                }
                parse_gmail(&response.data)
            }
            Source::Fastmail { app, user } => {
                // Fastmail only takes days; the overlap and seen ids cover the rest.
                let query = format!("after:{}", after.format("%Y-%m-%d"));
                let mut arguments = json!({"query": query, "limit": PAGE_SIZE});
                if let Some(cursor) = cursor {
                    arguments["cursor"] = json!(cursor);
                }
                let value = app
                    .mcp
                    .call(user, crate::mcp::FASTMAIL, "search_email", arguments)
                    .await?;
                parse_fastmail(&value)
            }
        }
    }

    /// Replaces a Fastmail preview with the whole body. On failure the
    /// preview stays and is scored as it is.
    async fn fill_body(&self, email: &mut Email) {
        let Source::Fastmail { app, user } = self else {
            return;
        };
        let arguments = json!({"id": email.id});
        match app
            .mcp
            .call(user, crate::mcp::FASTMAIL, "read_email", arguments)
            .await
        {
            Ok(value) => {
                if let Some(body) = value["bodyText"].as_str() {
                    email.body = body.to_owned();
                }
            }
            Err(error) => log::debug!("mail: unable to read a Fastmail body: {error:#}"),
        }
    }

    /// Every email received after `after`, oldest first.
    async fn list(&self, after: DateTime<Utc>) -> Result<Vec<Email>> {
        let mut emails = Vec::new();
        let mut cursor: Option<String> = None;
        for page_number in 1..=MAX_PAGES {
            let page = self
                .page(after, cursor.as_deref())
                .await
                .with_context(|| format!("page {page_number}"))?;
            log::debug!(
                "mail: listed page {page_number} with {} emails",
                page.emails.len()
            );
            emails.extend(page.emails);
            match page.next {
                Some(next) if !next.is_empty() => cursor = Some(next),
                _ => break,
            }
            if page_number == MAX_PAGES {
                log::warn!("mail: stopped listing after {MAX_PAGES} pages");
            }
        }
        emails.sort_by_key(|email| email.received);
        Ok(emails)
    }
}

/// How a mailbox run went.
#[derive(Debug, Default)]
struct Outcome {
    listed: usize,
    scored: usize,
    passed: usize,
    learned: usize,
}

/// Syncs every connected mailbox not synced in the last day.
pub async fn sync_due(app: &App) {
    let Some(decision) = app.decision() else {
        log::debug!("mail: Jev is not configured; skipping mail sync");
        return;
    };
    for user in app.auth.user_ids() {
        for mailbox in app.apps.mailboxes(user).await {
            match app
                .store
                .mail_synced_recently(user, mailbox, SYNC_EVERY)
                .await
            {
                Ok(true) => {
                    log::debug!("mail: {} synced recently; user={user}", mailbox.slug());
                    continue;
                }
                Ok(false) => {}
                Err(error) => {
                    log::warn!("mail: unable to read sync state: {error:#}");
                    continue;
                }
            }
            log::info!("mail: syncing {}; user={user}", mailbox.slug());
            match sync(app, decision, user, mailbox).await {
                Ok(outcome) => {
                    log::info!(
                        "mail: synced {}; user={user} listed={} scored={} passed={} learned={}",
                        mailbox.slug(),
                        outcome.listed,
                        outcome.scored,
                        outcome.passed,
                        outcome.learned
                    );
                    if let Err(error) = app.store.mark_mail_synced(user, mailbox).await {
                        log::warn!("mail: unable to save sync state: {error:#}");
                    }
                }
                // Not marked, so the next sweep tries again; progress so far is kept.
                Err(error) => log::warn!(
                    "mail: sync of {} stopped: {error:#}; user={user}",
                    mailbox.slug()
                ),
            }
        }
    }
}

async fn sync(app: &App, decision: &DecisionTool, user: &str, mailbox: Mailbox) -> Result<Outcome> {
    let source = Source::new(app, user, mailbox)?;
    let store = &app.store;
    let after = match store.mail_cursor(user, mailbox).await? {
        Some(cursor) => cursor - Days::days(OVERLAP_DAYS),
        None => Utc::now() - Days::days(BACKFILL_DAYS),
    };
    store
        .prune_mail_seen(user, mailbox, after - Days::days(OVERLAP_DAYS))
        .await?;
    let seen = store.mail_seen(user, mailbox).await?;
    let listed = source.list(after).await?;
    let mut outcome = Outcome {
        listed: listed.len(),
        ..Outcome::default()
    };
    let fresh = unseen(listed, &seen);
    log::info!(
        "mail: {} new of {} listed in {} after {after}; user={user}",
        fresh.len(),
        outcome.listed,
        mailbox.slug()
    );

    // Bodies and scores run ahead in parallel; results come back in order.
    let today = Utc::now().format("%A %Y-%m-%d").to_string();
    let mut scored = stream::iter(fresh)
        .map(|email| score(decision, &source, &today, email))
        .buffered(CONCURRENCY);
    let generator = app.generator(user);
    while let Some((email, score)) = scored.next().await {
        // Stop at the first unscored email so the cursor never passes it.
        let probability = score.context("Jev failed")?;
        log::debug!("mail: scored {probability:.2}; user={user} id={}", email.id);
        outcome.scored += 1;
        if probability >= MEMORY_THRESHOLD {
            outcome.passed += 1;
            outcome.learned += app.memories.learn_email(generator, &email).await.len();
        }
        store
            .mark_mail_seen(user, mailbox, &email.id, email.received)
            .await?;
    }
    Ok(outcome)
}

/// Fetches the body and asks Jev how likely the email is to teach a lasting fact.
async fn score(
    decision: &DecisionTool,
    source: &Source<'_>,
    today: &str,
    mut email: Email,
) -> (Email, Result<f64>) {
    source.fill_body(&mut email).await;
    let state = format!("Today: {today}\n{}", email.render(JEV_EMAIL_CHARS));
    let score = decision
        .probability_with(&state, EMAIL_QUESTION, &EMAIL_CRITERIA)
        .await;
    (email, score)
}

/// Drops emails already processed and duplicates within the listing.
fn unseen(emails: Vec<Email>, seen: &HashSet<String>) -> Vec<Email> {
    let mut ids = HashSet::new();
    emails
        .into_iter()
        .filter(|email| !seen.contains(&email.id) && ids.insert(email.id.clone()))
        .collect()
}

/// Runs the mail sync once at start and then every hour.
pub fn start(app: Arc<App>) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(SYNC_SWEEP);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            interval.tick().await;
            sync_due(&app).await;
        }
    });
}

fn parse_gmail(data: &Value) -> Result<Page> {
    let messages = data["messages"]
        .as_array()
        .context("Gmail returned no messages list")?;
    let mut emails = Vec::new();
    for message in messages {
        let Some(id) = message["messageId"].as_str() else {
            continue;
        };
        let Some(received) = message["messageTimestamp"].as_str().and_then(timestamp) else {
            log::debug!("mail: Gmail message without a usable date; id={id}");
            continue;
        };
        let labels = message["labelIds"].as_array();
        let sent = labels.is_some_and(|labels| labels.iter().any(|l| l == "SENT"));
        let body = message["messageText"]
            .as_str()
            .or_else(|| message["preview"]["body"].as_str())
            .unwrap_or_default();
        emails.push(Email {
            id: id.to_owned(),
            received,
            from: text(&message["sender"]),
            to: text(&message["to"]),
            subject: text(&message["subject"]),
            sent,
            body: body.to_owned(),
        });
    }
    let next = data["nextPageToken"].as_str().map(str::to_owned);
    Ok(Page { emails, next })
}

fn parse_fastmail(value: &Value) -> Result<Page> {
    let results = value["results"]
        .as_array()
        .context("Fastmail returned no results list")?;
    let mut emails = Vec::new();
    for result in results {
        let Some(id) = result["id"].as_str() else {
            continue;
        };
        let Some(received) = result["receivedAt"].as_str().and_then(timestamp) else {
            log::debug!("mail: Fastmail email without a usable date; id={id}");
            continue;
        };
        let folders = std::iter::once(&result["folder"])
            .chain(result["labels"].as_array().into_iter().flatten());
        let sent = folders
            .into_iter()
            .any(|folder| folder["role"].as_str() == Some("sent"));
        emails.push(Email {
            id: id.to_owned(),
            received,
            from: addresses(&result["from"]),
            to: addresses(&result["to"]),
            subject: text(&result["subject"]),
            sent,
            body: text(&result["preview"]),
        });
    }
    let more = value["hasMore"].as_bool().unwrap_or(false);
    let next = value["nextCursor"]
        .as_str()
        .filter(|_| more)
        .map(str::to_owned);
    Ok(Page { emails, next })
}

/// Reads RFC 3339 dates and the Unix seconds or milliseconds some APIs use.
fn timestamp(text: &str) -> Option<DateTime<Utc>> {
    if let Ok(date) = DateTime::parse_from_rfc3339(text) {
        return Some(date.with_timezone(&Utc));
    }
    if let Ok(date) = DateTime::parse_from_rfc2822(text) {
        return Some(date.with_timezone(&Utc));
    }
    let number: i64 = text.parse().ok()?;
    if number > 100_000_000_000 {
        DateTime::from_timestamp_millis(number)
    } else {
        DateTime::from_timestamp(number, 0)
    }
}

fn text(value: &Value) -> String {
    value.as_str().unwrap_or_default().to_owned()
}

/// `Name <address>` for each entry of a JMAP-style address list, or the
/// value itself when it is already text.
fn addresses(value: &Value) -> String {
    let Some(list) = value.as_array() else {
        return text(value);
    };
    list.iter()
        .map(
            |entry| match (entry["name"].as_str(), entry["email"].as_str()) {
                (Some(name), Some(email)) if !name.is_empty() => format!("{name} <{email}>"),
                (_, Some(email)) => email.to_owned(),
                _ => text(entry),
            },
        )
        .collect::<Vec<_>>()
        .join(", ")
}

fn clip(text: &str, chars: usize) -> String {
    text.chars().take(chars).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_gmail_page() {
        let data = json!({
            "messages": [
                {
                    "messageId": "m1",
                    "messageTimestamp": "2026-03-01T10:00:00Z",
                    "sender": "Sam <sam@example.com>",
                    "to": "me@example.com",
                    "subject": "Dinner",
                    "messageText": "See you Friday",
                    "labelIds": ["INBOX"]
                },
                {
                    "messageId": "m2",
                    "messageTimestamp": "2026-02-01T10:00:00Z",
                    "subject": "Re: trip",
                    "messageText": "Booked",
                    "labelIds": ["SENT"]
                },
                {"messageId": "m3"}
            ],
            "nextPageToken": "next"
        });
        let page = parse_gmail(&data).unwrap();
        assert_eq!(page.emails.len(), 2);
        assert!(!page.emails[0].sent && page.emails[1].sent);
        assert_eq!(page.emails[0].body, "See you Friday");
        assert_eq!(page.next.as_deref(), Some("next"));
    }

    #[test]
    fn parses_fastmail_page() {
        let value = json!({
            "results": [{
                "id": "e1",
                "receivedAt": "2026-03-01T10:00:00Z",
                "from": [{"name": "Sam", "email": "sam@example.com"}],
                "to": [{"name": "", "email": "me@example.com"}],
                "subject": "Dinner",
                "preview": "See you",
                "folder": {"id": "f", "name": "Sent", "role": "sent"}
            }],
            "hasMore": false,
            "nextCursor": "ignored"
        });
        let page = parse_fastmail(&value).unwrap();
        let email = &page.emails[0];
        assert_eq!(email.from, "Sam <sam@example.com>");
        assert_eq!(email.to, "me@example.com");
        assert!(email.sent);
        assert!(page.next.is_none());
    }

    #[test]
    fn reads_dates_and_skips_seen() {
        assert!(timestamp("1767225600").is_some());
        assert_eq!(timestamp("1767225600000"), timestamp("1767225600"));
        assert!(timestamp("Thu, 1 Jan 2026 00:00:00 +0000").is_some());
        assert!(timestamp("soon").is_none());

        let email = |id: &str| Email {
            id: id.to_owned(),
            received: Utc::now(),
            from: String::new(),
            to: String::new(),
            subject: String::new(),
            sent: false,
            body: String::new(),
        };
        let seen = HashSet::from(["a".to_owned()]);
        let fresh = unseen(vec![email("a"), email("b"), email("b")], &seen);
        assert_eq!(fresh.len(), 1);
        assert_eq!(fresh[0].id, "b");
    }
}
