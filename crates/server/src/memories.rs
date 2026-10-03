//! Long-term memory about each user.
//!
//! A memory is one sentence about the user with an id, an embedding, who it
//! came from, and the days it holds when it only holds for a while, like a
//! trip. Memories are told apart by meaning: one that says the same as an
//! existing memory replaces it. Recall ranks by meaning, with a bonus for
//! shared words and a nudge for recent ones, falls back to words alone when
//! embeddings are unavailable, and leaves out facts whose days are over.
//! Nothing is put in front of the model unasked: it searches with
//! `memory_recall`. After each chat a cheap model call reads the latest
//! exchange, with the turns before it for context, and adds, corrects or
//! forgets facts. Once a day a curation pass merges duplicates, settles
//! contradictions and drops what went stale. Memory text is never logged.

use crate::{
    app::App,
    mail::Email,
    protocol::MemorySource,
    starters::{Generator, unfence},
    store::{Embedded, Fact, MEMORY_CATEGORIES, MemoryRow, Store, Validity},
};
use anyhow::{Result, ensure};
use chrono::{NaiveDate, NaiveDateTime, Utc};
use flashmind::{
    memory::EmbeddingProvider,
    types::{AgentLlmConfig, LlmProvider},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    fmt::Write,
    sync::Arc,
    time::{Duration, Instant},
};

/// The same model under its OpenAI and OpenRouter names, so stored
/// embeddings stay comparable whichever provider is configured.
pub const EMBEDDING_MODEL: &str = "text-embedding-3-small";
pub const OPENROUTER_EMBEDDING_MODEL: &str = "openai/text-embedding-3-small";
/// Embedding calls sit in front of every turn, so they get little time.
const EMBED_TIMEOUT: Duration = Duration::from_secs(4);
/// The run stays open while learning, so it gets little time.
const LEARN_TIMEOUT: Duration = Duration::from_secs(15);
/// Mail sync runs in the background, so a slow model can take its time.
const EMAIL_LEARN_TIMEOUT: Duration = Duration::from_secs(90);
const CURATE_TIMEOUT: Duration = Duration::from_secs(90);
/// Missing embeddings filled per lookup.
const BACKFILL: usize = 64;
/// Two texts this close say the same thing.
const DUPLICATE: f32 = 0.88;
/// How close a memory must be to show up in `memory_recall`.
const RECALLABLE: f32 = 0.22;
const MAX_LEARN_OPS: usize = 5;
const MAX_TURN_CHARS: usize = 1500;
const MAX_EMAIL_CHARS: usize = 4000;
/// Earlier turns the extractor sees to resolve "she" or "that place".
pub const LEARN_CONTEXT_TURNS: usize = 3;
const MAX_CONTEXT_CHARS: usize = 600;
/// A user's memories are curated at most this often, and only after a change.
const CURATE_EVERY: Duration = Duration::from_secs(24 * 60 * 60);
const CURATE_SWEEP: Duration = Duration::from_secs(60 * 60);
/// Newest memories one curation pass reads.
const MAX_CURATED: usize = 200;
const MAX_CURATE_OPS: usize = 40;
/// Memories whose days are over are kept this long, then deleted.
const ENDED_KEPT_DAYS: u32 = 30;

const STOPWORDS: &[&str] = &[
    "the", "and", "for", "with", "that", "this", "what", "when", "where", "who", "how", "are",
    "was", "you", "your", "can", "could", "would", "should", "have", "has", "from", "about",
    "please", "tell", "know", "does", "did", "not", "but", "any", "some", "just", "like",
];

#[derive(Clone)]
pub struct Memories {
    store: Store,
    embedder: Option<Arc<dyn EmbeddingProvider>>,
}

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
    /// Meaning alone, 0 without embeddings.
    pub similarity: f32,
    /// Share of the query's words it contains.
    overlap: f32,
    score: f32,
}

impl Hit {
    /// What `memory_recall` returns for this memory.
    pub fn to_json(&self) -> Value {
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
        value
    }
}

/// One user message and Beep's reply to it.
pub struct Turn {
    pub message: String,
    pub reply: String,
}

/// What the extractor reads after a chat.
pub struct Exchange {
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

impl Memories {
    pub fn new(store: Store, embedder: Option<Arc<dyn EmbeddingProvider>>) -> Self {
        Self { store, embedder }
    }

