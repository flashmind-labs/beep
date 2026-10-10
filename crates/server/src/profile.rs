//! The profile pass: once per connected mailbox and calendar, Beep reads
//! everything it can reach and builds a profile of the user from it.
//!
//! Mail is read a year at a time, newest first, back to the first empty
//! years. Newsletters and promotions only feed counts. Jev drops the other
//! received mail that is plainly not about the user. Threads the user wrote
//! in, repeated senders (a few samples and every subject) and the remaining
//! emails go to the cheap model in batches, which writes short dated notes.
//! Calendars are read once, five years back and half a year ahead, with each
//! repeating event shown once with its count. Notes are stored as each year
//! finishes, so a pass cut short resumes at the year it was in.
//!
//! When no source of the user is still being read, the notes are condensed
//! into dossiers and then into facts, saved as memories with the evidence
//! behind each, and the user gets a notice. Content is never logged.

use crate::{
    app::{App, UserNotice},
    apps::mcp::FASTMAIL,
    decision::DecisionTool,
    feeds::mail::{
        Email, Mailbox, Source as MailSource, Window, clip, worth_learning, worth_reading,
    },
    protocol::{MemorySource, Notice},
    starters::{Generator, unfence},
    store::{Fact, Origin, ProfileNote, Validity},
    user::User,
};
use anyhow::{Context, Result, bail};
use chrono::{DateTime, Datelike, Duration as Days, NaiveDate, SecondsFormat, Utc};
use futures::{StreamExt, TryStreamExt, future, stream};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    cmp::Reverse,
    collections::{BTreeMap, HashMap, HashSet},
    fmt::{self, Write},
    mem,
    sync::{Arc, LazyLock, Mutex, MutexGuard, PoisonError},
};

/// Mail older than this is not looked for.
const OLDEST_YEAR: i32 = 1995;
/// Empty years in a row that end the mail read.
const EMPTY_YEARS: u32 = 3;
/// Most text from one year of mail the model reads.
const YEAR_CHARS: usize = 480_000;
/// Text per extraction call.
const BATCH_CHARS: usize = 16_000;
/// Extraction calls at once.
const CONCURRENCY: usize = 8;
/// Jev calls at once while screening a year of mail.
const SCREEN_CONCURRENCY: usize = 16;
/// Emails from one sender past which they are summed up, not read one by one.
const DIGEST_AFTER: usize = 10;
/// Emails of a repeated sender read in full.
const DIGEST_SAMPLES: usize = 5;
/// Subject lines a digest lists at most.
const DIGEST_SUBJECTS: usize = 80;
/// Body kept of an email the user sent, and of one anyone else did.
const SENT_CHARS: usize = 3000;
const RECEIVED_CHARS: usize = 1200;
const SAMPLE_CHARS: usize = 700;
/// Longest thread text.
const THREAD_CHARS: usize = 12_000;
/// People and senders a yearly count note names.
const TOP_COUNT: usize = 15;
/// How far the calendar read reaches back and ahead.
const CALENDAR_BACK_DAYS: i64 = 5 * 365;
const CALENDAR_AHEAD_DAYS: i64 = 183;
const CALENDAR_PAGES: usize = 200;
/// How much of the calendar one Fastmail search covers.
const FASTMAIL_SPAN_DAYS: i64 = 90;
/// Notes per condensing call, and the facts it may end with.
const CONDENSE_CHARS: usize = 60_000;
const MAX_FACTS: usize = 80;
/// Answer room for a dossier and for the final facts.
const DOSSIER_TOKENS: u32 = 6000;
const FACTS_TOKENS: u32 = 12_000;
/// Mail from these senders is skipped: dating apps and medical portals.
const SKIPPED_SENDERS: &[&str] = &[
    "tinder",
    "bumble",
    "hinge.co",
    "grindr",
    "okcupid",
    "match.com",
    "badoo",
    "happn",
    "feeld",
    "mychart",
    "patient",
    "clinic",
    "hospital",
    "pharmacy",
    "doctolib",
    "zocdoc",
    "healthcare",
];

const NOTE_RULES: &str = "Note anything that tells who the user is and how they live: their name, \
languages, where they live and lived, work, employers, colleagues, studies, the people in their life and how \
they relate, purchases (shop, item, brand, size, color, price range), bookings and trips (where, when, with \
whom), subscriptions and services, hobbies, sports and fitness, food and diet, events they go to, habits and \
routines, tastes, homes and property, health and insurance, beliefs, religion and politics, money: banks, \
investments and debts, residency and visas. Be specific and keep names, places, brands and dates. A repeated \
thing is worth one note with how often. A newsletter or promotion shows at most a subscription, worth a note \
only when it says something about the user's interests or views. Never note passwords, codes, tokens, or card, \
account, contract or ID numbers. Only what the text shows about the user, never guesses (nothing \"likely\" \
or \"probably\"). Each note one short sentence in English about the user in third person.
Reply with only JSON: {\"notes\": [{\"date\": \"YYYY-MM-DD\", \"note\": string}]}. date is the day the \
note is about, left out when there is none. At most 60 notes; {\"notes\": []} when nothing is worth it.";

const MAIL_SYSTEM: &str = "You help a personal assistant app get to know its user. You get part of one \
year of the user's own mailbox: threads they wrote in, summaries of senders who write often, and single \
emails. Mail marked as sent was written by the user.";

const CALENDAR_SYSTEM: &str = "You help a personal assistant app get to know its user. You get events \
from the user's own calendar. A repeating event is listed once with how many times it happened and between \
which days.";

const DOSSIER_SYSTEM: &str = "You help a personal assistant app get to know its user. You get notes \
taken from their mail and calendar over the years, plus counts, oldest first. Merge them into a dossier on \
the user: identity and languages, home over time, work over time, people and how they relate (and who \
appears in both mail and calendar), shopping (shops, brands, sizes, colors), travel, subscriptions, hobbies, \
fitness, food, routines and tastes, homes and property, health and insurance, beliefs, religion and politics, \
money (banks, investments, debts), residency and visas. Keep counts, names, places and dates, and a timeline \
wherever something changed. Drop repeats. Plain text, at most 1500 words.";

const FACTS_SYSTEM: &str = "You keep the long-term memory of a personal assistant app. You get dossiers \
on the user built from their mail and calendar, and the memories already kept. Write the facts most worth \
knowing to help the user day to day: who they are, where they live, their work, the people close to them, \
shops, brands, sizes and colors they buy, trips, subscriptions, hobbies, routines and tastes, homes and \
property, health, beliefs, religion and politics, money (banks, investments, debts), residency and visas. Each fact is one \
short self-contained sentence in third person that names its subject, like \"Buys running shoes from \
Decathlon, size 43.\" Where something changed over time (a job, a home), only the current one is a lasting \
fact; a past one may be kept with valid_until. Skip what a kept memory already says. Never write a secret or a number that gives access, or a \
card, account, contract or ID number.
Reply with only JSON: {\"facts\": [{\"content\": string, \"valid_from\": \"YYYY-MM-DD\", \"valid_until\": \
\"YYYY-MM-DD\", \"evidence\": string}]}. Days only for facts that hold for a while. evidence says in at most \
25 words what in the dossier shows it. At most 80 facts.";

/// One thing the pass reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Source {
    Mail(Mailbox),
    Calendar(Calendar),
}

/// A calendar the user can connect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Calendar {
    Google,
    Outlook,
    Fastmail,
}

