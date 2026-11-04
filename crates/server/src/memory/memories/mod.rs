//! Long-term memory about each user: recall, the per-chat profile, learning after each chat and curation per scope.
//! Memory text is never logged.

mod curation;
mod prompts;
pub(crate) use curation::{
    CurateOp, LearnKind, LearnOp, Step, core_ids, languages, parse_curation, parse_learn_ops,
    plan_curation, profile, profile_facts,
};
pub(crate) use prompts::{
    LEARN_RULES, LEARN_SYSTEM, curate_prompt, curate_system, learn_prompt, reading_prompt,
};
pub use prompts::{Reading, ReadingKind};

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
    store::{MemoryRow, Scope, Validity},
};
use anyhow::{Context, Result};
use chrono::NaiveDate;
use flashmind::memory::{EmbeddingProvider, OpenAIEmbedding, OpenRouterEmbedding};
use serde_json::{Value, json};
use std::{
    fmt::{self},
    sync::Arc,
    time::Duration,
};
use tokio::time::{error::Elapsed, interval};

/// Same name on OpenAI and OpenRouter, so stored embeddings stay comparable.
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

/// Most memories one hourly sweep files, and how many it asks Jev about at once.
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
/// Longest profile, in characters.
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
    /// What a `memory` recall returns for this memory.
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

/// A memory change made by `learn`, shown in the chat as its memory tool.
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

#[cfg(test)]
mod tests;