    async fn embed(&self, texts: &[&str]) -> Option<Vec<Vec<f32>>> {
        let embedder = self.embedder.as_ref()?;
        if texts.is_empty() {
            return Some(Vec::new());
        }
        match tokio::time::timeout(EMBED_TIMEOUT, embedder.embed_batch(texts)).await {
            Ok(Ok(vectors)) if vectors.len() == texts.len() => Some(vectors),
            Ok(Ok(_)) => {
                log::warn!("memory embedding returned the wrong count");
                None
            }
            Ok(Err(error)) => {
                log::warn!("memory embedding failed: {error}");
                None
            }
            Err(_) => {
                log::warn!("memory embedding timed out");
                None
            }
        }
    }

    async fn embed_one(&self, text: &str) -> Option<Vec<f32>> {
        self.embed(&[text]).await.and_then(|mut v| v.pop())
    }

    /// A user's memories, embedding the ones that have none yet.
    async fn rows(&self, user: &str) -> Result<Vec<MemoryRow>> {
        let mut rows = self.store.memory_rows(user).await?;
        let dims = self.embedder.as_ref().map(|e| e.dimensions());
        let missing: Vec<usize> = rows
            .iter()
            .enumerate()
            .filter(|(_, row)| row.embedding.as_ref().map(Vec::len) != dims)
            .map(|(i, _)| i)
            .take(BACKFILL)
            .collect();
        if dims.is_none() || missing.is_empty() {
            return Ok(rows);
        }
        let texts: Vec<&str> = missing.iter().map(|&i| rows[i].content.as_str()).collect();
        if let Some(vectors) = self.embed(&texts).await {
            let mut embedded = Vec::new();
            for (&i, vector) in missing.iter().zip(vectors) {
                embedded.push(Embedded {
                    id: rows[i].id.clone(),
                    content: rows[i].content.clone(),
                    embedding: vector.clone(),
                });
                rows[i].embedding = Some(vector);
            }
            if let Err(error) = self.store.set_embeddings(user, embedded).await {
                log::warn!("unable to save memory embeddings: {error}");
            }
        }
        Ok(rows)
    }

    /// Memories that still hold, ranked against `query`. An empty query lists
    /// the newest.
    pub async fn search(
        &self,
        user: &str,
        query: &str,
        category: Option<&str>,
        limit: usize,
    ) -> Result<Vec<Hit>> {
        let query = query.trim();
        let rows = if query.is_empty() {
            self.store.memory_rows(user).await?
        } else {
            self.rows(user).await?
        };
        let today = Utc::now().date_naive();
        let rows = rows
            .into_iter()
            .filter(|row| !row.valid.ended(today))
            .filter(|row| category.is_none_or(|c| row.category == c));
        if query.is_empty() {
            return Ok(rows
                .take(limit)
                .map(|row| hit(row, 0.0, 0.0, 0.0))
                .collect());
        }
        let vector = self.embed_one(query).await;
        Ok(rank(rows, query, vector.as_deref())
            .into_iter()
            .filter(|h| h.similarity >= RECALLABLE || h.overlap > 0.0)
            .take(limit)
            .collect())
    }

    /// Stores a fact. When it says the same as a memory, that one is replaced.
    pub async fn remember(&self, user: &str, fact: Fact<'_>) -> Result<Remembered> {
        let Some(vector) = self.embed_one(fact.content).await else {
            let id = self.store.add_memory(user, fact).await?;
            return Ok(Remembered {
                id,
                replaced: false,
            });
        };
        let rows = self.rows(user).await?;
        let closest = rows
            .iter()
            .filter_map(|row| Some((cosine(&vector, row.embedding.as_ref()?), row)))
            .max_by(|a, b| a.0.total_cmp(&b.0));
        let fact = Fact {
            embedding: Some(vector),
            ..fact
        };
        if let Some((similarity, row)) = closest
            && similarity >= DUPLICATE
        {
            // The new text replaces the old one whole, days included.
            let replacement = Fact {
                valid: Some(fact.valid.unwrap_or_default()),
                ..fact.clone()
            };
            if self.store.update_memory(user, &row.id, replacement).await? {
                return Ok(Remembered {
                    id: row.id.clone(),
                    replaced: true,
                });
            }
            // Deleted meanwhile: add it instead.
        }
        let id = self.store.add_memory(user, fact).await?;
        Ok(Remembered {
            id,
            replaced: false,
        })
    }

