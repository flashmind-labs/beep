//! The profile pass: reads each connected mailbox and calendar once and builds a profile of the user.
//! Notes are stored per finished year, so a cut-short pass resumes; content is never logged.

use crate::{
    account::user::User,
    app::{App, Reach, UserNotice},
    apps::feeds::mail::Mailbox,
    apps::mcp::FASTMAIL,
    memory::memories::clip,
    memory::starters::{Generator, unfence},
    protocol::{MemoryRead, MemorySource, Notice},
    store::{Fact, MemoryRow, Origin, ProfileNote, Scope, Validity},
};
use anyhow::{Context, Result, bail};
use chrono::{DateTime, NaiveDate, SecondsFormat, Utc};
use futures::{StreamExt, stream};
use serde::Deserialize;
use std::{
    collections::{HashMap, HashSet},
    fmt::{self, Write},
    mem,
    sync::{Arc, LazyLock, Mutex, MutexGuard, PoisonError},
};

mod calendar;
mod mail;

use calendar::read_calendar;
use mail::read_mail;

/// Text per extraction call.
const BATCH_CHARS: usize = 16_000;
/// Extraction calls at once.
const CONCURRENCY: usize = 8;
/// People and senders a yearly count note names.
const TOP_COUNT: usize = 15;
/// Notes per condensing call, and the facts it may end with.
const CONDENSE_CHARS: usize = 60_000;
const MAX_FACTS: usize = 80;
/// Answer room for a dossier and for the final facts.
const DOSSIER_TOKENS: u32 = 6000;
const FACTS_TOKENS: u32 = 12_000;
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
    const ALL: [Source; 6] = [
        Source::Mail(Mailbox::Gmail),
        Source::Mail(Mailbox::Outlook),
        Source::Mail(Mailbox::Fastmail),
        Source::Calendar(Calendar::Google),
        Source::Calendar(Calendar::Outlook),
        Source::Calendar(Calendar::Fastmail),
    ];

    /// How the user knows it.
    pub fn name(self) -> &'static str {
        match self {
            Source::Mail(mailbox) => mailbox.name(),
            Source::Calendar(Calendar::Google) => "Google Calendar",
            Source::Calendar(Calendar::Outlook) => "Outlook calendar",
            Source::Calendar(Calendar::Fastmail) => "Fastmail calendar",
        }
    }

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
        Self::take_id(user.id().to_owned(), source)
    }

    fn take_id(user: String, source: Source) -> Option<Self> {
        let inserted = running().insert((user.clone(), source));
        inserted.then_some(Claim { user, source })
    }

    /// Frees the source and says, under one lock, whether no other source of the user is still read.
    fn release_last(mut self) -> bool {
        let user = mem::take(&mut self.user);
        let mut running = running();
        running.remove(&(user.clone(), self.source));
        let last = !running.iter().any(|(id, _)| *id == user);
        drop(running);
        // Already freed above.
        mem::forget(self);
        last
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

/// The sources behind the connected apps `slugs`.
fn sources(slugs: &[String]) -> Vec<Source> {
    slugs
        .iter()
        .flat_map(|slug| Source::from_slug(slug))
        .copied()
        .collect()
}

/// Reads, in the background, each unread source behind the connected apps `slugs`.
pub fn spawn(app: &Arc<App>, user: &Arc<User>, slugs: &[String]) {
    for source in sources(slugs) {
        let Some(claim) = Claim::take(user, source) else {
            continue;
        };
        tokio::spawn(read(app.clone(), user.clone(), source, claim));
    }
}

/// Rereads each source behind `slugs` from the start, dropping its notes; returns the sources started.
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
        if claim.release_last() {
            match condense(generator, &user, source).await {
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
    if !claim.release_last() {
        return;
    }
    match condense(generator, &user, source).await {
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
        desktop: None,
        to: Reach::Everyone,
    };
    // Nobody listening means the phone is offline; Memories still shows them.
    let _ = user.notifications.send(notice);
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

fn rfc3339(time: DateTime<Utc>) -> String {
    time.to_rfc3339_opts(SecondsFormat::Secs, true)
}

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

/// `texts` joined into pieces of about `chars`, splitting only texts longer than that.
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

/// Condenses every source's notes into memories tagged with `source`; returns how many were saved.
async fn condense(generator: Generator<'_>, user: &User, source: Source) -> Result<usize> {
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
        // A round that merges nothing would repeat forever, so it is the last.
        let last = chunks.len() <= 1 || chunks.len() >= texts.len();
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
    let read = read_reference(source, Utc::now());
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
            reference: read.clone(),
            quote: fact.evidence.map(|evidence| clip(evidence.trim(), 300)),
        };
        let fact = Fact {
            content,
            category: None,
            source: MemorySource::Email {},
            valid: Some(valid),
            origin: Some(origin),
            scope: Some(Scope::User),
        };
        match user.remember(fact).await {
            Ok(_) => saved += 1,
            Err(error) => log::debug!("profile: a fact was not saved: {error:#}"),
        }
    }
    Ok(saved)
}

/// Marks a memory a read added or confirmed: `read:<source key>:<finished>`.
const READ_PREFIX: &str = "read:";

fn read_reference(source: Source, finished: DateTime<Utc>) -> String {
    format!("{READ_PREFIX}{}:{}", source.key(), rfc3339(finished))
}

/// The reads that added or confirmed `rows`, newest first.
pub fn memory_reads(rows: &[MemoryRow]) -> Vec<MemoryRead> {
    let mut reads: Vec<MemoryRead> = Vec::new();
    for row in rows {
        let Some(reference) = row.origin.as_ref().map(|origin| &origin.reference) else {
            continue;
        };
        let Some((key, finished_at)) = reference
            .strip_prefix(READ_PREFIX)
            .and_then(|rest| rest.split_once(':'))
        else {
            continue;
        };
        let Some(source) = Source::ALL.into_iter().find(|source| source.key() == key) else {
            continue;
        };
        match reads.iter_mut().find(|read| &read.id == reference) {
            Some(read) => read.memories.push(row.id.clone()),
            None => reads.push(MemoryRead {
                id: reference.clone(),
                source: source.name().to_owned(),
                finished_at: finished_at.to_owned(),
                memories: vec![row.id.clone()],
            }),
        }
    }
    // RFC 3339 in UTC to the second sorts as text.
    reads.sort_by(|a, b| b.finished_at.cmp(&a.finished_at));
    reads
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_last_read_to_end_condenses() {
        let claim = |source| Claim::take_id("release-last-test".into(), source).unwrap();
        let gmail = claim(Source::Mail(Mailbox::Gmail));
        let calendar = claim(Source::Calendar(Calendar::Google));
        assert!(!gmail.release_last());
        assert!(calendar.release_last());
        assert!(!running().iter().any(|(id, _)| id == "release-last-test"));
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
}
