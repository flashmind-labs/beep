//! Personalized conversation starters for the empty chat screen.
//!
//! The model gets the user's memories, recent session titles, local time,
//! place and connected apps, and answers with four starters as JSON. A starter
//! tied to an event says until when it holds, and the set goes stale then. Anything else, a
//! provider error or a slow answer falls back to fixed starters. Memories are
//! never logged.

use crate::{
    protocol::{PromptSuggestion, PromptSuggestions},
    store::Location,
};
use anyhow::{Result, bail, ensure};
use chrono::{DateTime, Local, NaiveDateTime, TimeDelta, TimeZone, Utc};
use chrono_tz::Tz;
use flashmind::types::{
    AgentLlmConfig, CompletionRequest, LlmProvider, Message, ReasoningLevel, SamplingParams,
    StreamEvent,
};
use futures::StreamExt;
use serde::Deserialize;
use std::{
    fmt::Write,
    time::{Duration, Instant},
};

/// The app shows default starters while it waits, so this can be generous.
pub const TIMEOUT: Duration = Duration::from_secs(25);
pub(crate) const FRESH: Duration = Duration::from_secs(12 * 60 * 60);
/// A fallback is retried sooner than a real answer.
pub(crate) const FALLBACK_FRESH: Duration = Duration::from_secs(5 * 60);
const COUNT: usize = 4;
const MAX_TITLE_CHARS: usize = 28;
const MAX_PROMPT_CHARS: usize = 400;
const MAX_MEMORIES: usize = 40;
const MAX_MEMORY_CHARS: usize = 200;
const MAX_TITLES: usize = 8;
/// Moving farther than this, like flying to another city, makes new starters.
const MOVED_KM: f64 = 50.0;
/// The earliest a set goes stale, so a time already past does not make the
/// starters regenerate on every visit.
const MIN_STALE: TimeDelta = TimeDelta::minutes(30);

const SYSTEM: &str = "You write conversation starters for Beep, a personal assistant app on the user's phone. \
Reply with only a JSON array of exactly 4 objects, each {\"title\": string, \"prompt\": string, \"until\": string or null}, and nothing else. \
title: at most 28 characters, shown on a small chip. \
prompt: the full message the user would send, in first person, one or two sentences. \
until: when the starter stops being useful, as local time YYYY-MM-DDTHH:MM, for one tied to an event, \
like a few hours after a flight lands or the end of a meeting; null when it stays useful. \
Leave out starters about events that are already over, judging by the local time and place. \
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
    /// Where the phone last was, like "Marina Bay, Singapore".
    pub place: Option<String>,
    pub connected_apps: Vec<String>,
}

/// Starters from the model and when the first of them stops being useful.
pub struct Starters {
    pub suggestions: Vec<PromptSuggestion>,
    pub stale_at: Option<DateTime<Utc>>,
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

    /// Reads a local time without an offset, as the model writes it.
    fn read(&self, text: &str) -> Option<DateTime<Utc>> {
        let naive = NaiveDateTime::parse_from_str(text.trim(), "%Y-%m-%dT%H:%M").ok()?;
        let local = match self.timezone {
            Some(tz) => tz
                .from_local_datetime(&naive)
                .earliest()?
                .with_timezone(&Utc),
            None => Local
                .from_local_datetime(&naive)
                .earliest()?
                .with_timezone(&Utc),
        };
        Some(local)
    }
}

impl StarterContext {
    fn render(&self) -> String {
        let mut text = String::new();
        let _ = writeln!(text, "Local time: {}", self.now.describe());
        if let Some(place) = &self.place {
            let _ = writeln!(text, "Location: {}", clip(place, 80));
        }
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
    #[serde(default)]
    until: Option<String>,
}

/// Parses the model's answer: a JSON array of exactly four valid starters,
/// optionally inside a Markdown code fence. An `until` that does not parse
/// is ignored, and one in the past counts as soon.
fn parse(answer: &str, now: &LocalTime) -> Result<Starters> {
    // serde_json errors name a position, never the input.
    let raw: Vec<RawSuggestion> = serde_json::from_str(unfence(answer))?;
    ensure!(raw.len() == COUNT, "expected {COUNT} suggestions");
    let soonest = now.time + MIN_STALE;
    let stale_at = raw
        .iter()
        .filter_map(|s| now.read(s.until.as_deref()?))
        .map(|at| at.max(soonest))
        .min();
    let suggestions = raw
        .into_iter()
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
        .collect::<Result<_>>()?;
    Ok(Starters {
        suggestions,
        stale_at,
    })
}

/// One model call.
#[derive(Clone, Copy)]
pub struct Generator<'a> {
    pub provider: &'a dyn LlmProvider,
    /// The assistant's model and provider routing; reasoning and budget are set here.
    pub llm: &'a AgentLlmConfig,
    pub user: &'a str,
}