    /// Replaces a memory's text. Returns whether it exists.
    pub async fn update(&self, user: &str, id: &str, fact: Fact<'_>) -> Result<bool> {
        let embedding = self.embed_one(fact.content).await;
        self.store
            .update_memory(user, id, Fact { embedding, ..fact })
            .await
    }

    /// Reads a finished exchange and updates memory with what it teaches about
    /// the user, returning the changes made. Failures are logged and dropped.
    pub async fn learn(&self, generator: Generator<'_>, exchange: Exchange) -> Vec<Learned> {
        let user = generator.user.to_owned();
        let attempt = async {
            if exchange.latest.message.split_whitespace().count() < 3 {
                return Ok(Vec::new());
            }
            let query = format!(
                "{}\n{}",
                exchange.latest.message,
                clip(&exchange.latest.reply, 300)
            );
            let known = self.search(&user, &query, None, 15).await?;
            let prompt = learn_prompt(&known, &exchange);
            let answer = generator.text(LEARN_SYSTEM, prompt).await?;
            let ops = parse_learn_ops(&answer)?;
            let mut learned = Vec::new();
            for op in ops {
                learned.extend(
                    self.apply(&user, &known, op, MemorySource::Learned {})
                        .await?,
                );
            }
            anyhow::Ok(learned)
        };
        settle(&user, tokio::time::timeout(LEARN_TIMEOUT, attempt).await)
    }

    /// Reads one email from the user's mailbox and updates memory with what it
    /// teaches about them, the same way `learn` does for a chat.
    pub async fn learn_email(&self, generator: Generator<'_>, email: &Email) -> Vec<Learned> {
        let user = generator.user.to_owned();
        let started = Instant::now();
        let attempt = async {
            let query = format!("{}\n{}", email.subject, clip(&email.body, 300));
            let mut known = self.search(&user, &query, None, 15).await?;
            // Any email can mention the user's name or family, which a search
            // on its text would not bring up, so personal facts always come along.
            for hit in self.search(&user, "", Some("personal"), 15).await? {
                if !known.iter().any(|k| k.id == hit.id) {
                    known.push(hit);
                }
            }
            let prompt = email_prompt(&known, email);
            let answer = generator.text(EMAIL_SYSTEM, prompt).await?;
            let mut learned = Vec::new();
            for op in parse_learn_ops(&answer)? {
                learned.extend(
                    self.apply(&user, &known, op, MemorySource::Email {})
                        .await?,
                );
            }
            anyhow::Ok(learned)
        };
        let learned = settle(
            &user,
            tokio::time::timeout(EMAIL_LEARN_TIMEOUT, attempt).await,
        );
        log::debug!(
            "memory email learning took {:.1}s; id={}",
            started.elapsed().as_secs_f32(),
            email.id
        );
        learned
    }

    async fn apply(
        &self,
        user: &str,
        known: &[Hit],
        op: LearnOp,
        source: MemorySource,
    ) -> Result<Option<Learned>> {
        let shown = op
            .id
            .as_deref()
            .and_then(|id| known.iter().find(|h| h.id == id));
        if op.op == LearnKind::Forget {
            // Only what the extractor was shown can be forgotten.
            let Some(hit) = shown else {
                return Ok(None);
            };
            let forgotten = self.store.forget(user, &hit.id).await?;
            return Ok(forgotten.then(|| Learned {
                change: Change::Forgotten,
                id: hit.id.clone(),
                content: hit.content.clone(),
            }));
        }
        let Some(content) = op
            .content
            .as_deref()
            .map(str::trim)
            .filter(|c| !c.is_empty())
        else {
            return Ok(None);
        };
        let fact = Fact {
            content,
            category: op
                .category
                .as_deref()
                .filter(|c| MEMORY_CATEGORIES.contains(c)),
            source,
            valid: op.validity(),
            embedding: None,
        };
        // An update of a memory it was not shown is a new fact.
        if let Some(hit) = shown
            && self.update(user, &hit.id, fact.clone()).await?
        {
            return Ok(Some(Learned {
                change: Change::Updated,
                id: hit.id.clone(),
                content: content.to_owned(),
            }));
        }
        let fact = Fact {
            valid: Some(fact.valid.unwrap_or_default()),
            ..fact
        };
        let remembered = self.remember(user, fact).await?;
        Ok(Some(Learned {
            change: if remembered.replaced {
                Change::Updated
            } else {
                Change::Stored
            },
            id: remembered.id,
            content: content.to_owned(),
        }))
    }

