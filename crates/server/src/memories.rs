//! Long-term memory about each user.
//!
//! Memories live in the `memories` table with an embedding each. Recall ranks
//! them by meaning, with a bonus for shared words and a nudge for recent ones,
//! and falls back to words alone when embeddings are unavailable. A memory
//! that says the same as an existing one updates it instead of adding a near
//! copy. After each chat a cheap model call reads the exchange and adds,
//! corrects or forgets facts, so memory does not depend on the assistant
//! remembering to call a tool. Memory text is never logged.

use crate::{
    starters::{Generator, unfence},
    store::{Embedded, MEMORY_CATEGORIES, MemoryRow, MemorySource, NewMemory, Store},
};
use anyhow::{Result, ensure};
use chrono::{NaiveDateTime, Utc};
use flashmind::memory::EmbeddingProvider;
use serde::Deserialize;
use std::{collections::HashSet, fmt::Write, sync::Arc, time::Duration};

pub const EMBEDDING_MODEL: &str = "openai/text-embedding-3-small";
/// Embedding calls sit in front of every turn, so they get little time.
const EMBED_TIMEOUT: Duration = Duration::from_secs(4);
const LEARN_TIMEOUT: Duration = Duration::from_secs(60);
/// Missing embeddings filled per lookup.
const BACKFILL: usize = 64;
/// Two texts this close say the same thing.
const DUPLICATE: f32 = 0.88;
/// How close a memory must be to the message to be put in front of the model.
const RELEVANT: f32 = 0.35;
/// How close a memory must be to show up in `memory_recall`.
const RECALLABLE: f32 = 0.22;
const CONTEXT_RELEVANT: usize = 5;
const CONTEXT_PROFILE: usize = 3;
const MAX_CONTEXT_CHARS: usize = 300;
const MAX_LEARN_OPS: usize = 5;
const MAX_EXCHANGE_CHARS: usize = 1500;

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
    pub key: String,
    pub content: String,
    pub category: String,
    /// `YYYY-MM-DD`, empty when unknown.
    pub date: String,
    /// Meaning alone, 0 without embeddings.
    pub similarity: f32,
    /// Share of the query's words it contains.
    overlap: f32,
    score: f32,
}