impl Source {
    /// The key of its read in the store.
    pub fn key(self) -> &'static str {
        match self {
            Source::Mail(mailbox) => mailbox.slug(),
            Source::Calendar(Calendar::Google) => "googlecalendar",
            Source::Calendar(Calendar::Outlook) => "outlook-calendar",
            Source::Calendar(Calendar::Fastmail) => "fastmail-calendar",
        }
    }

    /// What connecting the app `slug` lets the pass read.
    pub fn from_slug(slug: &str) -> &'static [Source] {
        match slug {
            "gmail" => &[Source::Mail(Mailbox::Gmail)],
            "googlecalendar" => &[Source::Calendar(Calendar::Google)],
            "outlook" => &[
                Source::Mail(Mailbox::Outlook),
                Source::Calendar(Calendar::Outlook),
            ],
            FASTMAIL => &[
                Source::Mail(Mailbox::Fastmail),
                Source::Calendar(Calendar::Fastmail),
            ],
            _ => &[],
        }
    }
}

impl fmt::Display for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.key())
    }
}

/// Sources being read, by user, so a second request leaves them alone.
static RUNNING: LazyLock<Mutex<HashSet<(String, Source)>>> = LazyLock::new(Mutex::default);

/// A source being read; dropping it frees the source.
struct Claim {
    user: String,
    source: Source,
}

impl Claim {
    fn take(user: &User, source: Source) -> Option<Self> {
        let user = user.id().to_owned();
        let inserted = running().insert((user.clone(), source));
        inserted.then_some(Claim { user, source })
    }
}

impl Drop for Claim {
    fn drop(&mut self) {
        running().remove(&(mem::take(&mut self.user), self.source));
    }
}

fn running() -> MutexGuard<'static, HashSet<(String, Source)>> {
    RUNNING.lock().unwrap_or_else(PoisonError::into_inner)
}

fn others_running(user: &User) -> bool {
    running().iter().any(|(id, _)| id == user.id())
}

/// The sources behind the connected apps `slugs`.
fn sources(slugs: &[String]) -> Vec<Source> {
    slugs
        .iter()
        .flat_map(|slug| Source::from_slug(slug))
        .copied()
        .collect()
}

/// Reads, in the background, each source behind the connected apps `slugs`
/// that has not been read yet.
pub fn spawn(app: &Arc<App>, user: &Arc<User>, slugs: &[String]) {
    for source in sources(slugs) {
        let Some(claim) = Claim::take(user, source) else {
            continue;
        };
        tokio::spawn(read(app.clone(), user.clone(), source, claim));
    }
}

/// Reads each source behind the connected apps `slugs` again from the
/// start, dropping its earlier notes. Memories already made stay. Returns
/// the sources started; one being read right now is left alone.
pub async fn reread(app: &Arc<App>, user: &Arc<User>, slugs: &[String]) -> Result<Vec<Source>> {
    let mut started = Vec::new();
    for source in sources(slugs) {
        let Some(claim) = Claim::take(user, source) else {
            continue;
        };
        user.reset_profile_read(source.key()).await?;
        tokio::spawn(read(app.clone(), user.clone(), source, claim));
        started.push(source);
    }
    Ok(started)
}

async fn read(app: Arc<App>, user: Arc<User>, source: Source, claim: Claim) {
    match user.profile_read(source.key()).await {
        Ok(read) if read.finished => return,
        Ok(_) => {}
        Err(error) => {
            log::warn!("profile: unable to read state: {error:#}; user={user}");
            return;
        }
    }
    log::info!("profile: reading {source}; user={user}");
    let generator = app.generator(user.id());
    let mut summary = Summary::default();
    let read = match source {
        Source::Mail(mailbox) => {
            read_mail(generator, app.decision(), &user, mailbox, &mut summary).await
        }
        Source::Calendar(calendar) => read_calendar(generator, &user, calendar, &mut summary).await,
    };
    if let Err(error) = read {
        // Not finished, so the next listing of apps resumes it. What it noted
        // so far is condensed now, or it would wait for a read that may keep
        // stopping.
        log::warn!("profile: read of {source} stopped: {error:#}; user={user} {summary}");
        drop(claim);
        if summary.notes > 0 && !others_running(&user) {
            match condense(generator, &user).await {
                Ok(learned) => log::info!("profile: condensed; user={user} learned={learned}"),
                Err(error) => log::warn!("profile: condensing stopped: {error:#}; user={user}"),
            }
        }
        return;
    }
    if let Err(error) = user.finish_profile_read(source.key()).await {
        log::warn!("profile: unable to save state: {error:#}; user={user}");
        return;
    }
    log::info!("profile: read {source}; user={user} {summary}");
    drop(claim);
    if others_running(&user) {
        return;
    }
    match condense(generator, &user).await {
        Ok(learned) => notify(&user, source, &summary, learned),
        Err(error) => log::warn!("profile: condensing stopped: {error:#}; user={user}"),
    }
}

/// What one source gave.
#[derive(Debug, Default)]
struct Summary {
    years: u32,
    items: usize,
    notes: usize,
}

impl fmt::Display for Summary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "years={} items={} notes={}",
            self.years, self.items, self.notes
        )
    }
}