    /// Merges duplicates, settles contradictions and drops stale memories of
    /// one user, returning how many memories changed.
    pub async fn curate(&self, generator: Generator<'_>) -> Result<usize> {
        let user = generator.user.to_owned();
        let rows: Vec<MemoryRow> = self
            .store
            .memory_rows(&user)
            .await?
            .into_iter()
            .take(MAX_CURATED)
            .collect();
        if rows.len() < 2 {
            return Ok(0);
        }
        let today = Utc::now().date_naive();
        let attempt = async {
            let answer = generator
                .text(CURATE_SYSTEM, curate_prompt(&rows, today))
                .await?;
            parse_curate_ops(&answer)
        };
        let ops = tokio::time::timeout(CURATE_TIMEOUT, attempt)
            .await
            .map_err(|_| anyhow::anyhow!("curation timed out"))??;
        let by_id: HashMap<&str, &MemoryRow> = rows.iter().map(|r| (r.id.as_str(), r)).collect();
        let plan = plan_curation(ops, &by_id, today);
        let mut changed = 0;
        for step in plan {
            match step {
                Step::Forget(id) => {
                    changed += usize::from(self.store.forget(&user, &id).await?);
                }
                Step::Rewrite { keep, fact, drop } => {
                    let fact = Fact {
                        content: &fact.content,
                        category: fact.category.as_deref(),
                        source: fact.source,
                        valid: fact.valid,
                        embedding: None,
                    };
                    if !self.update(&user, &keep, fact).await? {
                        continue;
                    }
                    changed += 1;
                    for id in drop {
                        changed += usize::from(self.store.forget(&user, &id).await?);
                    }
                }
            }
        }
        Ok(changed)
    }
}

impl Memories {
    /// Deletes memories whose days ended a while ago and curates every user
    /// whose memories changed since the last pass.
    pub async fn curate_due(&self, provider: &dyn LlmProvider, llm: &AgentLlmConfig) {
        match self.store.purge_ended_memories(ENDED_KEPT_DAYS).await {
            Ok(0) => {}
            Ok(purged) => log::info!("purged {purged} ended memories"),
            Err(error) => log::warn!("unable to purge ended memories: {error}"),
        }
        let users = match self.store.users_to_curate(CURATE_EVERY).await {
            Ok(users) => users,
            Err(error) => {
                log::warn!("unable to list users to curate: {error}");
                return;
            }
        };
        for user in users {
            let generator = Generator {
                provider,
                llm,
                user: &user,
            };
            match self.curate(generator).await {
                Ok(0) => {}
                Ok(changed) => log::info!("memory curation changed {changed}; user={user}"),
                Err(error) => log::info!("memory curation skipped: {error}; user={user}"),
            }
            // Marked even after a failure, so a bad answer is retried tomorrow
            // rather than every hour.
            if let Err(error) = self.store.mark_curated(&user).await {
                log::warn!("unable to mark memories curated: {error}; user={user}");
            }
        }
    }
}

/// Curates memories of users whose memories changed, once an hour.
pub fn start_curation(app: Arc<App>) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(CURATE_SWEEP);
        loop {
            interval.tick().await;
            app.curate_memories().await;
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
    /// The memory tool that makes the same change.
    pub fn tool(self) -> &'static str {
        match self {
            Change::Stored => "memory_store",
            Change::Updated => "memory_update",
            Change::Forgotten => "memory_forget",
        }
    }
}

impl Learned {
    pub fn arguments(&self) -> Value {
        match self.change {
            Change::Forgotten => json!({"id": self.id}),
            _ => json!({"id": self.id, "content": self.content}),
        }
    }

    /// Matches what the memory tools return.
    pub fn output(&self) -> String {
        match self.change {
            Change::Stored => json!({"stored": true, "id": self.id}),
            Change::Updated => json!({"updated": true, "id": self.id}),
            Change::Forgotten => json!({"forgotten": true}),
        }
        .to_string()
    }
}