/// The finished chat turn the extractor reads.
pub struct Exchange {
    pub message: String,
    pub reply: String,
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
        let texts: Vec<String> = missing.iter().map(|&i| embed_text(&rows[i])).collect();
        let refs: Vec<&str> = texts.iter().map(String::as_str).collect();
        if let Some(vectors) = self.embed(&refs).await {
            let mut embedded = Vec::new();
            for (&i, vector) in missing.iter().zip(vectors) {
                embedded.push(Embedded {
                    key: rows[i].key.clone(),
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

    /// Memories ranked against `query`. An empty query lists the newest.
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
        let rows = rows
            .into_iter()
            .filter(|row| category.is_none_or(|c| row.category == c));
        if query.is_empty() {
            return Ok(rows
                .take(limit)
                .map(|row| hit(row, 0.0, 0.0, 0.0))
                .collect());
        }
        let vector = self.embed(&[query]).await.and_then(|mut v| v.pop());
        Ok(rank(rows, query, vector.as_deref())
            .into_iter()
            .filter(|h| h.similarity >= RECALLABLE || h.overlap > 0.0)
            .take(limit)
            .collect())
    }

    /// What Beep should know before answering `message`: a few profile facts and
    /// the memories closest to the message. Never fails; on error it is empty.
    pub async fn context(&self, user: &str, message: &str) -> Vec<Hit> {
        let rows = match self.rows(user).await {
            Ok(rows) => rows,
            Err(error) => {
                log::warn!("unable to load memories: {error}");
                return Vec::new();
            }
        };
        let mut picked: Vec<Hit> = rows
            .iter()
            .filter(|row| row.category == "personal")
            .take(CONTEXT_PROFILE)
            .cloned()
            .map(|row| hit(row, 0.0, 0.0, 0.0))
            .collect();
        if !message.trim().is_empty() {
            let vector = self.embed(&[message]).await.and_then(|mut v| v.pop());
            let relevant = rank(rows, message, vector.as_deref())
                .into_iter()
                .filter(|h| h.similarity >= RELEVANT || (vector.is_none() && h.overlap >= 0.5))
                .filter(|h| !picked.iter().any(|p| p.key == h.key))
                .take(CONTEXT_RELEVANT)
                .collect::<Vec<_>>();
            picked.extend(relevant);
        }
        picked
    }

    /// Stores a memory and returns the key it ended up under. When it says the
    /// same as another memory, that one is updated instead.
    pub async fn remember(&self, user: &str, memory: NewMemory<'_>) -> Result<String> {
        let mut key = memory.key.to_owned();
        let mut embedding = None;
        if self.embedder.is_some() {
            let rows = self.rows(user).await?;
            let probe = MemoryRow {
                key: key.clone(),
                content: memory.content.to_owned(),
                category: String::new(),
                updated_at: String::new(),
                embedding: None,
            };
            if let Some(vector) = self
                .embed(&[&embed_text(&probe)])
                .await
                .and_then(|mut v| v.pop())
            {
                let closest = rows
                    .iter()
                    .filter(|row| row.key != key)
                    .filter_map(|row| Some((cosine(&vector, row.embedding.as_ref()?), row)))
                    .max_by(|a, b| a.0.total_cmp(&b.0));
                if let Some((similarity, row)) = closest
                    && similarity >= DUPLICATE
                {
                    key = row.key.clone();
                }
                embedding = Some(vector);
            }
        }
        self.store
            .remember(
                user,
                NewMemory {
                    key: &key,
                    ..memory
                },
            )
            .await?;
        // The probe embedded the stored text under the original key; it is
        // only kept when the key did not change, else the next lookup redoes it.
        if let Some(embedding) = embedding
            && key == memory.key
        {
            let embedded = Embedded {
                key: key.clone(),
                content: memory.content.to_owned(),
                embedding,
            };
            if let Err(error) = self.store.set_embeddings(user, vec![embedded]).await {
                log::warn!("unable to save memory embedding: {error}");
            }
        }
        Ok(key)
    }

    /// Reads a finished exchange and updates memory with what it teaches about
    /// the user. Runs in the background; failures are logged and dropped.
    pub async fn learn(&self, generator: Generator<'_>, exchange: Exchange) {
        let user = generator.user.to_owned();
        let attempt = async {
            if exchange.message.split_whitespace().count() < 3 {
                return Ok(0);
            }
            let known = self.search(&user, &exchange.message, None, 12).await?;
            let prompt = learn_prompt(&known, &exchange);
            let answer = generator.text(LEARN_SYSTEM, prompt).await?;
            let ops = parse_ops(&answer)?;
            let mut applied = 0;
            for op in ops {
                if self.apply(&user, &known, op).await? {
                    applied += 1;
                }
            }
            anyhow::Ok(applied)
        };
        match tokio::time::timeout(LEARN_TIMEOUT, attempt).await {
            Ok(Ok(0)) => {}
            Ok(Ok(applied)) => log::info!("memory learned {applied} change(s); user={user}"),
            Ok(Err(error)) => log::info!("memory learning skipped: {error}"),
            Err(_) => log::info!("memory learning timed out"),
        }
    }

    async fn apply(&self, user: &str, known: &[Hit], op: Op) -> Result<bool> {
        match op.op {
            OpKind::Forget => {
                // Only what the extractor was shown can be forgotten.
                if !known.iter().any(|h| h.key == op.key) {
                    return Ok(false);
                }
                self.store.forget(user, &op.key).await
            }
            OpKind::Add | OpKind::Update => {
                let Some(content) = op
                    .content
                    .as_deref()
                    .map(str::trim)
                    .filter(|c| !c.is_empty())
                else {
                    return Ok(false);
                };
                let category = op
                    .category
                    .as_deref()
                    .filter(|c| MEMORY_CATEGORIES.contains(c))
                    .or_else(|| {
                        known
                            .iter()
                            .find(|h| h.key == op.key)
                            .map(|h| h.category.as_str())
                    });
                self.remember(
                    user,
                    NewMemory {
                        key: &op.key,
                        content,
                        category,
                        source: MemorySource::Auto,
                    },
                )
                .await?;
                Ok(true)
            }
        }
    }
}

/// The text embedded for a memory. The key often names the subject the
/// content leaves implicit ("dog_name": "Rex").
fn embed_text(row: &MemoryRow) -> String {
    format!("{}: {}", row.key.replace('_', " "), row.content)
}

fn hit(row: MemoryRow, similarity: f32, overlap: f32, score: f32) -> Hit {
    Hit {
        date: row.updated_at.get(..10).unwrap_or_default().to_owned(),
        key: row.key,
        content: row.content,
        category: row.category,
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
            let overlap = overlap(&words, &embed_text(&row));
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

/// The developer-message block for [`Memories::context`].
pub fn render_context(hits: &[Hit]) -> String {
    let mut text = String::new();
    if hits.is_empty() {
        return text;
    }
    let _ = writeln!(text, "What you remember about the user (may be relevant):");
    for h in hits {
        let content: String = h.content.chars().take(MAX_CONTEXT_CHARS).collect();
        let _ = writeln!(text, "- {} [{}]: {content}", h.key, h.category);
    }
    text
}

const LEARN_SYSTEM: &str = "You keep the long-term memory of Beep, a personal assistant app. \
Read the latest exchange and decide what it teaches about the user that is worth remembering for months.
First triage it: casual chatter, a one-off task, or something personal. Most exchanges teach nothing lasting; then reply {\"ops\": []}.
Worth keeping: the user's name, the people and pets in their life and how they relate, where they live and work, \
their job and ongoing projects, stable likes, dislikes and dietary needs, routines, important dates, goals, \
and how they want Beep to answer. Only what the user said or clearly confirmed. Never guesses, general knowledge, \
what Beep did, the content of a one-off request, or secrets such as passwords, codes, tokens, card or account numbers.
Known memories are listed with their keys. If a fact is already known, do nothing. If the exchange corrects or \
extends one, update it under its key with the full new text. If the user says something is no longer true or asks \
Beep to forget it, forget it by key. Otherwise add it under a new short snake_case key.
Write each memory as one short self-contained sentence about the user in third person, like \"Has a dog named Rex.\", \
in the language the user wrote in.
Reply with only JSON: {\"ops\": [{\"op\": \"add\" | \"update\" | \"forget\", \"key\": string, \"content\": string, \
\"category\": \"personal\" | \"preference\" | \"work\" | \"location\" | \"routine\" | \"general\"}]}. \
Leave out content and category for forget. At most 5 ops.";

fn learn_prompt(known: &[Hit], exchange: &Exchange) -> String {
    let clip = |text: &str| text.chars().take(MAX_EXCHANGE_CHARS).collect::<String>();
    let mut text = String::new();
    let _ = writeln!(text, "Today: {}", Utc::now().format("%Y-%m-%d"));
    let _ = writeln!(text, "\nKnown memories:");
    if known.is_empty() {
        let _ = writeln!(text, "(none related)");
    }
    for h in known {
        let _ = writeln!(
            text,
            "- {} [{}] ({}): {}",
            h.key, h.category, h.date, h.content
        );
    }
    let _ = writeln!(text, "\nExchange:\nUser: {}", clip(&exchange.message));
    let _ = writeln!(text, "Beep: {}", clip(&exchange.reply));
    text
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
enum OpKind {
    Add,
    Update,
    Forget,
}

#[derive(Debug, Deserialize)]
struct Op {
    op: OpKind,
    key: String,
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    category: Option<String>,
}

#[derive(Deserialize)]
struct Ops {
    ops: Vec<Op>,
}

fn parse_ops(answer: &str) -> Result<Vec<Op>> {
    // serde_json errors name a position, never the input.
    let Ops { ops } = serde_json::from_str(unfence(answer))?;
    ensure!(ops.len() <= MAX_LEARN_OPS * 2, "too many memory ops");
    Ok(ops
        .into_iter()
        .filter(|op| valid_key(&op.key))
        .take(MAX_LEARN_OPS)
        .collect())
}

fn valid_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 100
        && key
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(key: &str, content: &str, embedding: Option<Vec<f32>>) -> MemoryRow {
        MemoryRow {
            key: key.into(),
            content: content.into(),
            category: "personal".into(),
            updated_at: "2026-01-01 10:00:00".into(),
            embedding,
        }
    }

    #[test]
    fn ranks_by_meaning_then_words() {
        let rows = vec![
            row("dog_name", "Has a dog named Rex.", Some(vec![1.0, 0.0])),
            row("job", "Works as a nurse.", Some(vec![0.0, 1.0])),
        ];
        let hits = rank(rows.clone(), "my pet", Some(&[0.9, 0.1]));
        assert_eq!(hits[0].key, "dog_name");

        // Without embeddings only shared words count.
        let hits = rank(rows, "what is the dog called", None);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].key, "dog_name");
    }

    #[test]
    fn parses_ops_strictly() {
        let ops = parse_ops(
            r#"```json
{"ops":[{"op":"add","key":"dog_name","content":"Has a dog named Rex.","category":"personal"},
{"op":"forget","key":"old_job"},{"op":"add","key":"bad key!","content":"x"}]}
```"#,
        )
        .unwrap();
        assert_eq!(ops.len(), 2);
        assert_eq!(ops[1].op, OpKind::Forget);
        assert!(parse_ops("Nothing to remember.").is_err());
        assert!(parse_ops(r#"{"ops":[]}"#).unwrap().is_empty());
    }

    #[test]
    fn cosine_handles_mismatch() {
        assert_eq!(cosine(&[1.0, 0.0], &[1.0]), 0.0);
        assert!((cosine(&[1.0, 1.0], &[2.0, 2.0]) - 1.0).abs() < 1e-6);
    }
}