impl Generator<'_> {
    pub(crate) async fn ask(&self, context: &StarterContext) -> Result<Starters> {
        parse(&self.text(SYSTEM, context.render()).await?, &context.now)
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

pub(crate) struct Cached {
    pub(crate) expires: Instant,
    pub(crate) suggestions: PromptSuggestions,
    /// Where the phone was when they were made.
    pub(crate) location: Option<Location>,
}

impl Cached {
    /// Still fresh for a phone at `here`: not past its time, and not made
    /// before the phone moved to another city.
    pub(crate) fn fresh(&self, here: Option<&Location>) -> bool {
        let moved = match (&self.location, here) {
            (Some(then), Some(now)) => km_between(then, now) > MOVED_KM,
            (None, Some(_)) => true,
            (_, None) => false,
        };
        self.expires > Instant::now() && !moved
    }
}

/// Great-circle distance in kilometers.
fn km_between(a: &Location, b: &Location) -> f64 {
    const EARTH_KM: f64 = 6371.0;
    let (lat1, lat2) = (a.latitude.to_radians(), b.latitude.to_radians());
    let dlat = lat2 - lat1;
    let dlon = (b.longitude - a.longitude).to_radians();
    let h = (dlat / 2.0).sin().powi(2) + lat1.cos() * lat2.cos() * (dlon / 2.0).sin().powi(2);
    2.0 * EARTH_KM * h.sqrt().asin()
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
        let now = singapore("2026-10-04T22:00:00+08:00");
        let parsed = parse(good, &now).unwrap();
        assert_eq!(parsed.suggestions.len(), 4);
        assert_eq!(parsed.suggestions[3].title, "Inbox");
        assert_eq!(parsed.stale_at, None);

        assert!(parse("Sure! Here are some ideas", &now).is_err());
        assert!(parse(r#"[{"title":"a","prompt":"b"}]"#, &now).is_err());
        let long = format!(
            r#"[{{"title":"{}","prompt":"p"}},{{"title":"a","prompt":"b"}},{{"title":"a","prompt":"b"}},{{"title":"a","prompt":"b"}}]"#,
            "x".repeat(29)
        );
        assert!(parse(&long, &now).is_err());
        let extra = r#"[{"title":"a","prompt":"b","x":1},{"title":"a","prompt":"b"},{"title":"a","prompt":"b"},{"title":"a","prompt":"b"}]"#;
        assert!(parse(extra, &now).is_err());
    }

    fn singapore(time: &str) -> LocalTime {
        LocalTime {
            time: time.parse().expect("valid time"),
            timezone: Some(Tz::Asia__Singapore),
        }
    }

    #[test]
    fn starters_go_stale_at_the_first_until() {
        let now = singapore("2026-10-04T22:00:00+08:00");
        let answer = r#"[{"title":"a","prompt":"b","until":"2026-10-05T14:00"},
 {"title":"a","prompt":"b","until":"2026-10-05T09:30"},
 {"title":"a","prompt":"b","until":null},
 {"title":"a","prompt":"b","until":"soon"}]"#;
        let stale_at = parse(answer, &now).unwrap().stale_at;
        assert_eq!(stale_at, Some("2026-10-05T01:30:00Z".parse().unwrap()));

        // A time already past waits a little instead of regenerating at once.
        let past = answer.replace("2026-10-05T09:30", "2026-10-04T08:00");
        let stale_at = parse(&past, &now).unwrap().stale_at;
        assert_eq!(stale_at, Some(now.time + MIN_STALE));
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

    fn at(latitude: f64, longitude: f64) -> Location {
        Location {
            latitude,
            longitude,
            accuracy: 10.0,
            timezone: "UTC".into(),
            place: None,
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn starters_go_stale_after_moving_to_another_city() {
        let dubai = at(25.2, 55.27);
        let cached = Cached {
            expires: Instant::now() + FRESH,
            suggestions: PromptSuggestions {
                suggestions: fallback(),
                personal: true,
            },
            location: Some(dubai.clone()),
        };
        assert!(cached.fresh(Some(&dubai)));
        assert!(cached.fresh(Some(&at(25.25, 55.3))));
        assert!(cached.fresh(None));
        assert!(!cached.fresh(Some(&at(1.29, 103.85))));
    }
}