/// Logs how a learning attempt went and returns its changes, none on failure.
fn settle(
    user: &str,
    outcome: Result<Result<Vec<Learned>>, tokio::time::error::Elapsed>,
) -> Vec<Learned> {
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

fn clip(text: &str, chars: usize) -> String {
    text.chars().take(chars).collect()
}

fn hit(row: MemoryRow, similarity: f32, overlap: f32, score: f32) -> Hit {
    Hit {
        date: row.updated().to_owned(),
        id: row.id,
        content: row.content,
        category: row.category,
        source: row.source,
        valid: row.valid,
        similarity,
        overlap,
        score,
    }
}

fn rank(
    rows: impl IntoIterator<Item = MemoryRow>,
    query: &str,
    vector: Option<&[f32]>,
) -> Vec<Hit> {
    let words = words(query);
    let now = Utc::now().naive_utc();
    let mut hits: Vec<Hit> = rows
        .into_iter()
        .filter_map(|row| {
            let similarity = match (vector, row.embedding.as_deref()) {
                (Some(q), Some(e)) => cosine(q, e),
                _ => 0.0,
            };
            let overlap = overlap(&words, &row.content);
            if similarity <= 0.0 && overlap <= 0.0 {
                return None;
            }
            let age_days = NaiveDateTime::parse_from_str(&row.updated_at, "%Y-%m-%d %H:%M:%S")
                .map(|t| (now - t).num_days().max(0) as f32)
                .unwrap_or(365.0);
            let score = similarity + 0.25 * overlap + 0.03 * (-age_days / 90.0).exp();
            Some(hit(row, similarity, overlap, score))
        })
        .collect();
    hits.sort_by(|a, b| b.score.total_cmp(&a.score));
    hits
}

fn words(text: &str) -> HashSet<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .map(str::to_lowercase)
        .filter(|w| w.chars().count() >= 3 && !STOPWORDS.contains(&w.as_str()))
        .collect()
}

/// Share of the query's words found in `text`.
fn overlap(query: &HashSet<String>, text: &str) -> f32 {
    if query.is_empty() {
        return 0.0;
    }
    let text = words(text);
    query.iter().filter(|w| text.contains(*w)).count() as f32 / query.len() as f32
}

fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let (mut dot, mut na, mut nb) = (0.0, 0.0, 0.0);
    for (x, y) in a.iter().zip(b) {
        dot += x * y;
        na += x * x;
        nb += y * y;
    }
    if na == 0.0 || nb == 0.0 {
        0.0
    } else {
        dot / (na.sqrt() * nb.sqrt())
    }
}

/// How a memory's days read in a prompt, empty for a lasting fact.
fn days(valid: &Validity) -> String {
    match (valid.from, valid.until) {
        (None, None) => String::new(),
        (Some(from), None) => format!(" (from {from})"),
        (None, Some(until)) => format!(" (until {until})"),
        (Some(from), Some(until)) => format!(" ({from} to {until})"),
    }
}