fn notify(user: &User, source: Source, summary: &Summary, learned: usize) {
    let read = match source {
        Source::Mail(_) if summary.years == 1 => "a year of your mail".to_owned(),
        Source::Mail(_) => format!("{} years of your mail", summary.years),
        Source::Calendar(_) => "your calendar".to_owned(),
    };
    let notice = UserNotice {
        user: user.id().to_owned(),
        notice: Notice {
            title: "Beep got to know you".to_owned(),
            body: format!("Read {read} and learned {learned} things. See Memories."),
            session_id: String::new(),
            app: None,
            approval: None,
            changed: false,
            group: false,
        },
    };
    // Nobody listening means the phone is offline; Memories still shows them.
    let _ = user.notifications.send(notice);
}

// Mail.

async fn read_mail(
    generator: Generator<'_>,
    decision: Option<&DecisionTool>,
    user: &User,
    mailbox: Mailbox,
    summary: &mut Summary,
) -> Result<()> {
    let source = MailSource::new(user, mailbox).await?;
    let key = Source::Mail(mailbox).key();
    let mut before = user
        .profile_read(key)
        .await?
        .read_before
        .unwrap_or_else(Utc::now);
    let mut empty = 0;
    while empty < EMPTY_YEARS && before.year() > OLDEST_YEAR {
        let after = before - Days::days(365);
        let window = Window {
            after,
            before: Some(before),
        };
        let year = read_year(generator, decision, &source, window).await?;
        if year.emails == 0 {
            empty += 1;
        } else {
            empty = 0;
            summary.years += 1;
        }
        user.add_profile_notes(key, &year.notes, Some(after))
            .await?;
        summary.items += year.emails;
        summary.notes += year.notes.len();
        log::debug!(
            "profile: read mail before {before}; emails={} notes={}",
            year.emails,
            year.notes.len()
        );
        before = after;
    }
    Ok(())
}

/// What a year of mail gave.
struct Year {
    emails: usize,
    notes: Vec<ProfileNote>,
}

async fn read_year(
    generator: Generator<'_>,
    decision: Option<&DecisionTool>,
    source: &MailSource<'_>,
    window: Window,
) -> Result<Year> {
    let mut emails: Vec<Email> = Vec::new();
    let mut ids = HashSet::new();
    let mut pages = source.history(window).boxed();
    while let Some(page) = pages.try_next().await? {
        for mut email in page {
            let inside = email.received >= window.after
                && window.before.is_none_or(|before| email.received < before);
            if !inside || !ids.insert(email.id.clone()) {
                continue;
            }
            // Kept short while listing; a year can hold tens of thousands.
            let keep = if email.bulk { 0 } else { SENT_CHARS };
            email.body = clip(email.body.trim(), keep);
            emails.push(email);
        }
    }
    drop(pages);
    let count = emails.len();
    if count == 0 {
        return Ok(Year {
            emails: 0,
            notes: Vec::new(),
        });
    }
    let year = window.after.date_naive();
    let mut notes = vec![counts_note(year, &emails)];
    emails.retain(|email| !email.bulk && !skipped(&email.from));
    if let Some(decision) = decision {
        emails = screen(decision, emails).await;
    }
    let mut groups = group_mail(emails);
    fill_bodies(source, &mut groups).await;
    let texts: Vec<String> = groups.iter().map(Group::render).collect();
    let header = format!(
        "Mail from {} to {}.",
        window.after.format("%Y-%m-%d"),
        window.before.unwrap_or_else(Utc::now).format("%Y-%m-%d")
    );
    notes.extend(extract(generator, MAIL_SYSTEM, &header, &texts).await?);
    Ok(Year {
        emails: count,
        notes,
    })
}

/// Keeps mail the user wrote, the threads they wrote in, and received mail
/// Jev finds worth reading. Mailboxes without a bulk flag, like Fastmail,
/// rely on this to drop newsletters. Mail Jev fails on is kept.
async fn screen(decision: &DecisionTool, emails: Vec<Email>) -> Vec<Email> {
    let written: HashSet<String> = emails
        .iter()
        .filter(|email| email.sent)
        .filter_map(|email| email.thread.clone())
        .collect();
    let today = Utc::now().format("%A %Y-%m-%d").to_string();
    let count = emails.len();
    let kept: Vec<Email> = stream::iter(emails)
        .map(|email| {
            let today = &today;
            let mine = email.sent || email.thread.as_ref().is_some_and(|t| written.contains(t));
            async move {
                if mine {
                    return Some(email);
                }
                match worth_reading(decision, today, &email).await {
                    Ok(probability) if !worth_learning(probability) => None,
                    Ok(_) => Some(email),
                    Err(error) => {
                        log::debug!("profile: not screened: {error:#}");
                        Some(email)
                    }
                }
            }
        })
        .buffer_unordered(SCREEN_CONCURRENCY)
        .filter_map(future::ready)
        .collect()
        .await;
    log::debug!("profile: screened mail; kept={} of={count}", kept.len());
    kept
}

/// Mail the model reads together.
enum Group {
    /// A conversation the user wrote in, oldest first.
    Thread(Vec<Email>),
    /// A sender who wrote often: a few emails and the rest's subjects.
    Digest {
        sender: String,
        count: usize,
        samples: Vec<Email>,
        subjects: Vec<(NaiveDate, String)>,
    },
    Single(Email),
}

impl Group {
    /// Text the model reads, a rough size before bodies are filled.
    fn size(&self) -> usize {
        match self {
            Group::Thread(emails) => emails
                .iter()
                .map(|email| body_chars(email) + 200)
                .sum::<usize>()
                .min(THREAD_CHARS),
            Group::Digest {
                samples, subjects, ..
            } => samples.len() * (SAMPLE_CHARS + 200) + subjects.len() * 60,
            Group::Single(email) => body_chars(email) + 200,
        }
    }

    fn emails_mut(&mut self) -> Vec<&mut Email> {
        match self {
            Group::Thread(emails) => emails.iter_mut().collect(),
            Group::Digest { samples, .. } => samples.iter_mut().collect(),
            Group::Single(email) => vec![email],
        }
    }

