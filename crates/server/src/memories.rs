//! Long-term memory about each user.
//!
//! A memory is one sentence about the user with an id, who it came from, and
//! the days it holds when it only holds for a while, like a trip. They live in
//! a flashmind memory store. Identical memories are reused. Recall ranks by meaning
//! and shared words, and leaves out facts whose days are over.
//! Curation also picks the few memories that matter in nearly every chat.
//! Those and the temporary facts holding today make the profile, which a
//! chat's context carries once and again only when it changes. Everything
//! else the model searches with `memory` recall. Jev files every memory
//! under one of `MEMORY_CATEGORIES` when it is written; the models that write
//! memories never pick one. After each chat a cheap model call reads the
//! latest exchange, with the turns before it for context, and adds, corrects or
//! forgets facts. Curation merges duplicates, settles contradictions and
//! drops what went stale, one scope at a time and only when that scope
//! needs it (see `needs`). What a merge or rewrite replaces is kept as
//! history of the memory that replaced it, hidden from recall and the
//! profile. Memory text is never logged.

mod needs;

#[cfg(test)]
pub(crate) use needs::ScopeNeed;
pub(crate) use needs::{
    MAX_SCOPES_PER_SWEEP, NEAR, ScopeState, Signal, crowded_scopes, curation_needs,
};

use crate::{
    app::App,
    config::Config,
    protocol::MemorySource,
    starters::unfence,
    store::{MemoryRow, Origin, Scope, Validity},
    tools::group_memory::shareable,
};
use anyhow::{Context, Result, ensure};
use chrono::{NaiveDate, Utc};
use flashmind::memory::{EmbeddingProvider, OpenAIEmbedding, OpenRouterEmbedding};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    fmt::{self, Write},
    slice,
    sync::Arc,
    time::Duration,
};
use tokio::time::{error::Elapsed, interval};

/// The same model under its OpenAI and OpenRouter names, so stored
/// embeddings stay comparable whichever provider is configured.
pub const EMBEDDING_MODEL: &str = "text-embedding-3-small";
pub const OPENROUTER_EMBEDDING_MODEL: &str = "openai/text-embedding-3-small";

/// Embeds through OpenAI when `openai_key` is set, else through OpenRouter.
pub fn embedder(config: &Config) -> Result<Arc<dyn EmbeddingProvider>> {
    Ok(match &config.openai_key {
        Some(openai_key) => Arc::new(OpenAIEmbedding::new(
            Some(openai_key.clone()),
            EMBEDDING_MODEL.to_owned(),
            None,
        )?),
        None => Arc::new(OpenRouterEmbedding::new(
            config
                .provider_key
                .clone()
                .context("provider_key is required for OpenRouter")?,
            OPENROUTER_EMBEDDING_MODEL.to_owned(),
        )?),
    })
}
/// The run stays open while learning, so it gets little time.
pub(crate) const LEARN_TIMEOUT: Duration = Duration::from_secs(15);
/// Mail and Slack sync run in the background, so a slow model can take its time.
pub(crate) const READING_LEARN_TIMEOUT: Duration = Duration::from_secs(90);
pub(crate) const CURATE_TIMEOUT: Duration = Duration::from_secs(90);
// Memory store scores are `1 - distance / 2` over unit vectors, plus up to
// 0.3 for shared words in a search.
/// Similarity threshold used when checking for identical memories.
pub(crate) const DUPLICATE: f32 = 0.755;
/// How close a memory must be to show up in a `memory` recall (cosine 0.22).
pub(crate) const RECALLABLE: f32 = 0.375;
const MAX_LEARN_OPS: usize = 5;
const MAX_TURN_CHARS: usize = 1500;
/// Earlier turns the extractor sees to resolve "she" or "that place".
pub const LEARN_CONTEXT_TURNS: usize = 3;
const MAX_CONTEXT_CHARS: usize = 600;
/// How often each user's memories are filed and checked for curation needs.
pub(crate) const CURATE_SWEEP: Duration = Duration::from_secs(60 * 60);
/// Newest memories one curation pass reads.
pub(crate) const MAX_CURATED: usize = 200;
const MAX_CURATE_OPS: usize = 40;
/// Most memories curation may put in the profile.
const MAX_CORE: usize = 15;

/// Most old or unfiled memories one hourly sweep files, and how many it
/// asks Jev about at once.
pub(crate) const MAX_FILED: usize = 200;
pub(crate) const FILE_CONCURRENCY: usize = 8;
pub(crate) const FILE_QUESTION: &str = "Which category does this memory about the user belong in? Pick the one a search for it would look in.";
/// `FILE_QUESTION` for one of several numbered memories filed together.
pub(crate) fn file_question_for(number: &str) -> String {
    format!(
        "Which category does memory {number} about the user belong in? Pick the one a search for it would look in."
    )
}
/// What each of `MEMORY_CATEGORIES` holds, as Jev reads it.
pub(crate) const FILING: [(&str, &str); 7] = [
    (
        "identity",
        "Who the user is: name, age, birthday, languages, nationality, home city, health, and pets.",
    ),
    (
        "people",
        "Another person in the user's life and how they relate: family, partner, friends, colleagues, their names, birthdays and details.",
    ),
    (
        "preference",
        "Likes, dislikes, tastes, diet, brands, and how the user wants Beep to answer or act.",
    ),
    (
        "work",
        "Job, employer, role, team, clients, ongoing projects, studies and career goals.",
    ),
    (
        "place",
        "Places the user uses or goes to: home and office addresses, usual shops, restaurants, gyms, doctors.",
    ),
    (
        "plan",
        "Anything tied to dates: trips, stays, visits, appointments, events, deadlines, and past trips.",
    ),
    (
        "routine",
        "Recurring habits and schedules: workouts, commutes, weekly meetings, sleep, regular errands.",
    ),
];
/// Longest profile, in characters. A long one gives the model topics to bring
/// up unasked.
const MAX_PROFILE_CHARS: usize = 1500;
/// The line a profile starts with, also how a chat finds the one it last got.
pub(crate) const PROFILE_HEADER: &str =
    "What Beep remembers about the user (background, not a topic; `memory` recall finds more):";