const LEARN_SYSTEM: &str = "You keep the long-term memory of Beep, a personal assistant app. \
Read the latest exchange and decide what it teaches about the user that is worth remembering. \
Earlier turns are only there to explain the latest one; do not learn from them again.
First triage it: casual chatter, a one-off task, or something personal. Most exchanges teach nothing lasting; then reply {\"ops\": []}.
Worth keeping: the user's name, the people and pets in their life and how they relate (a person the user asks \
about by name is someone in their life), where they live and work, \
their job and ongoing projects, stable likes, dislikes and dietary needs, routines, important dates, goals, \
plans such as trips, visits and appointments, and how they want Beep to answer. Only what the user said or clearly \
confirmed. Never guesses, general knowledge, what Beep did, the content of a one-off request, or secrets such as \
passwords, codes, tokens, card or account numbers.
Known memories are listed with their ids. If a fact is already known, do nothing. If the exchange corrects or \
extends one, update it by id with the full new text. If the user says something is no longer true or asks \
Beep to forget it, forget it by id. Otherwise add it.
A fact that only holds for a while (a trip, a visit, a stay, an appointment, a temporary job) gets valid_from \
and/or valid_until as YYYY-MM-DD, worked out from today's date. Lasting facts leave both out.
Write each memory as one short self-contained sentence about the user in third person that names its subject, \
like \"Has a dog named Rex.\" or \"Is in Singapore at the Mandarin Oriental.\", in the language of the user's \
latest message. Never translate a known memory.
Reply with only JSON: {\"ops\": [{\"op\": \"add\" | \"update\" | \"forget\", \"id\": string, \"content\": string, \
\"category\": \"personal\" | \"preference\" | \"work\" | \"location\" | \"routine\" | \"general\", \
\"valid_from\": string, \"valid_until\": string}]}. \
id only for update and forget; content and category only for add and update. At most 5 ops.";

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

fn learn_prompt(known: &[Hit], exchange: &Exchange) -> String {
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
    let _ = writeln!(text, "Beep: {}", clip(&latest.reply, MAX_TURN_CHARS));
    text
}

const EMAIL_SYSTEM: &str = "You keep the long-term memory of Beep, a personal assistant app. \
You get one email from the user's own mailbox: either sent by the user, or received by them. Decide what it \
teaches about the user that is worth remembering. Most emails teach nothing lasting, such as newsletters, \
marketing, notifications and routine receipts; then reply {\"ops\": []}.
Worth keeping: the user's name, the people in their life and how they relate, where they live and work, their \
job and ongoing projects, stable likes, dislikes and dietary needs, routines, important dates, goals, and plans \
such as trips, visits and appointments that are still ahead. Only what the email clearly shows about the user, \
not about the sender or anyone else, and only what is still likely true today. \
Never keep one-off errands and admin (utilities, meters, deliveries, repairs, bookings, accounts, applications, \
support requests), events and episodes that only mattered at the time, plans that are already over by today, \
guesses, general knowledge, money, health, or secrets such as passwords, codes, tokens, card or account numbers. \
The older the email, the higher the bar: from an email more than a few weeks old keep only stable facts, like \
where the user lives, who they know, what they do and what they like, and nothing about what they were doing then.
Known memories are listed with their ids. If a fact is already known, do nothing. If the email shows a known \
fact changed or extends it, update it by id with the full new text. Forget a memory by id only when the email \
shows it is no longer true; never because the email does not mention it. Otherwise add it.
A fact that only holds for a while (a trip, a visit, a stay, an appointment, a temporary job) gets valid_from \
and/or valid_until as YYYY-MM-DD, worked out from the date the email was sent. Lasting facts leave both out.
Write each memory as one short self-contained sentence about the user in third person that names its subject, \
like \"Has a dog named Rex.\" or \"Works at Acme as a designer.\", in the language most known memories are in, \
English when there are none. Never translate a known memory.
Reply with only JSON: {\"ops\": [{\"op\": \"add\" | \"update\" | \"forget\", \"id\": string, \"content\": string, \
\"category\": \"personal\" | \"preference\" | \"work\" | \"location\" | \"routine\" | \"general\", \
\"valid_from\": string, \"valid_until\": string}]}. \
id only for update and forget; content and category only for add and update. At most 5 ops.";

fn email_prompt(known: &[Hit], email: &Email) -> String {
    let mut text = String::new();
    let _ = writeln!(text, "Today: {}", Utc::now().format("%A %Y-%m-%d"));
    write_known(&mut text, known);
    let _ = writeln!(text, "\nEmail:\n{}", email.render(MAX_EMAIL_CHARS));
    text
}

const CURATE_SYSTEM: &str = "You tidy the long-term memory of Beep, a personal assistant app. \
You get every memory about one user with its id, category, source, last change date and, for facts that only \
hold for a while, their days. Source \"user\" means the user said it or asked Beep to remember it; \"learned\" \
means Beep picked it up from a chat; \"email\" means Beep read it in the user's email.
Clean it up:
- Merge memories that say the same thing or are parts of one fact into one sentence, keeping every detail.
- When two memories contradict each other, keep the newer one, unless the older was said by the user and the \
newer was only learned; then merge them into one sentence that keeps the user's version.
- Forget memories that are trivial, a one-off task, general knowledge rather than about the user, or a secret \
such as a password or code. Forget learned memories whose days are over and that say nothing lasting. \
Never forget a memory said by the user unless its days are over.
- Fix a memory that is unclear without context so it names its subject.
Leave everything else alone; most memories need nothing. Each id may appear in at most one op.
Write memories as one short self-contained sentence in third person. Never translate: keep a memory in the \
language it is already in, and write a merge in the language most of its memories are in, English when unsure.
Reply with only JSON: {\"ops\": [{\"op\": \"merge\", \"ids\": [string], \"content\": string, \"category\": string, \
\"valid_from\": string, \"valid_until\": string} | {\"op\": \"update\", \"id\": string, \"content\": string, \
\"category\": string, \"valid_from\": string, \"valid_until\": string} | {\"op\": \"forget\", \"id\": string}]}. \
Categories: personal, preference, work, location, routine, general. Leave out days for lasting facts. \
At most 40 ops; {\"ops\": []} when nothing needs doing.";

fn curate_prompt(rows: &[MemoryRow], today: NaiveDate) -> String {
    let mut text = String::new();
    let _ = writeln!(text, "Today: {}\n\nMemories:", today.format("%Y-%m-%d"));
    let mut rows: Vec<&MemoryRow> = rows.iter().collect();
    rows.sort_by(|a, b| a.category.cmp(&b.category));
    for row in rows {
        let ended = if row.valid.ended(today) { ", over" } else { "" };
        let _ = writeln!(
            text,
            "- {} [{}, {}, {}]{}{ended}: {}",
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
enum LearnKind {
    Add,
    Update,
    Forget,
}

#[derive(Debug, Deserialize)]
struct LearnOp {
    op: LearnKind,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    valid_from: Option<String>,
    #[serde(default)]
    valid_until: Option<String>,
}

impl LearnOp {
    fn validity(&self) -> Option<Validity> {
        validity(self.valid_from.as_deref(), self.valid_until.as_deref())
    }
}

#[derive(Deserialize)]
struct Ops<T> {
    ops: Vec<T>,
}

fn parse_learn_ops(answer: &str) -> Result<Vec<LearnOp>> {
    // serde_json errors name a position, never the input.
    let Ops { ops } = serde_json::from_str::<Ops<LearnOp>>(unfence(answer))?;
    ensure!(ops.len() <= MAX_LEARN_OPS * 2, "too many memory ops");
    Ok(ops.into_iter().take(MAX_LEARN_OPS).collect())
}

#[derive(Debug, Deserialize)]
#[serde(tag = "op", rename_all = "lowercase")]
enum CurateOp {
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
    Forget {
        id: String,
    },
}

#[derive(Debug, Deserialize)]
struct CuratedText {
    content: String,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    valid_from: Option<String>,
    #[serde(default)]
    valid_until: Option<String>,
}

fn parse_curate_ops(answer: &str) -> Result<Vec<CurateOp>> {
    let Ops { ops } = serde_json::from_str::<Ops<CurateOp>>(unfence(answer))?;
    ensure!(ops.len() <= MAX_CURATE_OPS * 2, "too many curation ops");
    Ok(ops.into_iter().take(MAX_CURATE_OPS).collect())
}

/// A memory's new text from curation.
#[derive(Debug, PartialEq)]
struct NewText {
    content: String,
    category: Option<String>,
    source: MemorySource,
    valid: Option<Validity>,
}

/// One checked curation change.
#[derive(Debug, PartialEq)]
enum Step {
    Forget(String),
    /// Rewrites `keep` and deletes `drop`, which it now covers.
    Rewrite {
        keep: String,
        fact: NewText,
        drop: Vec<String>,
    },
}

/// Checks the model's ops against the memories it was shown: unknown and
/// reused ids are dropped, and a memory the user stated is only forgotten
/// once its days are over. A merge keeps a stated memory when it has one,
/// and the result counts as stated.
fn plan_curation(
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
            category: text
                .category
                .filter(|c| MEMORY_CATEGORIES.contains(&c.as_str())),
            content,
            source,
        })
    };
    for op in ops {
        match op {
            CurateOp::Forget { id } => {
                let Some(row) = rows.get(id.as_str()) else {
                    continue;
                };
                if row.source == (MemorySource::User {}) && !row.valid.ended(today) {
                    continue;
                }
                if claim(std::slice::from_ref(&id)).is_some() {
                    steps.push(Step::Forget(id));
                }
            }
            CurateOp::Update { id, text: new } => {
                let Some(found) = claim(std::slice::from_ref(&id)) else {
                    continue;
                };
                if let Some(fact) = text(new, found[0].source) {
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
                let stated = found.iter().find(|r| r.source == (MemorySource::User {}));
                let source = if stated.is_some() {
                    MemorySource::User {}
                } else {
                    MemorySource::Learned {}
                };
                let keep = stated.unwrap_or(&found[0]).id.clone();
                let Some(fact) = text(new, source) else {
                    continue;
                };
                // A merge without days is a lasting fact.
                let fact = NewText {
                    valid: Some(fact.valid.unwrap_or_default()),
                    ..fact
                };
                let drop = ids.into_iter().filter(|id| *id != keep).collect();
                steps.push(Step::Rewrite { keep, fact, drop });
            }
        }
    }
    steps
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: &str, content: &str, embedding: Option<Vec<f32>>) -> MemoryRow {
        MemoryRow {
            id: id.into(),
            content: content.into(),
            category: "personal".into(),
            source: MemorySource::Learned {},
            updated_at: "2026-01-01 10:00:00".into(),
            valid: Validity::default(),
            embedding,
        }
    }

    fn day(text: &str) -> NaiveDate {
        NaiveDate::parse_from_str(text, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn ranks_by_meaning_then_words() {
        let rows = vec![
            row("a1", "Has a dog named Rex.", Some(vec![1.0, 0.0])),
            row("b2", "Works as a nurse.", Some(vec![0.0, 1.0])),
        ];
        let hits = rank(rows.clone(), "my pet", Some(&[0.9, 0.1]));
        assert_eq!(hits[0].id, "a1");

        // Without embeddings only shared words count.
        let hits = rank(rows, "what is the dog called", None);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, "a1");
    }

    #[test]
    fn parses_learn_ops_with_days() {
        let ops = parse_learn_ops(
            r#"```json
{"ops":[{"op":"add","content":"Is in Singapore.","category":"location","valid_from":"2026-10-04","valid_until":"2026-10-09"},
{"op":"forget","id":"abc123"}]}
```"#,
        )
        .unwrap();
        assert_eq!(ops.len(), 2);
        assert_eq!(
            ops[0].validity(),
            Some(Validity {
                from: Some(day("2026-10-04")),
                until: Some(day("2026-10-09")),
            })
        );
        assert_eq!(ops[1].op, LearnKind::Forget);
        assert!(parse_learn_ops("Nothing to remember.").is_err());
        assert!(parse_learn_ops(r#"{"ops":[]}"#).unwrap().is_empty());
    }

    #[test]
    fn ended_facts_are_over_the_day_after() {
        let trip = Validity {
            from: Some(day("2026-10-04")),
            until: Some(day("2026-10-09")),
        };
        assert!(!trip.ended(day("2026-10-09")));
        assert!(trip.ended(day("2026-10-10")));
        assert!(!Validity::default().ended(day("2030-01-01")));
    }

    #[test]
    fn curation_keeps_what_the_user_said() {
        let mut stated = row("u1", "Prefers tea.", None);
        stated.source = MemorySource::User {};
        let learned = row("l1", "Likes tea.", None);
        let other = row("l2", "Asked about flights once.", None);
        let mut trip = row("u2", "Is in Rome.", None);
        trip.source = MemorySource::User {};
        trip.valid.until = Some(day("2026-09-01"));
        let rows: HashMap<&str, &MemoryRow> = [&stated, &learned, &other, &trip]
            .into_iter()
            .map(|r| (r.id.as_str(), r))
            .collect();
        let ops = parse_curate_ops(
            r#"{"ops":[
            {"op":"forget","id":"u1"},
            {"op":"merge","ids":["l1","u1"],"content":"Prefers tea."},
            {"op":"forget","id":"l1"},
            {"op":"forget","id":"missing"},
            {"op":"forget","id":"l2"},
            {"op":"forget","id":"u2"}]}"#,
        )
        .unwrap();
        let steps = plan_curation(ops, &rows, day("2026-10-03"));
        assert_eq!(
            steps,
            vec![
                Step::Rewrite {
                    keep: "u1".into(),
                    fact: NewText {
                        content: "Prefers tea.".into(),
                        category: None,
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
    fn cosine_handles_mismatch() {
        assert_eq!(cosine(&[1.0, 0.0], &[1.0]), 0.0);
        assert!((cosine(&[1.0, 1.0], &[2.0, 2.0]) - 1.0).abs() < 1e-6);
    }
}