    fn render(&self) -> String {
        let mut text = String::new();
        match self {
            Group::Thread(emails) => {
                let _ = writeln!(text, "Thread of {} emails:", emails.len());
                for email in emails {
                    let _ = writeln!(text, "---\n{}", email.render(body_chars(email)));
                }
                text = clip(&text, THREAD_CHARS);
            }
            Group::Digest {
                sender,
                count,
                samples,
                subjects,
            } => {
                let _ = writeln!(text, "{count} emails from {sender}. A few of them:");
                for email in samples {
                    let _ = writeln!(text, "---\n{}", email.render(SAMPLE_CHARS));
                }
                let _ = writeln!(text, "---\nAll their subjects:");
                for (day, subject) in subjects {
                    let _ = writeln!(text, "{day} {}", clip(subject, 120));
                }
            }
            Group::Single(email) => {
                let _ = writeln!(text, "Email:\n{}", email.render(body_chars(email)));
            }
        }
        text
    }
}

fn body_chars(email: &Email) -> usize {
    if email.sent {
        SENT_CHARS
    } else {
        RECEIVED_CHARS
    }
}

/// Threads the user wrote in first, then repeated senders, then the rest,
/// cut to what one year may send to the model.
fn group_mail(emails: Vec<Email>) -> Vec<Group> {
    let mut threads: HashMap<String, Vec<Email>> = HashMap::new();
    let mut loose = Vec::new();
    for email in emails {
        match email.thread.clone() {
            Some(thread) => threads.entry(thread).or_default().push(email),
            None => loose.push(email),
        }
    }
    let mut groups = Vec::new();
    let mut written: Vec<Vec<Email>> = Vec::new();
    for (_, mut thread) in threads {
        if thread.iter().any(|email| email.sent) {
            thread.sort_by_key(|email| email.received);
            written.push(thread);
        } else {
            loose.extend(thread);
        }
    }
    // Longest conversations first: those are the people who matter most.
    written.sort_by_key(|thread| Reverse(thread.len()));
    groups.extend(written.into_iter().map(Group::Thread));

    let mut senders: BTreeMap<String, Vec<Email>> = BTreeMap::new();
    for email in loose {
        senders.entry(address(&email.from)).or_default().push(email);
    }
    let mut digests = Vec::new();
    let mut singles = Vec::new();
    for (sender, mut emails) in senders {
        if emails.len() <= DIGEST_AFTER {
            singles.extend(emails.into_iter().map(Group::Single));
            continue;
        }
        emails.sort_by_key(|email| email.received);
        let count = emails.len();
        let subjects = emails
            .iter()
            .rev()
            .take(DIGEST_SUBJECTS)
            .rev()
            .map(|email| (email.received.date_naive(), email.subject.clone()))
            .collect();
        let step = count / DIGEST_SAMPLES;
        let samples = emails
            .into_iter()
            .step_by(step.max(1))
            .take(DIGEST_SAMPLES)
            .collect();
        digests.push(Group::Digest {
            sender,
            count,
            samples,
            subjects,
        });
    }
    // Mail the user sent first among the singles.
    singles.sort_by_key(|group| match group {
        Group::Single(email) => !email.sent,
        _ => true,
    });
    groups.extend(digests);
    groups.extend(singles);

    let mut total = 0;
    groups
        .into_iter()
        .take_while(|group| {
            total += group.size();
            total <= YEAR_CHARS
        })
        .collect()
}

/// Reads whole bodies where the listing only gave previews.
async fn fill_bodies(source: &MailSource<'_>, groups: &mut [Group]) {
    let emails: Vec<&mut Email> = groups.iter_mut().flat_map(Group::emails_mut).collect();
    stream::iter(emails)
        .for_each_concurrent(CONCURRENCY, |email| async move {
            source.fill_body(email).await;
            email.body = clip(email.body.trim(), SENT_CHARS);
        })
        .await;
}

/// A year's totals: who the user writes to, who writes to them, and what
/// they are subscribed to.
fn counts_note(year: NaiveDate, emails: &[Email]) -> ProfileNote {
    let mut to: HashMap<String, usize> = HashMap::new();
    let mut from: HashMap<String, usize> = HashMap::new();
    let mut bulk: HashMap<String, usize> = HashMap::new();
    let mut sent = 0;
    for email in emails {
        if email.sent {
            sent += 1;
            for recipient in email.to.split(',') {
                let recipient = recipient.trim();
                if !recipient.is_empty() {
                    *to.entry(recipient.to_owned()).or_default() += 1;
                }
            }
        } else if email.bulk {
            *bulk.entry(sender_name(&email.from)).or_default() += 1;
        } else {
            *from.entry(email.from.trim().to_owned()).or_default() += 1;
        }
    }
    let mut text = format!(
        "Mail in the year from {year}: {} emails, {sent} sent by the user.",
        emails.len()
    );
    for (label, counts) in [
        ("Writes most to", to),
        ("Gets most personal mail from", from),
        ("Gets most newsletters and promotions from", bulk),
    ] {
        if !counts.is_empty() {
            let _ = write!(text, " {label}: {}.", top(counts));
        }
    }
    ProfileNote { date: None, text }
}

