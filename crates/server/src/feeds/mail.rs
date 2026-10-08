//! Learns about users from their email.
//!
//! Once a day each connected mailbox is read back to a day before the last
//! finished run, or a month back until one finishes; the profile pass (see
//! `crate::profile`) reads the rest once. Emails are scored
//! page by page as they are listed, newest first where the mailbox lists them
//! that way, several at once; nothing waits for the whole listing. Jev scores
//! each one for whether it says something lasting about the user, and the
//! likely ones go to the memory learner one at a time, at most
//! `SCORES_PER_WINDOW` emails every `SCORE_WINDOW`. No agent decides what to read.
//!
//! Each processed email is recorded as seen, and a run skips those, so a run
//! that stops halfway resumes where it was. An email Jev fails to score stays
//! unseen and the run does not count as finished, so the next one tries it
//! again from the same point. Email content is never logged.
//!
//! While the user has the evening digest on, each recent email they did not
//! send also gets a second question: does it need their attention? Those
//! are kept for the digest (see `crate::digest`).

use crate::{
    app::App,
    apps::mcp::FASTMAIL,
    decision::{Criteria, DecisionTool},
    memories::{Reading, ReadingKind},
    store::{AttentionItem, Feed},
    user::User,
};
use anyhow::{Context, Result, bail};
use chrono::{DateTime, Duration as Days, SecondsFormat, Utc};
use flashmind::tools::composio::ComposioClient;
use futures::{FutureExt, Stream, StreamExt, TryStreamExt, future, stream};
use serde_json::{Value, json};
use std::{
    collections::HashSet,
    fmt::{self, Write},
    slice,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::time::{MissedTickBehavior, interval};

/// How far back the first run reads.
const BACKFILL_DAYS: i64 = 30;
/// Each run starts this long before the last finished one; seen ids skip the overlap.
const OVERLAP_DAYS: i64 = 1;
/// Jev calls, and Fastmail body reads, in flight at once.
const CONCURRENCY: usize = 32;
/// Emails one mailbox run scores per `SCORE_WINDOW`, so a full mailbox spreads out.
const SCORES_PER_WINDOW: u32 = 5000;
const SCORE_WINDOW: Duration = Duration::from_secs(5 * 60);
const PAGE_SIZE: usize = 50;
/// Stops a provider that keeps handing out next pages.
const MAX_PAGES: usize = 10_000;
/// What Jev sees of each email: headers and the start of the body.
const JEV_EMAIL_CHARS: usize = 2500;
/// What the learner reads of an email Jev passed.
const LEARN_EMAIL_CHARS: usize = 4000;
/// Line width for HTML bodies turned into text.
const HTML_TEXT_WIDTH: usize = 200;
/// The Outlook message fields a listing returns.
const OUTLOOK_FIELDS: &[&str] = &[
    "id",
    "receivedDateTime",
    "from",
    "toRecipients",
    "subject",
    "body",
    "bodyPreview",
    "parentFolderId",
    "isDraft",
    "conversationId",
];
/// Gmail's tabs for mail nobody wrote to the user in person.
const BULK_LABELS: [&str; 3] = ["CATEGORY_PROMOTIONS", "CATEGORY_SOCIAL", "CATEGORY_FORUMS"];
/// Above this Jev score the learner reads the email.
const MEMORY_THRESHOLD: f64 = 0.8;
const EMAIL_QUESTION: &str = "Does this email from the user's mailbox show a fact about the user still worth remembering today?";
/// Above this Jev score an email goes to the digest.
const ATTENTION_THRESHOLD: f64 = 0.7;
/// Only mail this recent is judged for the digest, so a backfill flags nothing old.
const ATTENTION_DAYS: i64 = 1;
/// What the digest shows of a flagged email.
const DIGEST_EMAIL_CHARS: usize = 1500;
const ATTENTION_QUESTION: &str = "Does this email need the user's own attention soon: a reply, a decision or an action from them?";
const ATTENTION_CRITERIA: Criteria = Criteria {
    yes: "A real person or organisation asking the user to reply, decide, pay, sign, confirm or \
show up; a deadline, or a time-sensitive problem with their accounts, money, travel or health",
    no: "Newsletters, marketing, social and automated notifications, receipts and confirmations \
that need nothing, FYIs, and anything the user only needs to know",
};
const EMAIL_CRITERIA: Criteria = Criteria {
    yes: "A lasting fact about the user: their name, people in their life, where they live or work, \
what they do, what they like, or a trip or appointment still ahead",
    no: "Newsletters, marketing, notifications, receipts, one-off errands and admin, events already over, \
or facts only about other people",
};

/// A mailbox the user can connect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Mailbox {
    Gmail,
    Fastmail,
    Outlook,
}