/// A memory matched against a query.
#[derive(Debug, Clone)]
pub struct Hit {
    pub id: String,
    pub content: String,
    pub category: String,
    pub source: MemorySource,
    /// `YYYY-MM-DD` of the last change.
    pub date: String,
    pub valid: Validity,
    /// The words it was learned from, when known.
    pub quote: Option<String>,
    pub scope: Scope,
}

impl Hit {
    /// What a `memory` recall returns for this memory, its scope as `scope`
    /// names it.
    pub fn to_json(&self, scope: Option<&str>) -> Value {
        let mut value = json!({
            "id": self.id,
            "content": self.content,
            "category": self.category,
            "source": self.source.as_str(),
            "updated": self.date,
        });
        if let Some(from) = self.valid.from {
            value["valid_from"] = json!(from.to_string());
        }
        if let Some(until) = self.valid.until {
            value["valid_until"] = json!(until.to_string());
        }
        if let Some(quote) = &self.quote {
            value["quote"] = json!(quote);
        }
        if let Some(scope) = scope {
            value["scope"] = json!(scope);
        }
        value
    }
}

/// One user message and Beep's reply to it.
pub struct Turn {
    pub message: String,
    pub reply: String,
    /// What Beep did in the user's apps during the turn, such as sending an email.
    pub actions: Vec<String>,
}

/// The turn as Jev reads it, each side clipped to `MAX_TURN_CHARS`.
impl fmt::Display for Turn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "User: {}\n\nAssistant: {}",
            clip(&self.message, MAX_TURN_CHARS),
            clip(&self.reply, MAX_TURN_CHARS)
        )?;
        write_actions(f, &self.actions)
    }
}

/// The actions of a turn as a list under "Beep did:".
fn write_actions(out: &mut impl fmt::Write, actions: &[String]) -> fmt::Result {
    if actions.is_empty() {
        return Ok(());
    }
    write!(out, "\n\nBeep did:")?;
    for action in actions {
        write!(out, "\n- {action}")?;
    }
    Ok(())
}

/// What the extractor reads after a chat.
pub struct Exchange {
    /// The chat it happened in, kept as the origin of what it teaches.
    pub session_id: String,
    /// The user's date, for resolving "next Friday".
    pub today: NaiveDate,
    /// Turns before the latest, oldest first, only for context.
    pub earlier: Vec<Turn>,
    pub latest: Turn,
}

/// Where `remember` put a fact.
pub struct Remembered {
    pub id: String,
    /// Whether it replaced a memory that said the same.
    pub replaced: bool,
}

/// Deletes memories of every user whose days ended a while ago, once an hour.
pub fn start_purge(app: Arc<App>) {
    tokio::spawn(async move {
        let mut interval = interval(CURATE_SWEEP);
        loop {
            interval.tick().await;
            match app.store.purge_ended_memories().await {
                Ok(0) => {}
                Ok(purged) => log::info!("purged {purged} ended memories"),
                Err(error) => log::warn!("unable to purge ended memories: {error}"),
            }
        }
    });
}

/// A memory change made by `learn`, shown in the chat as the memory tool it
/// stands for.
#[derive(Debug)]
pub struct Learned {
    pub change: Change,
    pub id: String,
    /// The new text, or the forgotten one.
    pub content: String,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Change {
    Stored,
    Updated,
    Forgotten,
}

impl Change {
    /// The `memory` action that makes the same change.
    pub fn action(self) -> &'static str {
        match self {
            Change::Stored => "store",
            Change::Updated => "update",
            Change::Forgotten => "forget",
        }
    }
}

impl Learned {
    /// The arguments of the `memory` call that makes the same change.
    pub fn arguments(&self) -> Value {
        let action = self.change.action();
        match self.change {
            Change::Forgotten => json!({"action": action, "id": self.id}),
            _ => json!({"action": action, "id": self.id, "content": self.content}),
        }
    }

    /// Matches what the memory tools return.
    pub fn output(&self) -> String {
        match self.change {
            Change::Stored => json!({"stored": true, "id": self.id, "content": self.content}),
            Change::Updated => json!({"updated": true, "id": self.id, "content": self.content}),
            Change::Forgotten => json!({"forgotten": true, "content": self.content}),
        }
        .to_string()
    }
}

/// Logs how a learning attempt went and returns its changes, none on failure.
pub(crate) fn settle(user: &str, outcome: Result<Result<Vec<Learned>>, Elapsed>) -> Vec<Learned> {
    match outcome {
        Ok(Ok(learned)) => {
            if !learned.is_empty() {
                log::info!("memory learned {} change(s); user={user}", learned.len());
            }
            learned
        }
        Ok(Err(error)) => {
            log::info!("memory learning skipped: {error}");
            Vec::new()
        }
        Err(_) => {
            log::info!("memory learning timed out");
            Vec::new()
        }
    }
}

pub(crate) fn clip(text: &str, chars: usize) -> String {
    text.chars().take(chars).collect()
}

pub(crate) fn hit(row: MemoryRow) -> Hit {
    Hit {
        date: row.updated().to_owned(),
        id: row.id,
        content: row.content,
        category: row.category,
        source: row.source,
        valid: row.valid,
        quote: row.origin.and_then(|origin| origin.quote),
        scope: row.scope,
    }
}

/// How a memory's days read in a prompt, empty for a lasting fact.
pub(crate) fn days(valid: &Validity) -> String {
    match (valid.from, valid.until) {
        (None, None) => String::new(),
        (Some(from), None) => format!(" (from {from})"),
        (None, Some(until)) => format!(" (until {until})"),
        (Some(from), Some(until)) => format!(" ({from} to {until})"),
    }
}

