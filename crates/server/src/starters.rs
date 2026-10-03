//! Personalized conversation starters for the empty chat screen.
//!
//! The model gets the user's memories, recent session titles, local time and
//! connected apps, and answers with four starters as JSON. Anything else, a
//! provider error or a slow answer falls back to fixed starters. Memories are
//! never logged.

use crate::{
    AsyncMutex,
    protocol::{PromptSuggestion, PromptSuggestions},
};
use anyhow::{Result, bail, ensure};
use chrono::{DateTime, Local, Utc};
use chrono_tz::Tz;
use flashmind::types::{
    AgentLlmConfig, CompletionRequest, LlmProvider, Message, ReasoningLevel, SamplingParams,
    StreamEvent,
};
use futures::StreamExt;
use serde::Deserialize;
use std::{
    collections::HashMap,
    fmt::Write,
    time::{Duration, Instant},
};

/// The app shows default starters while it waits, so this can be generous.
pub const TIMEOUT: Duration = Duration::from_secs(25);
const FRESH: Duration = Duration::from_secs(12 * 60 * 60);
/// A fallback is retried sooner than a real answer.
const FALLBACK_FRESH: Duration = Duration::from_secs(5 * 60);
const COUNT: usize = 4;
const MAX_TITLE_CHARS: usize = 28;
const MAX_PROMPT_CHARS: usize = 400;
const MAX_MEMORIES: usize = 40;
const MAX_MEMORY_CHARS: usize = 200;
const MAX_TITLES: usize = 8;

const SYSTEM: &str = "You write conversation starters for Beep, a personal assistant app on the user's phone. \
Reply with only a JSON array of exactly 4 objects, each {\"title\": string, \"prompt\": string}, and nothing else. \
title: at most 28 characters, shown on a small chip. \
prompt: the full message the user would send, in first person, one or two sentences. \
Make them varied, specific to the context and useful at this time of day. \
Only use facts given in the context, and never include secrets. \
Write in the language the context suggests the user speaks, else English.";

/// Starters used when the model cannot help.
pub fn fallback() -> Vec<PromptSuggestion> {
    [
        (
            "Plan my day",
            "Help me plan the rest of my day. Ask what I have going on and suggest a simple schedule.",
        ),
        (
            "Set a reminder",
            "Remind me in an hour to take a short break.",
        ),
        (
            "Draft a message",
            "Help me write a short, friendly message. Ask me who it is for and what I want to say.",
        ),
        (
            "Find a place nearby",
            "Find a good place to eat near me that is open now.",
        ),
    ]
    .into_iter()
    .map(|(title, prompt)| PromptSuggestion {
        title: title.into(),
        prompt: prompt.into(),
    })
    .collect()
}

/// What the starters are personalized with.
pub struct StarterContext {
    pub memories: Vec<MemoryNote>,
    pub recent_titles: Vec<String>,
    pub now: LocalTime,
    pub connected_apps: Vec<String>,
}

pub struct MemoryNote {
    pub category: String,
    pub content: String,
}

pub struct LocalTime {
    pub time: DateTime<Utc>,
    /// The user's timezone, or `None` for the server's.
    pub timezone: Option<Tz>,
}

impl LocalTime {
    fn describe(&self) -> String {
        const FORMAT: &str = "%A %Y-%m-%d %H:%M";
        match self.timezone {
            Some(tz) => format!("{} ({tz})", self.time.with_timezone(&tz).format(FORMAT)),
            None => self.time.with_timezone(&Local).format(FORMAT).to_string(),
        }
    }
}

impl StarterContext {
    fn render(&self) -> String {
        let mut text = String::new();
        let _ = writeln!(text, "Local time: {}", self.now.describe());
        if !self.connected_apps.is_empty() {
            let _ = writeln!(text, "Connected apps: {}", self.connected_apps.join(", "));
        }
        if !self.recent_titles.is_empty() {
            let _ = writeln!(text, "\nRecent conversations:");
            for title in self.recent_titles.iter().take(MAX_TITLES) {
                let _ = writeln!(text, "- {}", clip(title, 80));
            }
        }
        if !self.memories.is_empty() {
            let _ = writeln!(text, "\nWhat the user has shared:");
            for memory in self.memories.iter().take(MAX_MEMORIES) {
                let content = clip(&memory.content, MAX_MEMORY_CHARS);
                let _ = writeln!(text, "- [{}] {content}", memory.category);
            }
        }
        text
    }
}

fn clip(text: &str, max: usize) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(max)
        .collect()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSuggestion {
    title: String,
    prompt: String,
}

/// Parses the model's answer: a JSON array of exactly four valid starters,
/// optionally inside a Markdown code fence.
fn parse(answer: &str) -> Result<Vec<PromptSuggestion>> {
    // serde_json errors name a position, never the input.
    let raw: Vec<RawSuggestion> = serde_json::from_str(unfence(answer))?;
    ensure!(raw.len() == COUNT, "expected {COUNT} suggestions");
    raw.into_iter()
        .map(|s| {
            let title = s.title.trim();
            let prompt = s.prompt.trim();
            ensure!(
                !title.is_empty() && title.chars().count() <= MAX_TITLE_CHARS,
                "bad title length"
            );
            ensure!(
                !prompt.is_empty() && prompt.chars().count() <= MAX_PROMPT_CHARS,
                "bad prompt length"
            );
            Ok(PromptSuggestion {
                title: title.to_owned(),
                prompt: prompt.to_owned(),
            })
        })
        .collect()
}