impl Mailbox {
    /// The app slug, also the key of its sync state.
    pub fn slug(self) -> &'static str {
        match self {
            Mailbox::Gmail => "gmail",
            Mailbox::Fastmail => FASTMAIL,
            Mailbox::Outlook => "outlook",
        }
    }

    /// How the user knows the mailbox.
    pub fn name(self) -> &'static str {
        match self {
            Mailbox::Gmail => "Gmail",
            Mailbox::Fastmail => "Fastmail",
            Mailbox::Outlook => "Outlook",
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
    /// Promotions, social or forum mail, as the mailbox files it.
    pub bulk: bool,
    /// The conversation it belongs to, when the mailbox says.
    pub thread: Option<String>,
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

    /// The email as the memory learner reads it.
    fn reading(&self) -> Reading {
        Reading {
            kind: ReadingKind::Email,
            id: self.id.clone(),
            query: format!("{}\n{}", self.subject, clip(&self.body, 300)),
            text: format!("Email:\n{}", self.render(LEARN_EMAIL_CHARS)),
        }
    }
}

/// One page of a mailbox listing.
struct Page {
    emails: Vec<Email>,
    next: Option<String>,
}

/// Which emails a listing returns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Listing {
    All,
    Unread,
}

impl Listing {
    /// The Gmail-style search term Gmail and Fastmail take for it.
    fn query_term(self) -> &'static str {
        match self {
            Listing::All => "",
            Listing::Unread => "is:unread ",
        }
    }
}

/// The stretch of time a listing covers.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Window {
    pub after: DateTime<Utc>,
    pub before: Option<DateTime<Utc>>,
}

/// Where a user's mail is read from.
pub(crate) enum Source<'a> {
    Gmail(Arc<ComposioClient>),
    Fastmail(&'a User),
    Outlook {
        client: Arc<ComposioClient>,
        /// The Sent Items folder, which marks mail the user sent.
        sent_folder: Option<String>,
    },
}

impl<'a> Source<'a> {
    pub(crate) async fn new(user: &'a User, mailbox: Mailbox) -> Result<Self> {
        let composio = || user.composio_client().context("Composio is not configured");
        Ok(match mailbox {
            Mailbox::Gmail => Source::Gmail(composio()?),
            Mailbox::Fastmail => Source::Fastmail(user),
            Mailbox::Outlook => {
                let client = composio()?;
                let sent_folder = outlook_sent_folder(&client).await;
                Source::Outlook {
                    client,
                    sent_folder,
                }
            }
        })
    }