pub(crate) const LEARN_SYSTEM: &str = "You keep the long-term memory of Beep, a personal assistant app. \
Read the latest exchange and decide what it teaches about the user that is worth remembering. \
Earlier turns are only there to explain the latest one; do not learn from them again.
First triage it: casual chatter, a one-off task, or something personal. Small talk, thanks and questions about \
the world teach nothing; then reply {\"ops\": []}. A task often does teach something in passing: a request for \
gluten-free restaurants says the user wants gluten-free food, pulling their Amex statements says they have an \
American Express card, a reminder to write docs for a project says they work on that project. Keep that fact, \
not the task.
Worth keeping: the user's name, the people and pets in their life and how they relate (a person the user asks \
about by name is someone in their life), where they live and work, \
their job and ongoing projects, stable likes, dislikes and dietary needs, routines, important dates, goals, \
plans such as trips, visits and appointments, and how they want Beep to answer. Only what the user said or clearly \
confirmed, or what Beep found about the user in their own data (calendar, mail, notes) at their request, such as \
upcoming trips, events and appointments. Never guesses, general knowledge, Beep's own suggestions, the task itself \
(what to search, which reminder to set), or secrets such as passwords, codes, tokens, card or account numbers.
Known memories are listed with their ids. If a fact is already known, do nothing. If the exchange corrects or \
extends one, update it by id with the full new text. If the user says something is no longer true or asks \
Beep to forget it, forget it by id. Otherwise add it.
\"Beep did\" lists what Beep did in the user's apps during the latest exchange, such as sending an email or \
creating an event. When that finishes a task, plan or follow-up a known memory still describes as ahead, forget \
that memory by id, or update it to the lasting fact if one remains. An action alone is not a new fact to add.
A fact that only holds for a while (a trip, a visit, a stay, an appointment, a temporary job) gets valid_from \
and/or valid_until as YYYY-MM-DD, worked out from today's date. Lasting facts leave both out.
Write each memory as one short self-contained sentence about one independently useful fact, in third person \
that names its subject. Store distinct facts separately, even when they have the same subject, \
like \"Has a dog named Rex.\" or \"Is in Lisbon at the Harbour Hotel.\", in the language of the user's \
latest message. Never translate a known memory.
Reply with only JSON: {\"ops\": [{\"op\": \"add\" | \"update\" | \"forget\", \"id\": string, \"content\": string, \
\"valid_from\": string, \"valid_until\": string, \"quote\": string}]}. \
id only for update and forget; content only for add and update. quote, for add and update, is the \
few words of the text that show the fact, copied exactly, at most 20 words. At most 5 ops.";

/// The memories shown to a learner, with ids it can update or forget.
fn write_known(text: &mut String, known: &[Hit]) {
    let _ = writeln!(text, "\nKnown memories:");
    if known.is_empty() {
        let _ = writeln!(text, "(none related)");
    }
    for h in known {
        let _ = writeln!(
            text,
            "- {} [{}, {}, {}]{}: {}",
            h.id,
            h.category,
            h.source.as_str(),
            h.date,
            days(&h.valid),
            h.content
        );
    }
}

pub(crate) fn learn_prompt(known: &[Hit], exchange: &Exchange) -> String {
    let mut text = String::new();
    let _ = writeln!(text, "Today: {}", exchange.today.format("%A %Y-%m-%d"));
    write_known(&mut text, known);
    if !exchange.earlier.is_empty() {
        let _ = writeln!(text, "\nEarlier in the chat:");
        for turn in &exchange.earlier {
            let _ = writeln!(text, "User: {}", clip(&turn.message, MAX_CONTEXT_CHARS));
            let _ = writeln!(text, "Beep: {}", clip(&turn.reply, MAX_CONTEXT_CHARS));
        }
    }
    let latest = &exchange.latest;
    let _ = writeln!(
        text,
        "\nLatest exchange:\nUser: {}",
        clip(&latest.message, MAX_TURN_CHARS)
    );
    let _ = write!(text, "Beep: {}", clip(&latest.reply, MAX_TURN_CHARS));
    let _ = write_actions(&mut text, &latest.actions);
    text.push('\n');
    text
}

/// What the learner reads besides chats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadingKind {
    Email,
    Slack,
    /// A stretch of a Beep group chat, read for one member.
    Group,
}

impl ReadingKind {
    pub(crate) fn source(self) -> MemorySource {
        match self {
            ReadingKind::Email => MemorySource::Email {},
            ReadingKind::Slack => MemorySource::Slack {},
            // Members said it in Beep itself, so like a chat it may be used in groups.
            ReadingKind::Group => MemorySource::Learned {},
        }
    }

    pub(crate) fn intro(self) -> &'static str {
        match self {
            ReadingKind::Email => EMAIL_INTRO,
            ReadingKind::Slack => SLACK_INTRO,
            ReadingKind::Group => GROUP_INTRO,
        }
    }
}

/// An email or a Slack conversation for the learner.
pub struct Reading {
    pub kind: ReadingKind,
    /// Tells attempts apart in the log; never content.
    pub id: String,
    /// Searched to bring up the memories it may touch.
    pub query: String,
    /// What the learner reads, headed by what it is.
    pub text: String,
}

const EMAIL_INTRO: &str = "You keep the long-term memory of Beep, a personal assistant app. \
You get one email from the user's own mailbox: either sent by the user, or received by them. Decide what it \
teaches about the user that is worth remembering. Most emails teach nothing lasting, such as newsletters, \
marketing, notifications and routine receipts; then reply {\"ops\": []}. A quick screen let this email \
through because it might be about the user, but the screen is loose and you are the final judge. \
The older the email, the higher the bar: from an email more than a few weeks old keep only stable facts, like \
where the user lives, who they know, what they do and what they like, and nothing about what they were doing then.";