/// One model call.
pub struct Generator<'a> {
    pub provider: &'a dyn LlmProvider,
    /// The assistant's model and provider routing; reasoning and budget are set here.
    pub llm: &'a AgentLlmConfig,
    pub user: &'a str,
}

impl Generator<'_> {
    async fn ask(&self, context: &StarterContext) -> Result<Vec<PromptSuggestion>> {
        parse(&self.text(SYSTEM, context.render()).await?)
    }

    /// One plain completion with no tools, returning the whole answer.
    pub async fn text(&self, system: &str, prompt: String) -> Result<String> {
        let request = CompletionRequest {
            model: self.llm.model.clone(),
            messages: vec![Message::system(system), Message::user(prompt)],
            tools: Vec::new(),
            // Off leaves the provider default, and reasoning models then spend
            // a small budget thinking before writing any text.
            max_tokens: Some(4000),
            reasoning: ReasoningLevel::Low,
            sampling: SamplingParams::default(),
            modalities: Vec::new(),
            audio_config: None,
            image_config: None,
            user: Some(self.user.to_owned()),
            provider_preferences: self.llm.provider_preferences.clone(),
        };
        let mut stream = self.provider.complete(request);
        let mut answer = String::new();
        while let Some(event) = stream.next().await {
            match event {
                Ok(StreamEvent::ContentDelta(delta)) => answer.push_str(&delta),
                Ok(StreamEvent::ToolCallStart { .. }) => bail!("model called a tool"),
                Ok(StreamEvent::Finished(_)) => break,
                Ok(_) => {}
                // Provider errors can echo upstream bodies; only the fact is kept.
                Err(_) => bail!("provider error"),
            }
        }
        Ok(answer)
    }
}

/// Strips an optional Markdown code fence around a JSON answer.
pub(crate) fn unfence(answer: &str) -> &str {
    let text = answer.trim();
    match text.strip_prefix("```") {
        Some(rest) => {
            let rest = rest.strip_prefix("json").unwrap_or(rest);
            rest.strip_suffix("```").unwrap_or(rest).trim()
        }
        None => text,
    }
}

struct Cached {
    expires: Instant,
    suggestions: PromptSuggestions,
}

/// Per-user starters, kept for 12 hours.
#[derive(Default)]
pub struct Starters {
    cache: AsyncMutex<HashMap<String, Cached>>,
}

impl Starters {
    pub async fn cached(&self, user: &str) -> Option<PromptSuggestions> {
        let cache = self.cache.lock().await;
        let entry = cache.get(user).filter(|c| c.expires > Instant::now())?;
        Some(entry.suggestions.clone())
    }

    /// Asks the model, falling back on any failure, and caches the result.
    pub async fn generate(
        &self,
        generator: Generator<'_>,
        context: impl Future<Output = StarterContext>,
    ) -> PromptSuggestions {
        let user = generator.user.to_owned();
        let attempt = async {
            let context = context.await;
            generator.ask(&context).await
        };
        let (suggestions, personal) = match tokio::time::timeout(TIMEOUT, attempt).await {
            Ok(Ok(suggestions)) => (suggestions, true),
            Ok(Err(error)) => {
                log::info!("prompt suggestions fell back: {error}");
                (fallback(), false)
            }
            Err(_) => {
                log::info!("prompt suggestions fell back: timed out");
                (fallback(), false)
            }
        };
        let fresh = if personal { FRESH } else { FALLBACK_FRESH };
        let suggestions = PromptSuggestions {
            suggestions,
            personal,
        };
        let mut cache = self.cache.lock().await;
        cache.retain(|_, c| c.expires > Instant::now());
        cache.insert(
            user,
            Cached {
                expires: Instant::now() + fresh,
                suggestions: suggestions.clone(),
            },
        );
        suggestions
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_strictly() {
        let good = r#"```json
[{"title":"Plan the weekend","prompt":"Help me plan a relaxed weekend."},
 {"title":"Gym check-in","prompt":"Log today's workout."},
 {"title":"Dinner idea","prompt":"Suggest a quick dinner."},
 {"title":"Inbox","prompt":"Summarize my unread email."}]
```"#;
        let parsed = parse(good).unwrap();
        assert_eq!(parsed.len(), 4);
        assert_eq!(parsed[3].title, "Inbox");

        assert!(parse("Sure! Here are some ideas").is_err());
        assert!(parse(r#"[{"title":"a","prompt":"b"}]"#).is_err());
        let long = format!(
            r#"[{{"title":"{}","prompt":"p"}},{{"title":"a","prompt":"b"}},{{"title":"a","prompt":"b"}},{{"title":"a","prompt":"b"}}]"#,
            "x".repeat(29)
        );
        assert!(parse(&long).is_err());
        let extra = r#"[{"title":"a","prompt":"b","x":1},{"title":"a","prompt":"b"},{"title":"a","prompt":"b"},{"title":"a","prompt":"b"}]"#;
        assert!(parse(extra).is_err());
    }

    #[test]
    fn fallback_fits_chips() {
        let fallback = fallback();
        assert_eq!(fallback.len(), COUNT);
        assert!(
            fallback
                .iter()
                .all(|s| s.title.chars().count() <= MAX_TITLE_CHARS)
        );
    }
}