    async fn page(&self, window: Window, listing: Listing, cursor: Option<&str>) -> Result<Page> {
        let Window { after, before } = window;
        match self {
            Source::Gmail(client) => {
                // Gmail reads Unix seconds after `after:`.
                let mut arguments = json!({
                    "query": format!(
                        "{}after:{}{}",
                        listing.query_term(),
                        after.timestamp(),
                        before.map(|b| format!(" before:{}", b.timestamp())).unwrap_or_default()
                    ),
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
            Source::Fastmail(user) => {
                // Fastmail only takes days; the overlap and seen ids cover the rest.
                let mut query =
                    format!("{}after:{}", listing.query_term(), after.format("%Y-%m-%d"));
                if let Some(before) = before {
                    let _ = write!(query, " before:{}", before.format("%Y-%m-%d"));
                }
                let mut arguments = json!({"query": query, "limit": PAGE_SIZE});
                if let Some(cursor) = cursor {
                    arguments["cursor"] = json!(cursor);
                }
                let value = user.call_mcp(FASTMAIL, "search_email", arguments).await?;
                parse_fastmail(&value)
            }
            Source::Outlook {
                client,
                sent_folder,
            } => {
                let mut arguments = json!({
                    "folder": "allfolders",
                    "received_date_time_ge": after.to_rfc3339_opts(SecondsFormat::Secs, true),
                    "top": PAGE_SIZE,
                    "select": OUTLOOK_FIELDS,
                });
                if let Some(before) = before {
                    arguments["received_date_time_lt"] =
                        json!(before.to_rfc3339_opts(SecondsFormat::Secs, true));
                }
                if listing == Listing::Unread {
                    arguments["is_read"] = json!(false);
                }
                if let Some(cursor) = cursor {
                    arguments["page_token"] = json!(cursor);
                }
                let response = client
                    .execute_tool("OUTLOOK_LIST_MESSAGES", arguments)
                    .await?;
                if response.successful == Some(false) {
                    bail!("Outlook listing failed");
                }
                parse_outlook(&response.data, sent_folder.as_deref())
            }
        }
    }

    /// Replaces a Fastmail preview with the whole body. On failure the
    /// preview stays and is scored as it is.
    pub(crate) async fn fill_body(&self, email: &mut Email) {
        let Source::Fastmail(user) = self else {
            return;
        };
        let arguments = json!({"id": email.id});
        match user.call_mcp(FASTMAIL, "read_email", arguments).await {
            Ok(value) => {
                if let Some(body) = value["bodyText"].as_str() {
                    email.body = body.to_owned();
                }
            }
            Err(error) => log::debug!("mail: unable to read a Fastmail body: {error:#}"),
        }
    }

    /// Pages of the emails received after `after`, in the mailbox's order,
    /// each fetched only when the one before it has been taken.
    fn pages(
        &self,
        window: Window,
        listing: Listing,
    ) -> impl Stream<Item = Result<Vec<Email>>> + '_ {
        // `None` once the last page is out; `Some(None)` before the first.
        let start: (Option<Option<String>>, usize) = (Some(None), 1);
        stream::try_unfold(start, move |(cursor, number)| async move {
            let Some(cursor) = cursor else {
                return Ok(None);
            };
            if number > MAX_PAGES {
                log::warn!("mail: stopped listing after {MAX_PAGES} pages");
                return Ok(None);
            }
            let page = self
                .page(window, listing, cursor.as_deref())
                .await
                .with_context(|| format!("page {number}"))?;
            log::debug!(
                "mail: listed page {number} with {} emails",
                page.emails.len()
            );
            let next = page.next.filter(|next| !next.is_empty()).map(Some);
            Ok(Some((page.emails, (next, number + 1))))
        })
    }

    /// Every email received after `after`.
    async fn list(&self, after: DateTime<Utc>, listing: Listing) -> Result<Vec<Email>> {
        let window = Window {
            after,
            before: None,
        };
        self.pages(window, listing).try_concat().await
    }

    /// Pages of every email in `window`, newest first where the mailbox
    /// lists them that way.
    pub(crate) fn history(&self, window: Window) -> impl Stream<Item = Result<Vec<Email>>> + '_ {
        self.pages(window, Listing::All)
    }
}

/// How a mailbox run went.
#[derive(Debug, Default)]
pub(crate) struct Outcome {
    listed: usize,
    scored: usize,
    failed: usize,
    passed: usize,
    learned: usize,
    flagged: usize,
}

impl fmt::Display for Outcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Outcome {
            listed,
            scored,
            failed,
            passed,
            learned,
            flagged,
        } = self;
        write!(
            f,
            "listed={listed} scored={scored} failed={failed} passed={passed} learned={learned} flagged={flagged}"
        )
    }
}