const SLACK_INTRO: &str = "You keep the long-term memory of Beep, a personal assistant app. \
You get a stretch of one Slack conversation around messages the user wrote, messages that mention them, or \
direct messages to them, with the user's own messages marked. Decide what it teaches about the user that is worth \
remembering. Most chat teaches nothing lasting, such as status updates, quick questions, links, jokes and small \
talk; then reply {\"ops\": []}. Facts about the user's work count, like their role, team and the projects they own, \
but not the day-to-day of tasks and tickets. One exception: a request made to the user with a date or deadline \
still ahead, like someone asking them to send something by Friday, is kept as a short fact naming who asked, with \
valid_until on that day. Drop requests without a date.";

const GROUP_INTRO: &str = "You keep the long-term memory of Beep, a personal assistant app, for one member \
of a Beep group chat, called the user here. You get the latest stretch of the chat with the user's own messages \
marked. Only the last message is new; the ones before it are context and were read already. Decide what the last \
message teaches about the user that is worth remembering, whether the user says it or another member says it about \
them, like a friend saying the user cannot eat soy. Plans the user takes part in count, like a trip the group is \
taking. When the last message asks Beep to remember, like \"remember that\" or \"recuérdanoslo\", the earlier messages \
it points to are what to learn from. Most group chat teaches nothing lasting about the user, such as jokes, greetings, Beep's suggestions, or \
facts about other members; then reply {\"ops\": []}.";

/// How the learner judges and answers, whatever it reads.
pub(crate) const LEARN_RULES: &str = "Worth keeping: the user's name, the people in their life and how they relate, where \
they live and work, homes and property they own, their job and ongoing projects, stable likes, dislikes and dietary \
needs, health conditions and insurance, beliefs, religion and politics, money: the banks, investments and debts \
they have, residency and visas, routines, important dates, goals, and plans such as trips, visits and \
appointments that are still ahead. Only what the text clearly shows about the user, not about anyone else, and \
only what is still likely true today. \
Never keep one-off errands and admin (utilities, meters, deliveries, repairs, support requests), single \
payments and statements, events and episodes that only mattered at the time, plans that are already over by \
today, guesses (nothing \"likely\" or \"probably\"), general knowledge, or secrets and numbers such as \
passwords, codes, tokens, card, account, contract or ID numbers.
Known memories are listed with their ids. If a fact is already known, do nothing. If the text shows a known \
fact changed or extends it, update it by id with the full new text. Forget a memory by id only when the text \
shows it is no longer true; never because the text does not mention it. Otherwise add it.
A fact that only holds for a while (a trip, a visit, a stay, an appointment, a temporary job) gets valid_from \
and/or valid_until as YYYY-MM-DD, worked out from the date the text was written. Lasting facts leave both out.
Write each memory as one short self-contained sentence about the user in third person that names its subject, \
like \"Has a dog named Rex.\" or \"Works at Acme as a designer.\", in the language most known memories are in, \
English when there are none. Never translate a known memory.
Reply with only JSON: {\"ops\": [{\"op\": \"add\" | \"update\" | \"forget\", \"id\": string, \"content\": string, \
\"valid_from\": string, \"valid_until\": string, \"quote\": string}]}. \
id only for update and forget; content only for add and update. quote, for add and update, is the \
few words of the text that show the fact, copied exactly, at most 20 words. At most 5 ops.";

pub(crate) fn reading_prompt(known: &[Hit], reading: &Reading) -> String {
    let mut text = String::new();
    let _ = writeln!(text, "Today: {}", Utc::now().format("%A %Y-%m-%d"));
    write_known(&mut text, known);
    let _ = writeln!(text, "\n{}", reading.text);
    text
}

/// What curation is told the memories are about.
fn curated_subject(scope: &Scope) -> &'static str {
    match scope {
        Scope::User => "one user",
        Scope::Machine(_) => "one of the user's machines",
        Scope::Project(_) => "one of the user's projects",
        Scope::Account(_) => "one of the user's accounts",
    }
}

const CURATE_RULES: &str = "Source \"user\" means the user said it or asked Beep to remember it; \"learned\" \
means Beep picked it up from a chat; \"email\" means Beep read it in the user's email; \"slack\" means Beep \
read it in the user's Slack.
Clean it up:
- Keep one independently useful fact per memory, with enough context to identify its subject. Split a memory \
that contains distinct facts into separate self-contained sentences. Keep dates and qualifications; only split \
when all resulting facts have the same days as the original. Do not invent facts.
- Merge only memories that say the same fact with the same meaning and days. A shared subject or topic is not \
enough. Keep each distinct detail in its own memory.
- When two memories clearly contradict each other, keep the newer one and supersede the older with it, unless the older \
was said by the user and the newer was only learned; then keep both. \
A superseded memory is kept as the newer one's history; never forget the losing side of a contradiction. \
Differences in time or context may explain both statements; when unsure, keep both.
- Forget memories that are trivial, a one-off task, general knowledge rather than about what these memories \
are about, or a secret such as a password or code. Never forget a memory said by the user unless its days are over.
- A memory marked over has passed. When it still says something lasting, like a place the user went, \
someone they met or a job they did, update it into a lasting fact in the past tense that names the month and year, \
like \"Went to Lisbon in October 2026.\", with no days. Forget it when nothing about it lasts, like an \
appointment or a deadline.
- Fix a memory that is unclear without context so it names its subject.
Leave everything else alone; most memories need nothing. Each id may appear in at most one op.
";

const CURATE_PROFILE: &str = "Then pick the profile: the ids of at most 15 memories that matter in nearly every chat, such as the user's \
name, how they want Beep to answer, their closest people, where they live and work, and their job, mostly from \
identity, people, preference and work. Not projects, \
passing interests or anything with days. Memories marked core are in it now; keep it stable and change it only \
for a better pick. A merged memory is named by any of its ids.
";