fn top(counts: HashMap<String, usize>) -> String {
    let mut counts: Vec<_> = counts.into_iter().collect();
    counts.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    counts
        .into_iter()
        .take(TOP_COUNT)
        .map(|(name, count)| format!("{name} ({count})"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The address in `Name <address>`, lowercased.
fn address(from: &str) -> String {
    let from = from.trim();
    let inner = match (from.rfind('<'), from.rfind('>')) {
        (Some(start), Some(end)) if start < end => &from[start + 1..end],
        _ => from,
    };
    inner.trim().to_lowercase()
}

/// The name in `Name <address>`, or the address's domain.
fn sender_name(from: &str) -> String {
    match from.trim().split_once('<') {
        Some((name, _)) if !name.trim().is_empty() => name.trim().trim_matches('"').to_owned(),
        _ => {
            let address = address(from);
            address.rsplit('@').next().unwrap_or(&address).to_owned()
        }
    }
}

fn skipped(from: &str) -> bool {
    let from = from.to_lowercase();
    SKIPPED_SENDERS.iter().any(|sender| from.contains(sender))
}

// Calendars.

/// One event as the pass reads it.
#[derive(Debug, Clone, PartialEq)]
struct Event {
    title: String,
    day: NaiveDate,
    location: String,
    people: Vec<String>,
}

async fn read_calendar(
    generator: Generator<'_>,
    user: &User,
    calendar: Calendar,
    summary: &mut Summary,
) -> Result<()> {
    let now = Utc::now();
    let window = Window {
        after: now - Days::days(CALENDAR_BACK_DAYS),
        before: Some(now + Days::days(CALENDAR_AHEAD_DAYS)),
    };
    let events = match calendar {
        Calendar::Google => google_events(user, window).await?,
        Calendar::Outlook => outlook_events(user, window).await?,
        Calendar::Fastmail => fastmail_events(user, window).await?,
    };
    let key = Source::Calendar(calendar).key();
    summary.items = events.len();
    if events.is_empty() {
        return Ok(());
    }
    let mut notes = vec![calendar_counts(&events)];
    let texts = render_events(events);
    let header = format!(
        "Calendar from {} to {}. Today is {}.",
        window.after.format("%Y-%m-%d"),
        window.before.unwrap_or(now).format("%Y-%m-%d"),
        now.format("%Y-%m-%d")
    );
    notes.extend(extract(generator, CALENDAR_SYSTEM, &header, &texts).await?);
    user.add_profile_notes(key, &notes, None).await?;
    summary.notes = notes.len();
    Ok(())
}

/// Each title once, with how often and when it happened, where and with whom.
fn render_events(events: Vec<Event>) -> Vec<String> {
    let mut by_title: BTreeMap<String, Vec<Event>> = BTreeMap::new();
    for event in events {
        by_title
            .entry(event.title.trim().to_lowercase())
            .or_default()
            .push(event);
    }
    let mut lines: Vec<(NaiveDate, String)> = by_title
        .into_values()
        .map(|mut events| {
            events.sort_by_key(|event| event.day);
            let first = &events[0];
            let last = &events[events.len() - 1];
            let mut line = if events.len() == 1 {
                format!("{} {}", first.day, first.title)
            } else {
                format!(
                    "{} to {} {} ({} times)",
                    first.day,
                    last.day,
                    first.title,
                    events.len()
                )
            };
            let places: HashSet<&str> = events
                .iter()
                .map(|event| event.location.as_str())
                .filter(|place| !place.is_empty())
                .collect();
            if !places.is_empty() {
                let places: Vec<&str> = places.into_iter().take(3).collect();
                let _ = write!(line, "; at {}", places.join(" / "));
            }
            let people: HashSet<&str> = events
                .iter()
                .flat_map(|event| event.people.iter().map(String::as_str))
                .collect();
            if !people.is_empty() {
                let people: Vec<&str> = people.into_iter().take(8).collect();
                let _ = write!(line, "; with {}", people.join(", "));
            }
            (first.day, clip(&line, 600))
        })
        .collect();
    lines.sort();
    lines.into_iter().map(|(_, line)| line).collect()
}

/// The people and places that come up most.
fn calendar_counts(events: &[Event]) -> ProfileNote {
    let mut people: HashMap<String, usize> = HashMap::new();
    let mut places: HashMap<String, usize> = HashMap::new();
    for event in events {
        for person in &event.people {
            *people.entry(person.clone()).or_default() += 1;
        }
        if !event.location.is_empty() {
            *places.entry(event.location.clone()).or_default() += 1;
        }
    }
    let mut text = format!("Calendar: {} events.", events.len());
    if !people.is_empty() {
        let _ = write!(text, " Meets most with: {}.", top(people));
    }
    if !places.is_empty() {
        let _ = write!(text, " Most frequent places: {}.", top(places));
    }
    ProfileNote { date: None, text }
}

fn rfc3339(time: DateTime<Utc>) -> String {
    time.to_rfc3339_opts(SecondsFormat::Secs, true)
}

async fn google_events(user: &User, window: Window) -> Result<Vec<Event>> {
    let client = user
        .composio_client()
        .context("Composio is not configured")?;
    let mut events = Vec::new();
    let mut token: Option<String> = None;
    for _ in 0..CALENDAR_PAGES {
        let mut arguments = json!({
            "calendarId": "primary",
            "timeMin": rfc3339(window.after),
            "timeMax": rfc3339(window.before.unwrap_or_else(Utc::now)),
            "maxResults": 250,
            "singleEvents": true,
        });
        if let Some(token) = &token {
            arguments["pageToken"] = json!(token);
        }
        let response = client
            .execute_tool("GOOGLECALENDAR_EVENTS_LIST", arguments)
            .await?;
        if response.successful == Some(false) {
            bail!("Google Calendar listing failed");
        }
        events.extend(event_list(&response.data).iter().filter_map(parse_event));
        token = find_string(&response.data, "nextPageToken");
        if token.is_none() {
            break;
        }
    }
    Ok(events)
}

async fn outlook_events(user: &User, window: Window) -> Result<Vec<Event>> {
    const TOP: usize = 100;
    let client = user
        .composio_client()
        .context("Composio is not configured")?;
    let mut events = Vec::new();
    for page in 0..CALENDAR_PAGES {
        // The calendar view expands repeating events into each time they happen.
        let arguments = json!({
            "start_datetime": rfc3339(window.after),
            "end_datetime": rfc3339(window.before.unwrap_or_else(Utc::now)),
            "top": TOP,
            "skip": page * TOP,
            "orderby": ["start/dateTime"],
        });
        let response = client
            .execute_tool("OUTLOOK_GET_CALENDAR_VIEW", arguments)
            .await?;
        if response.successful == Some(false) {
            bail!("Outlook calendar listing failed");
        }
        let listed = event_list(&response.data);
        events.extend(listed.iter().filter_map(parse_event));
        if listed.len() < TOP {
            break;
        }
    }
    Ok(events)
}

/// Fastmail's tool is read from its own schema, since its arguments are not
/// known ahead. It refuses to expand repeating events over a long window, so
/// the window is read a season at a time.
async fn fastmail_events(user: &User, window: Window) -> Result<Vec<Event>> {
    const TOOL: &str = "search_events";
    let mut schema = user.mcp_tool_schema(FASTMAIL, TOOL).await;
    if schema.is_none() {
        // Any call connects the server, which lists its tools.
        let _ = user.call_mcp(FASTMAIL, "list_calendars", json!({})).await;
        schema = user.mcp_tool_schema(FASTMAIL, TOOL).await;
    }
    let schema = schema.context("Fastmail has no search_events tool")?;
    let arguments = EventArguments::from_schema(&schema)?;
    let end = window.before.unwrap_or_else(Utc::now);
    let mut events = Vec::new();
    let mut after = window.after;
    while after < end {
        let before = (after + Days::days(FASTMAIL_SPAN_DAYS)).min(end);
        let span = Window {
            after,
            before: Some(before),
        };
        events.extend(fastmail_span(user, &arguments, span).await?);
        after = before;
    }
    Ok(events)
}

/// Fastmail's events in one span of at most `FASTMAIL_SPAN_DAYS`.
async fn fastmail_span(
    user: &User,
    arguments: &EventArguments,
    window: Window,
) -> Result<Vec<Event>> {
    const TOOL: &str = "search_events";
    const LIMIT: usize = 100;
    let mut events = Vec::new();
    let mut cursor: Option<String> = None;
    let mut offset = 0;
    for _ in 0..CALENDAR_PAGES {
        let call = arguments.build(window, LIMIT, cursor.as_deref(), offset);
        let value = user.call_mcp(FASTMAIL, TOOL, call).await?;
        let listed = event_list(&value);
        events.extend(listed.iter().filter_map(parse_event));
        offset += listed.len();
        cursor = ["nextCursor", "cursor", "next", "nextPageToken"]
            .iter()
            .find_map(|key| find_string(&value, key));
        let more = match (&arguments.cursor, &arguments.offset) {
            (Some(_), _) => cursor.is_some(),
            (None, Some(_)) => listed.len() >= LIMIT,
            (None, None) => false,
        };
        if !more || listed.is_empty() {
            break;
        }
    }
    Ok(events)
}

/// The argument names a search tool takes for its window, size and paging.
#[derive(Debug, Default, PartialEq)]
struct EventArguments {
    after: Option<Argument>,
    before: Option<Argument>,
    limit: Option<String>,
    cursor: Option<String>,
    offset: Option<String>,
}

#[derive(Debug, PartialEq)]
struct Argument {
    name: String,
    /// Takes `YYYY-MM-DD` rather than a full time.
    date_only: bool,
}

impl EventArguments {
    fn from_schema(schema: &Value) -> Result<Self> {
        let Some(properties) = schema["properties"].as_object() else {
            bail!("search_events has no properties");
        };
        let mut found = EventArguments::default();
        for (name, property) in properties {
            let lower = name.to_lowercase();
            let date_only = property["format"].as_str() == Some("date");
            let argument = || Argument {
                name: name.clone(),
                date_only,
            };
            let has = |words: &[&str]| words.iter().any(|word| lower.contains(word));
            if has(&[
                "limit",
                "maxresults",
                "max_results",
                "count",
                "pagesize",
                "page_size",
            ]) {
                found.limit = Some(name.clone());
            } else if has(&["cursor", "token"]) {
                found.cursor = Some(name.clone());
            } else if has(&["offset", "position", "skip"]) {
                found.offset = Some(name.clone());
            } else if has(&["after", "start", "from", "min", "since"]) {
                found.after.get_or_insert_with(argument);
            } else if has(&["before", "end", "until", "max", "to"]) {
                found.before.get_or_insert_with(argument);
            }
        }
        if found.after.is_none() {
            let names: Vec<&String> = properties.keys().collect();
            bail!("no start argument among {names:?}");
        }
        Ok(found)
    }

    fn build(&self, window: Window, limit: usize, cursor: Option<&str>, offset: usize) -> Value {
        let time = |argument: &Argument, time: DateTime<Utc>| {
            if argument.date_only {
                time.format("%Y-%m-%d").to_string()
            } else {
                rfc3339(time)
            }
        };
        let mut arguments = json!({});
        if let Some(after) = &self.after {
            arguments[&after.name] = json!(time(after, window.after));
        }
        if let (Some(before), Some(end)) = (&self.before, window.before) {
            arguments[&before.name] = json!(time(before, end));
        }
        if let Some(name) = &self.limit {
            arguments[name] = json!(limit);
        }
        if let (Some(name), Some(cursor)) = (&self.cursor, cursor) {
            arguments[name] = json!(cursor);
        }
        if let Some(name) = &self.offset {
            arguments[name] = json!(offset);
        }
        arguments
    }
}

/// The events of a listing, wherever the provider put them.
fn event_list(value: &Value) -> Vec<Value> {
    fn find(value: &Value, depth: usize) -> Option<&Vec<Value>> {
        if let Some(list) = value.as_array() {
            return Some(list);
        }
        let object = value.as_object()?;
        for key in [
            "items",
            "value",
            "events",
            "results",
            "data",
            "response_data",
        ] {
            if let Some(list) = object.get(key).and_then(|value| find_at(value, depth)) {
                return Some(list);
            }
        }
        None
    }
    fn find_at(value: &Value, depth: usize) -> Option<&Vec<Value>> {
        if depth == 0 {
            None
        } else {
            find(value, depth - 1)
        }
    }
    find(value, 4).cloned().unwrap_or_default()
}

/// A string field `key` at most a few levels down.
fn find_string(value: &Value, key: &str) -> Option<String> {
    fn find(value: &Value, key: &str, depth: usize) -> Option<String> {
        let object = value.as_object()?;
        if let Some(found) = object.get(key).and_then(Value::as_str) {
            return Some(found.to_owned()).filter(|found| !found.is_empty());
        }
        if depth == 0 {
            return None;
        }
        object
            .values()
            .filter(|value| value.is_object())
            .find_map(|value| find(value, key, depth - 1))
    }
    find(value, key, 3)
}

/// Reads a Google, Outlook or JMAP-style event.
fn parse_event(value: &Value) -> Option<Event> {
    let cancelled = value["status"].as_str() == Some("cancelled")
        || value["isCancelled"].as_bool() == Some(true);
    if cancelled {
        return None;
    }
    let title = ["summary", "subject", "title"]
        .iter()
        .find_map(|key| value[key].as_str())
        .unwrap_or_default()
        .trim()
        .to_owned();
    if title.is_empty() {
        return None;
    }
    let start = &value["start"];
    let start = start["dateTime"]
        .as_str()
        .or_else(|| start["date"].as_str())
        .or_else(|| start.as_str())
        .or_else(|| value["startTime"].as_str())
        .or_else(|| value["utcStart"].as_str())?;
    let day = NaiveDate::parse_from_str(start.get(..10)?, "%Y-%m-%d").ok()?;
    let location = &value["location"];
    let location = location
        .as_str()
        .or_else(|| location["displayName"].as_str())
        .map(str::to_owned)
        .or_else(|| {
            value["locations"]
                .as_object()?
                .values()
                .find_map(|place| place["name"].as_str().map(str::to_owned))
        })
        .unwrap_or_default();
    let people = people(&value["attendees"])
        .into_iter()
        .chain(people(&value["participants"]))
        .collect();
    Some(Event {
        title: clip(&title, 200),
        day,
        location: clip(location.trim(), 200),
        people,
    })
}

/// Names, or addresses when there is no name, of an attendee list or map.
fn people(value: &Value) -> Vec<String> {
    let entries: Vec<&Value> = match value {
        Value::Array(list) => list.iter().collect(),
        Value::Object(map) => map.values().collect(),
        _ => return Vec::new(),
    };
    entries
        .into_iter()
        .filter(|entry| entry["self"].as_bool() != Some(true))
        .filter_map(|entry| {
            let email = &entry["emailAddress"];
            [
                entry["displayName"].as_str(),
                entry["name"].as_str(),
                email["name"].as_str(),
                entry["email"].as_str(),
                email["address"].as_str(),
            ]
            .into_iter()
            .flatten()
            .map(str::trim)
            .find(|name| !name.is_empty())
            .map(str::to_owned)
        })
        .collect()
}

// The model.

#[derive(Deserialize)]
struct Notes {
    #[serde(default)]
    notes: Vec<RawNote>,
}

#[derive(Deserialize)]
struct RawNote {
    #[serde(default)]
    date: Option<String>,
    note: String,
}

/// Packs `texts` into batches and asks for notes on each, several at once.
async fn extract(
    generator: Generator<'_>,
    system: &str,
    header: &str,
    texts: &[String],
) -> Result<Vec<ProfileNote>> {
    let system = format!("{system}\n{NOTE_RULES}");
    let batches = pack(texts, BATCH_CHARS);
    let results: Vec<Result<Vec<ProfileNote>>> = stream::iter(batches)
        .map(|batch| {
            let system = &system;
            async move {
                let prompt = format!("{header}\n\n{batch}");
                let answer = generator.text(system, prompt).await?;
                parse_notes(&answer)
            }
        })
        .buffer_unordered(CONCURRENCY)
        .collect()
        .await;
    let mut notes = Vec::new();
    let mut failed = 0;
    for result in results {
        match result {
            Ok(found) => notes.extend(found),
            Err(error) => {
                failed += 1;
                log::debug!("profile: a batch failed: {error:#}");
            }
        }
    }
    // A few lost batches cost little; most failing means something is wrong.
    if failed > 0 && notes.is_empty() {
        bail!("every batch of notes failed");
    }
    Ok(notes)
}

/// `texts` joined into pieces of about `chars`, each text whole unless it is
/// longer than that alone.
fn pack(texts: &[String], chars: usize) -> Vec<String> {
    let mut batches = Vec::new();
    let mut batch = String::new();
    for text in texts {
        let text = clip(text, chars);
        if !batch.is_empty() && batch.len() + text.len() > chars {
            batches.push(mem::take(&mut batch));
        }
        batch.push_str(&text);
        batch.push_str("\n\n");
    }
    if !batch.trim().is_empty() {
        batches.push(batch);
    }
    batches
}

fn parse_notes(answer: &str) -> Result<Vec<ProfileNote>> {
    let answer = unfence(answer);
    let notes = match serde_json::from_str::<Notes>(answer) {
        Ok(notes) => notes.notes,
        Err(error) => match complete_notes(answer) {
            Some(notes) if !notes.is_empty() => notes,
            _ => return Err(error).context("notes are not JSON"),
        },
    };
    Ok(notes
        .into_iter()
        .filter(|note| !note.note.trim().is_empty())
        .map(|note| ProfileNote {
            date: note.date.as_deref().and_then(parse_day),
            text: clip(note.note.trim(), 500),
        })
        .collect())
}

/// The notes that arrived whole when the answer was cut off partway.
fn complete_notes(answer: &str) -> Option<Vec<RawNote>> {
    let start = answer.find("\"notes\"")?;
    let mut rest = &answer[start..];
    rest = &rest[rest.find('[')? + 1..];
    let mut notes = Vec::new();
    loop {
        rest = rest.trim_start_matches(|c: char| c.is_whitespace() || c == ',');
        if !rest.starts_with('{') {
            break;
        }
        let mut values = serde_json::Deserializer::from_str(rest).into_iter::<RawNote>();
        match values.next() {
            Some(Ok(note)) => {
                notes.push(note);
                rest = &rest[values.byte_offset()..];
            }
            _ => break,
        }
    }
    Some(notes)
}

fn parse_day(text: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(text.get(..10)?, "%Y-%m-%d").ok()
}

#[derive(Deserialize)]
struct Facts {
    #[serde(default)]
    facts: Vec<RawFact>,
}

#[derive(Deserialize)]
struct RawFact {
    content: String,
    #[serde(default)]
    valid_from: Option<String>,
    #[serde(default)]
    valid_until: Option<String>,
    #[serde(default)]
    evidence: Option<String>,
}

/// Turns every note about the user into dossiers, then into memories.
/// Returns how many were saved.
async fn condense(generator: Generator<'_>, user: &User) -> Result<usize> {
    let notes = user.profile_notes().await?;
    if notes.is_empty() {
        return Ok(0);
    }
    let mut texts: Vec<String> = notes
        .iter()
        .map(|note| match note.date {
            Some(day) => format!("{day} {}", note.text),
            None => note.text.clone(),
        })
        .collect();
    // Each round merges chunks of notes, then of dossiers, until one is left.
    loop {
        let chunks = pack(&texts, CONDENSE_CHARS);
        let last = chunks.len() <= 1;
        let dossiers: Vec<Result<String>> = stream::iter(chunks)
            .map(|chunk| generator.text_up_to(DOSSIER_SYSTEM, chunk, DOSSIER_TOKENS))
            .buffered(CONCURRENCY)
            .collect()
            .await;
        texts = dossiers
            .into_iter()
            .collect::<Result<Vec<_>>>()
            .context("a dossier failed")?;
        if last {
            break;
        }
    }

    let mut prompt = format!(
        "Today: {}\n\nKept memories:\n",
        Utc::now().format("%Y-%m-%d")
    );
    for row in user.memory_rows().await? {
        let _ = writeln!(prompt, "- {}", row.content);
    }
    let _ = write!(prompt, "\nDossiers:\n{}", texts.join("\n\n"));
    let answer = generator
        .text_up_to(FACTS_SYSTEM, prompt, FACTS_TOKENS)
        .await?;
    let facts: Facts = serde_json::from_str(unfence(&answer)).context("facts are not JSON")?;
    let mut saved = 0;
    for fact in facts.facts.into_iter().take(MAX_FACTS) {
        let content = fact.content.trim();
        if content.is_empty() {
            continue;
        }
        let valid = Validity {
            from: fact.valid_from.as_deref().and_then(parse_day),
            until: fact.valid_until.as_deref().and_then(parse_day),
        };
        let origin = Origin {
            reference: "profile".to_owned(),
            quote: fact.evidence.map(|evidence| clip(evidence.trim(), 300)),
        };
        let fact = Fact {
            content,
            category: None,
            source: MemorySource::Email {},
            valid: Some(valid),
            origin: Some(origin),
        };
        match user.remember(fact).await {
            Ok(_) => saved += 1,
            Err(error) => log::debug!("profile: a fact was not saved: {error:#}"),
        }
    }
    Ok(saved)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn email(id: &str, from: &str, sent: bool, thread: Option<&str>) -> Email {
        Email {
            id: id.into(),
            received: Utc::now(),
            from: from.into(),
            to: "Ana <ana@example.com>".into(),
            subject: format!("subject {id}"),
            sent,
            bulk: false,
            thread: thread.map(Into::into),
            body: "body".into(),
        }
    }

    #[test]
    fn threads_the_user_wrote_in_come_first_and_frequent_senders_are_summed_up() {
        let mut emails = vec![
            email("a", "Ana <ana@example.com>", false, Some("t1")),
            email("b", "me@example.com", true, Some("t1")),
            email("c", "Bo <bo@example.com>", false, Some("t2")),
        ];
        for n in 0..12 {
            emails.push(email(
                &format!("s{n}"),
                "Shop <news@shop.example>",
                false,
                None,
            ));
        }
        let groups = group_mail(emails);
        assert!(matches!(&groups[0], Group::Thread(thread) if thread.len() == 2));
        assert!(matches!(
            &groups[1],
            Group::Digest { sender, count: 12, samples, subjects }
                if sender == "news@shop.example" && samples.len() == DIGEST_SAMPLES && subjects.len() == 12
        ));
        assert!(matches!(&groups[2], Group::Single(email) if email.id == "c"));
    }

    #[test]
    fn reads_google_outlook_and_jmap_events() {
        let google = json!({"summary": "Gym", "start": {"dateTime": "2025-03-04T18:00:00+01:00"},
            "location": "Basic-Fit", "attendees": [{"email": "me@x.com", "self": true}, {"displayName": "Ana"}]});
        let outlook = json!({"subject": "Standup", "start": {"dateTime": "2025-03-05T09:00:00.0000000"},
            "location": {"displayName": "Office"}, "attendees": [{"emailAddress": {"name": "Bo", "address": "bo@x.com"}}]});
        let jmap = json!({"title": "Dinner", "start": "2025-03-06T20:00:00",
            "locations": {"1": {"name": "Casa"}}, "participants": {"p": {"name": "Cy"}}});
        let cancelled =
            json!({"summary": "Gone", "status": "cancelled", "start": {"date": "2025-03-07"}});
        let events: Vec<Event> = [google, outlook, jmap, cancelled]
            .iter()
            .filter_map(parse_event)
            .collect();
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].people, ["Ana"]);
        assert_eq!(events[1].location, "Office");
        assert_eq!(events[2].day, NaiveDate::from_ymd_opt(2025, 3, 6).unwrap());
        assert_eq!(events[2].location, "Casa");
    }

    #[test]
    fn repeating_events_are_listed_once() {
        let event = |day: u32| Event {
            title: "Gym".into(),
            day: NaiveDate::from_ymd_opt(2025, 1, day).unwrap(),
            location: "Basic-Fit".into(),
            people: Vec::new(),
        };
        let lines = render_events(vec![event(3), event(10), event(17)]);
        assert_eq!(
            lines,
            ["2025-01-03 to 2025-01-17 Gym (3 times); at Basic-Fit"]
        );
    }

    #[test]
    fn search_arguments_come_from_the_schema() {
        let schema = json!({"properties": {
            "after": {"type": "string", "format": "date"},
            "before": {"type": "string", "format": "date"},
            "limit": {"type": "integer"},
            "position": {"type": "integer"},
        }});
        let arguments = EventArguments::from_schema(&schema).unwrap();
        let day = |d| {
            NaiveDate::from_ymd_opt(2025, 1, d)
                .unwrap()
                .and_hms_opt(0, 0, 0)
                .unwrap()
                .and_utc()
        };
        let window = Window {
            after: day(1),
            before: Some(day(31)),
        };
        assert_eq!(
            arguments.build(window, 100, None, 200),
            json!({"after": "2025-01-01", "before": "2025-01-31", "limit": 100, "position": 200})
        );
        assert!(EventArguments::from_schema(&json!({"properties": {"query": {}}})).is_err());
    }

    #[test]
    fn notes_parse_with_and_without_dates() {
        let notes = parse_notes(
            "```json\n{\"notes\": [{\"date\": \"2024-05-01\", \"note\": \"Flew to Tokyo.\"}, {\"note\": \"Speaks Spanish.\"}]}\n```",
        )
        .unwrap();
        assert_eq!(notes[0].date, NaiveDate::from_ymd_opt(2024, 5, 1));
        assert_eq!(notes[1].date, None);
    }

    #[test]
    fn a_cut_off_answer_keeps_its_whole_notes() {
        let notes = parse_notes(
            "{\"notes\": [{\"note\": \"Flew to Tokyo.\"}, {\"note\": \"Speaks Spanish.\"}, {\"date\": \"2024-",
        )
        .unwrap();
        assert_eq!(notes.len(), 2);
        assert_eq!(notes[1].text, "Speaks Spanish.");
        assert!(parse_notes("{\"notes\": [{\"note\": \"Fle").is_err());
    }

    #[test]
    fn senders_from_dating_and_medical_services_are_skipped() {
        assert!(skipped("Tinder <no-reply@gotinder.com>"));
        assert!(skipped("MyChart <noreply@mychart.org>"));
        assert!(!skipped("Zara <news@zara.com>"));
        assert_eq!(address("\"Ana\" <Ana@Example.com>"), "ana@example.com");
        assert_eq!(sender_name("Zara <news@zara.com>"), "Zara");
        assert_eq!(sender_name("news@zara.com"), "zara.com");
    }
}