pub(crate) async fn sync(
    app: &App,
    decision: &DecisionTool,
    user: &User,
    mailbox: Mailbox,
) -> Result<Outcome> {
    let source = Source::new(user, mailbox).await?;
    let feed = Feed::Mail(mailbox);
    let after = match user.feed_synced_at(feed).await? {
        Some(synced) => synced - Days::days(OVERLAP_DAYS),
        None => Utc::now() - Days::days(BACKFILL_DAYS),
    };
    user.prune_feed_seen(feed, after - Days::days(OVERLAP_DAYS))
        .await?;
    let mut seen = user.feed_seen(feed).await?;
    let digest = user.digest_enabled().await?;
    log::info!(
        "mail: reading {} after {after}; user={user}",
        mailbox.slug()
    );

    // Each page is scored as soon as it is listed, several emails at once,
    // each waiting for a tick. `seen` also drops an email a page repeats.
    let listed = AtomicUsize::new(0);
    let mut every = interval(SCORE_WINDOW / SCORES_PER_WINDOW);
    every.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let ticks = stream::unfold(every, |mut every| async move {
        every.tick().await;
        Some(((), every))
    });
    let today = Utc::now().format("%A %Y-%m-%d").to_string();
    let mut scored = source
        .pages(
            Window {
                after,
                before: None,
            },
            Listing::All,
        )
        .map_ok(|emails| stream::iter(emails).map(anyhow::Ok))
        .try_flatten()
        .inspect_ok(|_| {
            listed.fetch_add(1, Ordering::Relaxed);
        })
        .try_filter(move |email| future::ready(seen.insert(email.id.clone())))
        .zip(ticks)
        .map(|(email, ())| email)
        .map_ok(|email| score(decision, &source, &today, email, digest).map(Ok))
        .try_buffer_unordered(CONCURRENCY)
        .boxed();
    let mut outcome = Outcome::default();
    let generator = app.generator(user.id());
    while let Some((email, scores)) = scored.try_next().await? {
        // Left unseen, so the next run tries it again.
        let probability = match scores.memory {
            Ok(probability) => probability,
            Err(error) => {
                outcome.failed += 1;
                log::debug!("mail: not scored: {error:#}; user={user} id={}", email.id);
                continue;
            }
        };
        log::debug!(
            "mail: scored {probability:.2} attention={:?}; user={user} id={}",
            scores.attention,
            email.id
        );
        outcome.scored += 1;
        if probability >= MEMORY_THRESHOLD {
            outcome.passed += 1;
            outcome.learned += user.learn_reading(generator, &email.reading()).await.len();
        }
        if scores.attention.is_some_and(|p| p >= ATTENTION_THRESHOLD) {
            outcome.flagged += 1;
            user.flag_attention(&AttentionItem {
                mailbox,
                message_id: email.id.clone(),
                received: email.received,
                text: email.render(DIGEST_EMAIL_CHARS),
            })
            .await?;
        }
        user.mark_feed_seen(feed, &email.id, email.received).await?;
    }
    drop(scored);
    outcome.listed = listed.into_inner();
    if outcome.failed > 0 {
        bail!("Jev could not score {} emails; {outcome}", outcome.failed);
    }
    Ok(outcome)
}

/// What Jev made of one email.
struct Scores {
    /// How likely it teaches a lasting fact.
    memory: Result<f64>,
    /// How likely it needs the user's attention, when that was asked.
    attention: Option<f64>,
}

/// Fetches the body and asks Jev how likely the email is to teach a lasting
/// fact and, for the digest, whether it needs the user's attention.
async fn score(
    decision: &DecisionTool,
    source: &Source<'_>,
    today: &str,
    mut email: Email,
    digest: bool,
) -> (Email, Scores) {
    source.fill_body(&mut email).await;
    let state = format!("Today: {today}\n{}", email.render(JEV_EMAIL_CHARS));
    let memory = decision.probability_with(&state, EMAIL_QUESTION, &EMAIL_CRITERIA);
    let recent = Utc::now() - Days::days(ATTENTION_DAYS);
    let attention = async {
        if !digest || email.sent || email.received < recent {
            return None;
        }
        decision
            .probability_with(&state, ATTENTION_QUESTION, &ATTENTION_CRITERIA)
            .await
            .inspect_err(|error| log::debug!("mail: attention not scored: {error:#}"))
            .ok()
    };
    let (memory, attention) = futures::join!(memory, attention);
    (email, Scores { memory, attention })
}