const CURATE_REPLY: &str = "Write memories as one short self-contained sentence in third person. Never translate: keep a memory in the \
language it is already in, and write a merge in the language most of its memories are in, English when unsure.
Reply with only JSON: {\"ops\": [{\"op\": \"merge\", \"ids\": [string], \"content\": string, \
\"valid_from\": string, \"valid_until\": string} | {\"op\": \"update\", \"id\": string, \"content\": string, \
\"valid_from\": string, \"valid_until\": string} | {\"op\": \"split\", \"id\": string, \"contents\": [string]} | \
{\"op\": \"supersede\", \"id\": string, \"by\": string} | \
{\"op\": \"forget\", \"id\": string}]}. \
Leave out days for lasting facts. \
At most 40 ops, [] when nothing needs doing.";

/// The curation instructions for the memories in `scope`. Only the user's
/// own scope picks the profile.
pub(crate) fn curate_system(scope: &Scope) -> String {
    let mut text = format!(
        "You tidy the long-term memory of Beep, a personal assistant app. \
You get every memory about {} with its id, category, source, last change date and, for facts that only \
hold for a while, their days. {CURATE_RULES}",
        curated_subject(scope)
    );
    if *scope == Scope::User {
        text.push_str(CURATE_PROFILE);
    }
    text.push_str(CURATE_REPLY);
    if *scope == Scope::User {
        text.push_str(" Add \"core\": [string], the profile's ids, next to \"ops\".");
    }
    text
}

pub(crate) fn curate_prompt(rows: &[MemoryRow], today: NaiveDate) -> String {
    let mut text = String::new();
    let _ = writeln!(text, "Today: {}\n\nMemories:", today.format("%Y-%m-%d"));
    let mut rows: Vec<&MemoryRow> = rows.iter().collect();
    rows.sort_by(|a, b| a.category.cmp(&b.category));
    for row in rows {
        let ended = if row.valid.ended(today) { ", over" } else { "" };
        let core = if row.core { ", core" } else { "" };
        let _ = writeln!(
            text,
            "- {} [{}, {}, {}]{}{ended}{core}: {}",
            row.id,
            row.category,
            row.source.as_str(),
            row.updated(),
            days(&row.valid),
            row.content
        );
    }
    text
}

fn date(text: Option<&str>) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(text?.trim(), "%Y-%m-%d").ok()
}