/// Ids of the unread emails of `mailbox` received after `after`.
pub(crate) async fn unread_since(
    user: &User,
    mailbox: Mailbox,
    after: DateTime<Utc>,
) -> Result<HashSet<String>> {
    let source = Source::new(user, mailbox).await?;
    let unread = source.list(after, Listing::Unread).await?;
    Ok(unread.into_iter().map(|email| email.id).collect())
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
        let has = |label: &str| labels.is_some_and(|labels| labels.iter().any(|l| l == label));
        let sent = has("SENT");
        let bulk = BULK_LABELS.iter().any(|label| has(label));
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
            bulk,
            thread: message["threadId"].as_str().map(str::to_owned),
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
            bulk: false,
            thread: result["threadId"].as_str().map(str::to_owned),
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

/// The id of the Sent Items folder, read from the newest sent message. Without
/// it Outlook mail is still read, only not marked as sent by the user.
async fn outlook_sent_folder(client: &ComposioClient) -> Option<String> {
    let arguments = json!({"folder": "sentitems", "top": 1, "select": ["parentFolderId"]});
    match client
        .execute_tool("OUTLOOK_LIST_MESSAGES", arguments)
        .await
    {
        Ok(response) if response.successful != Some(false) => {
            response.data["value"][0]["parentFolderId"]
                .as_str()
                .map(str::to_owned)
        }
        Ok(_) => {
            log::debug!("mail: unable to find the Outlook Sent Items folder");
            None
        }
        Err(error) => {
            log::debug!("mail: unable to find the Outlook Sent Items folder: {error:#}");
            None
        }
    }
}

fn parse_outlook(data: &Value, sent_folder: Option<&str>) -> Result<Page> {
    let messages = data["value"]
        .as_array()
        .context("Outlook returned no value list")?;
    let mut emails = Vec::new();
    for message in messages {
        let Some(id) = message["id"].as_str() else {
            continue;
        };
        if message["isDraft"].as_bool() == Some(true) {
            continue;
        }
        let Some(received) = message["receivedDateTime"].as_str().and_then(timestamp) else {
            log::debug!("mail: Outlook message without a usable date; id={id}");
            continue;
        };
        let sent = sent_folder.is_some_and(|folder| message["parentFolderId"] == folder);
        emails.push(Email {
            id: id.to_owned(),
            received,
            from: recipients(&message["from"]),
            to: recipients(&message["toRecipients"]),
            subject: text(&message["subject"]),
            sent,
            bulk: false,
            thread: message["conversationId"].as_str().map(str::to_owned),
            body: outlook_body(message),
        });
    }
    let next = data["next_page_token"].as_str().map(str::to_owned);
    Ok(Page { emails, next })
}

/// The body as plain text, or the preview when there is no body.
fn outlook_body(message: &Value) -> String {
    let body = &message["body"];
    let content = body["content"].as_str().unwrap_or_default();
    if content.trim().is_empty() {
        return text(&message["bodyPreview"]);
    }
    if !body["contentType"]
        .as_str()
        .is_some_and(|kind| kind.eq_ignore_ascii_case("html"))
    {
        return content.to_owned();
    }
    html2text::config::plain_no_decorate()
        .raw_mode(true)
        .string_from_read(content.as_bytes(), HTML_TEXT_WIDTH)
        .unwrap_or_else(|_| text(&message["bodyPreview"]))
}

/// `Name <address>` for one Microsoft Graph recipient or a list of them.
fn recipients(value: &Value) -> String {
    let list = match value.as_array() {
        Some(list) => list.as_slice(),
        None => slice::from_ref(value),
    };
    list.iter()
        .filter_map(|recipient| {
            let address = &recipient["emailAddress"];
            let email = address["address"].as_str()?;
            Some(match address["name"].as_str() {
                Some(name) if !name.is_empty() && name != email => format!("{name} <{email}>"),
                _ => email.to_owned(),
            })
        })
        .collect::<Vec<_>>()
        .join(", ")
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

pub(crate) fn clip(text: &str, chars: usize) -> String {
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
    fn parses_outlook_page() {
        let data = json!({
            "value": [
                {
                    "id": "o1",
                    "receivedDateTime": "2026-03-01T10:00:00Z",
                    "from": {"emailAddress": {"name": "Sam", "address": "sam@example.com"}},
                    "toRecipients": [
                        {"emailAddress": {"name": "me@example.com", "address": "me@example.com"}},
                        {"emailAddress": {"name": "Alex", "address": "alex@example.com"}}
                    ],
                    "subject": "Dinner",
                    "body": {"contentType": "html", "content": "<html><body><p>See you <b>Friday</b></p></body></html>"},
                    "parentFolderId": "inbox-id",
                    "isDraft": false
                },
                {
                    "id": "o2",
                    "receivedDateTime": "2026-03-02T10:00:00Z",
                    "subject": "Re: trip",
                    "body": {"contentType": "text", "content": "Booked"},
                    "parentFolderId": "sent-id"
                },
                {"id": "o3", "receivedDateTime": "2026-03-03T10:00:00Z", "isDraft": true},
                {"id": "o4"}
            ],
            "next_page_token": "next"
        });
        let page = parse_outlook(&data, Some("sent-id")).unwrap();
        assert_eq!(page.emails.len(), 2);
        let (first, second) = (&page.emails[0], &page.emails[1]);
        assert_eq!(first.from, "Sam <sam@example.com>");
        assert_eq!(first.to, "me@example.com, Alex <alex@example.com>");
        assert_eq!(first.body.trim(), "See you Friday");
        assert!(!first.sent && second.sent);
        assert_eq!(second.body, "Booked");
        assert_eq!(page.next.as_deref(), Some("next"));
    }

    #[test]
    fn reads_dates_and_skips_seen() {
        assert!(timestamp("1767225600").is_some());
        assert_eq!(timestamp("1767225600000"), timestamp("1767225600"));
        assert!(timestamp("Thu, 1 Jan 2026 00:00:00 +0000").is_some());
        assert!(timestamp("soon").is_none());
    }
}