/// Days a model gave, or `None` when it gave neither.
fn validity(from: Option<&str>, until: Option<&str>) -> Option<Validity> {
    let valid = Validity {
        from: date(from),
        until: date(until),
    };
    (valid != Validity::default()).then_some(valid)
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum LearnKind {
    Add,
    Update,
    Forget,
}

#[derive(Debug, Deserialize)]
pub(crate) struct LearnOp {
    pub(crate) op: LearnKind,
    #[serde(default)]
    pub(crate) id: Option<String>,
    #[serde(default)]
    pub(crate) content: Option<String>,
    #[serde(default)]
    pub(crate) valid_from: Option<String>,
    #[serde(default)]
    pub(crate) valid_until: Option<String>,
    #[serde(default)]
    pub(crate) quote: Option<String>,
    /// What the fact is about, as the memory watcher names it, such as
    /// `project:beep`. The other learners leave it out.
    #[serde(default)]
    pub(crate) scope: Option<String>,
}

impl LearnOp {
    /// The fact to store or update, `None` for a forget or an empty one.
    pub(crate) fn fact(&self) -> Option<&str> {
        if self.op == LearnKind::Forget {
            return None;
        }
        self.content
            .as_deref()
            .map(str::trim)
            .filter(|content| !content.is_empty())
    }

    pub(crate) fn validity(&self) -> Option<Validity> {
        validity(self.valid_from.as_deref(), self.valid_until.as_deref())
    }

    /// Where the fact came from: `reference` names the chat, email or Slack
    /// thread, and the model's quote shows the words.
    pub(crate) fn origin(&self, reference: &str) -> Origin {
        Origin {
            reference: reference.to_owned(),
            quote: self
                .quote
                .as_deref()
                .map(str::trim)
                .filter(|quote| !quote.is_empty())
                .map(str::to_owned),
        }
    }
}

#[derive(Deserialize)]
struct Ops<T> {
    ops: Vec<T>,
}

pub(crate) fn parse_learn_ops(answer: &str) -> Result<Vec<LearnOp>> {
    // serde_json errors name a position, never the input.
    let Ops { ops } = serde_json::from_str::<Ops<LearnOp>>(unfence(answer))?;
    ensure!(ops.len() <= MAX_LEARN_OPS * 2, "too many memory ops");
    Ok(ops.into_iter().take(MAX_LEARN_OPS).collect())
}

#[derive(Debug, Deserialize)]
#[serde(tag = "op", rename_all = "lowercase")]
pub(crate) enum CurateOp {
    Split {
        id: String,
        contents: Vec<String>,
    },
    Merge {
        ids: Vec<String>,
        #[serde(flatten)]
        text: CuratedText,
    },
    Update {
        id: String,
        #[serde(flatten)]
        text: CuratedText,
    },
    /// `id` is out of date: `by` says what holds now.
    Supersede {
        id: String,
        by: String,
    },
    Forget {
        id: String,
    },
}

#[derive(Debug, Deserialize)]
pub(crate) struct CuratedText {
    content: String,
    #[serde(default)]
    valid_from: Option<String>,
    #[serde(default)]
    valid_until: Option<String>,
}

/// What one curation pass answered.
#[derive(Debug, Deserialize)]
pub(crate) struct Curation {
    pub(crate) ops: Vec<CurateOp>,
    /// The profile's ids, `None` when the answer left it out, which keeps it.
    #[serde(default)]
    pub(crate) core: Option<Vec<String>>,
}

pub(crate) fn parse_curation(answer: &str) -> Result<Curation> {
    let mut curation = serde_json::from_str::<Curation>(unfence(answer))?;
    ensure!(
        curation.ops.len() <= MAX_CURATE_OPS * 2,
        "too many curation ops"
    );
    curation.ops.truncate(MAX_CURATE_OPS);
    Ok(curation)
}

/// The profile's ids once `steps` are done: a merged-away id stands for the
/// memory it went into, and forgotten or unknown ids are dropped.
pub(crate) fn core_ids(
    core: Vec<String>,
    steps: &[Step],
    rows: &HashMap<&str, &MemoryRow>,
) -> HashSet<String> {
    let mut merged = HashMap::new();
    let mut forgotten = HashSet::new();
    for step in steps {
        match step {
            Step::Split { .. } => {}
            Step::Forget(id) => {
                forgotten.insert(id.as_str());
            }
            Step::Rewrite { keep, drop, .. } => {
                for id in drop {
                    merged.insert(id.as_str(), keep.as_str());
                }
            }
            Step::Supersede { id, by } => {
                merged.insert(id.as_str(), by.as_str());
            }
        }
    }
    let mut ids = HashSet::new();
    for id in &core {
        let id = merged.get(id.as_str()).copied().unwrap_or(id);
        if rows.contains_key(id) && !forgotten.contains(id) && ids.len() < MAX_CORE {
            ids.insert(id.to_owned());
        }
    }
    ids
}

/// Whether a fact with days holds on `today`: it has an end and has started.
fn current(valid: &Validity, today: NaiveDate) -> bool {
    valid.until.is_some_and(|until| until >= today) && valid.from.is_none_or(|from| from <= today)
}

/// The facts a profile is made of, in order: what holds right now, then the
/// core memories, each with its days. Only memories about the user: those
/// about a machine, project or account come up where they are used.
pub(crate) fn profile_facts(rows: &[MemoryRow], today: NaiveDate) -> Vec<String> {
    let about_user = || rows.iter().filter(|row| row.scope == Scope::User);
    let now = about_user().filter(|row| current(&row.valid, today));
    let core = about_user()
        .filter(|row| row.core && !row.valid.ended(today) && !current(&row.valid, today));
    now.chain(core)
        .map(|row| format!("{}{}", row.content, days(&row.valid)))
        .collect()
}

/// The memory that says which languages the user speaks, such as "Speaks
/// Spanish and English.", newest first.
pub(crate) fn languages(rows: &[MemoryRow], today: NaiveDate) -> Option<&str> {
    rows.iter()
        .filter(|row| row.scope == Scope::User && !row.valid.ended(today))
        .map(|row| row.content.as_str())
        .find(|content| {
            let content = content.to_lowercase();
            content.starts_with("speaks ") || content.contains(" speaks ")
        })
}

/// The profile a chat's context carries: `profile_facts` cut to
/// `MAX_PROFILE_CHARS`. `None` when there is nothing to say.
pub(crate) fn profile(rows: &[MemoryRow], today: NaiveDate) -> Option<String> {
    let mut text = String::from(PROFILE_HEADER);
    let mut any = false;
    for fact in profile_facts(rows, today) {
        let line = format!("\n- {fact}");
        // A fact too long for what is left gives way to shorter ones.
        if text.chars().count() + line.chars().count() > MAX_PROFILE_CHARS {
            continue;
        }
        text.push_str(&line);
        any = true;
    }
    any.then_some(text)
}

/// A memory's new text from curation.
#[derive(Debug, PartialEq)]
pub(crate) struct NewText {
    pub(crate) content: String,
    pub(crate) source: MemorySource,
    pub(crate) valid: Option<Validity>,
}

/// One checked curation change.
#[derive(Debug, PartialEq)]
pub(crate) enum Step {
    /// Replaces one memory with independently searchable facts.
    Split { id: String, contents: Vec<String> },
    /// Deletes a memory: trivia, a one-off or a secret.
    Forget(String),
    /// Keeps `id` as history of `by`, which contradicts and replaces it.
    Supersede { id: String, by: String },
    /// Rewrites `keep` and deletes `drop`, which it now covers.
    Rewrite {
        keep: String,
        fact: NewText,
        drop: Vec<String>,
    },
}

/// The days a merge of `rows` gets. While any of them still holds, the merge
/// runs from the earliest start to the latest end, whatever days the model
/// gave, so a merge never cuts a fact short. Once all are over (or none has
/// days) the model's days stand, and none makes a lasting fact. `None` refuses
/// to merge a lasting fact with one that still has days, since the result
/// would either lose the end or end the lasting fact.
fn merged_days(rows: &[&MemoryRow], asked: Option<Validity>, today: NaiveDate) -> Option<Validity> {
    let dated: Vec<&Validity> = rows
        .iter()
        .map(|row| &row.valid)
        .filter(|valid| valid.from.is_some() || valid.until.is_some())
        .collect();
    if dated.iter().all(|valid| valid.ended(today)) {
        return Some(asked.unwrap_or_default());
    }
    if dated.len() < rows.len() {
        return None;
    }
    let from = dated
        .iter()
        .map(|valid| valid.from)
        .collect::<Option<Vec<_>>>();
    let until = dated
        .iter()
        .map(|valid| valid.until)
        .collect::<Option<Vec<_>>>();
    Some(Validity {
        from: from.and_then(|days| days.into_iter().min()),
        until: until.and_then(|days| days.into_iter().max()),
    })
}

/// Checks the model's ops against the memories it was shown: unknown and
/// reused ids are dropped, and a memory the user stated is only forgotten
/// once its days are over. A merge keeps a stated memory when it has one,
/// and the result counts as stated.
pub(crate) fn plan_curation(
    ops: Vec<CurateOp>,
    rows: &HashMap<&str, &MemoryRow>,
    today: NaiveDate,
) -> Vec<Step> {
    let mut used = HashSet::new();
    let mut steps = Vec::new();
    let mut claim = |ids: &[String]| -> Option<Vec<&MemoryRow>> {
        let unique: HashSet<&str> = ids.iter().map(String::as_str).collect();
        if unique.len() != ids.len() || ids.iter().any(|id| used.contains(id.as_str())) {
            return None;
        }
        let found: Vec<&MemoryRow> = ids
            .iter()
            .map(|id| rows.get(id.as_str()).copied())
            .collect::<Option<_>>()?;
        used.extend(found.iter().map(|row| row.id.as_str()));
        Some(found)
    };
    let text = |text: CuratedText, source| {
        let content = text.content.trim().to_owned();
        (!content.is_empty()).then(|| NewText {
            valid: validity(text.valid_from.as_deref(), text.valid_until.as_deref()),
            content,
            source,
        })
    };
    for op in ops {
        match op {
            CurateOp::Split { id, contents } => {
                let contents: Vec<String> =
                    contents.into_iter().map(|s| s.trim().to_owned()).collect();
                if !(2..=5).contains(&contents.len())
                    || contents
                        .iter()
                        .any(|s| s.is_empty() || s.chars().count() > 4000)
                    || contents.iter().collect::<HashSet<_>>().len() != contents.len()
                {
                    continue;
                }
                if claim(slice::from_ref(&id)).is_some() {
                    steps.push(Step::Split { id, contents });
                }
            }
            CurateOp::Forget { id } => {
                let Some(row) = rows.get(id.as_str()) else {
                    continue;
                };
                if row.source == (MemorySource::User {}) && !row.valid.ended(today) {
                    continue;
                }
                if claim(slice::from_ref(&id)).is_some() {
                    steps.push(Step::Forget(id));
                }
            }
            CurateOp::Supersede { id, by } => {
                let (Some(old), Some(new)) = (rows.get(id.as_str()), rows.get(by.as_str())) else {
                    continue;
                };
                // A learned memory does not overrule what the user said.
                if id == by
                    || (old.source == (MemorySource::User {})
                        && new.source != (MemorySource::User {})
                        && !old.valid.ended(today))
                {
                    continue;
                }
                if claim(slice::from_ref(&id)).is_some() {
                    steps.push(Step::Supersede { id, by });
                }
            }
            CurateOp::Update { id, text: new } => {
                let Some(found) = claim(slice::from_ref(&id)) else {
                    continue;
                };
                // An update without days keeps a memory's days, but one that is
                // over becomes the lasting fact it was rewritten into.
                let ended = found[0].valid.ended(today);
                if let Some(mut fact) = text(new, found[0].source) {
                    if ended && fact.valid.is_none() {
                        fact.valid = Some(Validity::default());
                    }
                    steps.push(Step::Rewrite {
                        keep: id,
                        fact,
                        drop: Vec::new(),
                    });
                }
            }
            CurateOp::Merge { ids, text: new } => {
                if ids.len() < 2 {
                    continue;
                }
                let Some(found) = claim(&ids) else {
                    continue;
                };
                // Mail or Slack in a merge keeps it out of groups, even beside
                // what the user said; otherwise a stated fact keeps its source.
                let private = found.iter().copied().find(|r| !shareable(r.source));
                let stated = found
                    .iter()
                    .copied()
                    .find(|r| r.source == (MemorySource::User {}));
                let (keep, source) = match (private, stated) {
                    (Some(row), _) => (row, row.source),
                    (None, Some(row)) => (row, MemorySource::User {}),
                    (None, None) => (found[0], MemorySource::Learned {}),
                };
                let keep = keep.id.clone();
                let Some(fact) = text(new, source) else {
                    continue;
                };
                let Some(valid) = merged_days(&found, fact.valid, today) else {
                    continue;
                };
                let fact = NewText {
                    valid: Some(valid),
                    ..fact
                };
                let drop = ids.into_iter().filter(|id| *id != keep).collect();
                steps.push(Step::Rewrite { keep, fact, drop });
            }
        }
    }
    follow_supersedes(steps)
}

/// Points each supersede at the memory its `by` ends up as: the one it was
/// merged into, or nothing when `by` itself is forgotten or superseded, in
/// which case the older memory stays as it is.
fn follow_supersedes(steps: Vec<Step>) -> Vec<Step> {
    let mut gone = HashSet::new();
    let mut merged = HashMap::new();
    for step in &steps {
        match step {
            Step::Split { .. } => {}
            Step::Forget(id) | Step::Supersede { id, .. } => {
                gone.insert(id.clone());
            }
            Step::Rewrite { keep, drop, .. } => {
                for id in drop {
                    merged.insert(id.clone(), keep.clone());
                }
            }
        }
    }
    steps
        .into_iter()
        .filter_map(|step| match step {
            Step::Supersede { id, by } => {
                let by = merged.get(&by).cloned().unwrap_or(by);
                (!gone.contains(&by) && by != id).then_some(Step::Supersede { id, by })
            }
            other => Some(other),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: &str, content: &str) -> MemoryRow {
        MemoryRow {
            id: id.into(),
            content: content.into(),
            category: "identity".into(),
            source: MemorySource::Learned {},
            updated_at: "2026-01-01 10:00:00".into(),
            valid: Validity::default(),
            origin: None,
            core: false,
            scope: Scope::User,
        }
    }

    fn day(text: &str) -> NaiveDate {
        NaiveDate::parse_from_str(text, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn curation_keeps_what_the_user_said() {
        let mut stated = row("u1", "Prefers tea.");
        stated.source = MemorySource::User {};
        let learned = row("l1", "Likes tea.");
        let other = row("l2", "Asked about flights once.");
        let mut trip = row("u2", "Is in Rome.");
        trip.source = MemorySource::User {};
        trip.valid.until = Some(day("2026-09-01"));
        let rows: HashMap<&str, &MemoryRow> = [&stated, &learned, &other, &trip]
            .into_iter()
            .map(|r| (r.id.as_str(), r))
            .collect();
        let ops = parse_curation(
            r#"{"ops":[
            {"op":"forget","id":"u1"},
            {"op":"merge","ids":["l1","u1"],"content":"Prefers tea."},
            {"op":"forget","id":"l1"},
            {"op":"forget","id":"missing"},
            {"op":"forget","id":"l2"},
            {"op":"forget","id":"u2"}]}"#,
        )
        .unwrap()
        .ops;
        let steps = plan_curation(ops, &rows, day("2026-10-03"));
        assert_eq!(
            steps,
            vec![
                Step::Rewrite {
                    keep: "u1".into(),
                    fact: NewText {
                        content: "Prefers tea.".into(),
                        source: MemorySource::User {},
                        valid: Some(Validity::default()),
                    },
                    drop: vec!["l1".into()],
                },
                Step::Forget("l2".into()),
                Step::Forget("u2".into()),
            ]
        );
    }

    #[test]
    fn a_contradicted_memory_is_superseded_not_forgotten() {
        let old = row("l1", "Works at Acme.");
        let new = row("l2", "Works at Globex.");
        let merged = row("l3", "Started at Globex in May.");
        let mut stated = row("u1", "Lives in Porto.");
        stated.source = MemorySource::User {};
        let moved = row("l4", "Lives in Lisbon.");
        let rows: HashMap<&str, &MemoryRow> = [&old, &new, &merged, &stated, &moved]
            .into_iter()
            .map(|r| (r.id.as_str(), r))
            .collect();
        let ops = parse_curation(
            r#"{"ops":[
            {"op":"supersede","id":"l1","by":"l2"},
            {"op":"merge","ids":["l3","l2"],"content":"Works at Globex since May."},
            {"op":"supersede","id":"u1","by":"l4"},
            {"op":"supersede","id":"missing","by":"l4"}]}"#,
        )
        .unwrap()
        .ops;
        let steps = plan_curation(ops, &rows, day("2026-10-03"));
        assert_eq!(
            steps[0],
            Step::Supersede {
                id: "l1".into(),
                by: "l3".into(),
            },
            "a supersede follows its newer memory into a merge"
        );
        assert_eq!(steps.len(), 2, "a learned memory never overrules the user");
        let core = core_ids(vec!["l1".into()], &steps, &rows);
        assert_eq!(core, HashSet::from(["l3".to_owned()]));
    }

    #[test]
    fn an_ended_memory_rewritten_without_days_lasts() {
        let mut trip = row("t1", "Is in Lisbon.");
        trip.valid = Validity {
            from: Some(day("2026-09-20")),
            until: Some(day("2026-09-27")),
        };
        let mut stay = row("t2", "Is staying with Ana.");
        stay.valid.until = Some(day("2026-10-10"));
        let rows: HashMap<&str, &MemoryRow> = [&trip, &stay]
            .into_iter()
            .map(|r| (r.id.as_str(), r))
            .collect();
        let ops = parse_curation(
            r#"{"ops":[
            {"op":"update","id":"t1","content":"Went to Lisbon in September 2026."},
            {"op":"update","id":"t2","content":"Is staying with Ana, a friend."}]}"#,
        )
        .unwrap()
        .ops;
        let steps = plan_curation(ops, &rows, day("2026-10-03"));
        let valid: Vec<Option<Validity>> = steps
            .into_iter()
            .map(|step| match step {
                Step::Rewrite { fact, .. } => fact.valid,
                Step::Forget(_) | Step::Supersede { .. } | Step::Split { .. } => {
                    panic!("unexpected step")
                }
            })
            .collect();
        assert_eq!(valid, vec![Some(Validity::default()), None]);
    }

    #[test]
    fn a_merge_never_cuts_a_fact_short() {
        let mut flight = row("f", "Flies SQ012 to Tokyo.");
        flight.valid = Validity {
            from: Some(day("2026-10-09")),
            until: Some(day("2026-10-09")),
        };
        let mut stay = row("s", "Is staying at a hotel in Tokyo.");
        stay.valid = Validity {
            from: Some(day("2026-10-09")),
            until: Some(day("2026-11-06")),
        };
        let home = row("h", "Lives in Dubai.");
        let rows: HashMap<&str, &MemoryRow> = [&flight, &stay, &home]
            .into_iter()
            .map(|r| (r.id.as_str(), r))
            .collect();
        let merge = |ids: &str| {
            let ops = parse_curation(&format!(
                r#"{{"ops":[{{"op":"merge","ids":{ids},"content":"Flies to Tokyo.",
                "valid_from":"2026-10-09","valid_until":"2026-10-09"}}]}}"#
            ))
            .unwrap()
            .ops;
            plan_curation(ops, &rows, day("2026-10-08"))
        };
        let Step::Rewrite { fact, .. } = &merge(r#"["f","s"]"#)[0] else {
            panic!("expected a rewrite");
        };
        assert_eq!(fact.valid, Some(stay.valid));
        assert!(merge(r#"["s","h"]"#).is_empty());
    }

    #[test]
    fn a_merge_with_mail_in_it_stays_mail() {
        let mut bank = row("b", "Banks with Santander.");
        bank.source = MemorySource::Email {};
        let mut loan = row("l", "Has a mortgage.");
        loan.source = MemorySource::User {};
        let rows: HashMap<&str, &MemoryRow> = [&bank, &loan]
            .into_iter()
            .map(|r| (r.id.as_str(), r))
            .collect();
        let ops = parse_curation(
            r#"{"ops":[{"op":"merge","ids":["l","b"],"content":"Has a mortgage with Santander."}]}"#,
        )
        .unwrap()
        .ops;
        let Step::Rewrite { keep, fact, .. } = &plan_curation(ops, &rows, day("2026-10-03"))[0]
        else {
            panic!("expected a rewrite");
        };
        assert_eq!(fact.source, MemorySource::Email {});
        assert_eq!(keep, "b");
    }
}
