//! Personalized conversation starters for the empty chat screen.
//!
//! The model gets the user's memories, recent session titles, local time,
//! place and connected apps, and answers with four starters as a Markdown
//! list. A starter
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
use std::{
    fmt::Write,
    time::{Duration, Instant},
};

/// The app shows default starters while it waits, so this can be generous.
pub const TIMEOUT: Duration = Duration::from_secs(25);
pub(crate) const FRESH: Duration = Duration::from_secs(12 * 60 * 60);
/// A fallback is retried sooner than a real answer.
pub(crate) const FALLBACK_FRESH: Duration = Duration::from_secs(5 * 60);
/// New starters a user may ask for within `STARTER_REFRESH_WINDOW`.
pub(crate) const STARTER_REFRESHES: usize = 3;
pub(crate) const STARTER_REFRESH_WINDOW: Duration = Duration::from_secs(10 * 60);
const COUNT: usize = 4;
const MAX_TITLE_CHARS: usize = 28;
const MAX_PROMPT_CHARS: usize = 400;
const MAX_MEMORIES: usize = 40;
const MAX_MEMORY_CHARS: usize = 200;
const MAX_TITLES: usize = 8;
/// How many recent actions the model sees.
pub(crate) const MAX_DONE: usize = 15;
const MAX_DONE_CHARS: usize = 240;
/// Moving farther than this, like flying to another city, makes new starters.
const MOVED_KM: f64 = 50.0;
/// The earliest a set goes stale, so a time already past does not make the
/// starters regenerate on every visit.
const MIN_STALE: TimeDelta = TimeDelta::minutes(30);

/// Fewer usable starters than this falls back.
const MIN_COUNT: usize = 2;

const SYSTEM: &str = "You write conversation starters for Beep, a personal assistant app on the user's phone. \
Answer with a Markdown list of 4 starters and nothing else, one per line as \"- title | prompt\", \
or \"- title | prompt | until YYYY-MM-DDTHH:MM\" for one tied to an event. \
title: at most 28 characters, shown on a small chip. Name the outcome the user gets, \
like \"Pick a gift for Sam\" or \"Dinner near the hotel\", never a vague topic like \"Email check\" or \"City offbeat\". \
prompt: the full message the user would send, in first person, one or two sentences, \
asking Beep to do or decide something concrete. \
until: the local time when the starter stops being useful, like a few hours after a flight lands \
or the end of a meeting; leave it out when it stays useful. \
Pick the 4 things that would help most right now, judging by the local time, place and what is coming up soon. \
Each starter covers a different need and a different kind of task: at most one about reading or answering email, \
and never two about the same trip, event or person. \
Memories say who the user knows and works with, not what they owe anyone: \
never suggest replying to or following up with someone unless the context names a message from them still waiting on the user. \
A starter about an email asks Beep to write and send it, since the user approves it before it goes out. \
Skip chores the user does without help, like checking out of a hotel or packing, \
unless the context gives a real problem to solve. \
Leave out starters about events that are already over. \
Never suggest something listed under \"Done recently\": it is done, even if a memory still mentions it as a plan. \
Recent conversations are things the user already asked; do not suggest asking them again. \
Starters under \"Shown just before\" were not wanted: suggest different things, not rewordings of them. \
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
    /// Where the phone last was, like "Old Town, Lisbon".
    pub place: Option<String>,
    pub connected_apps: Vec<String>,
    /// What Beep recently did in the user's apps, newest first.
    pub done: Vec<Done>,
    /// The set the user just asked to replace with New ideas.
    pub shown: Vec<PromptSuggestion>,
}

/// Something Beep did in the user's apps.
pub struct Done {
    pub at: DateTime<Utc>,
    pub description: String,
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
        if !self.done.is_empty() {
            let _ = writeln!(text, "\nDone recently:");
            for done in self.done.iter().take(MAX_DONE) {
                let at = LocalTime {
                    time: done.at,
                    timezone: self.now.timezone,
                };
                let description = clip(&done.description, MAX_DONE_CHARS);
                let _ = writeln!(text, "- {}: {description}", at.describe());
            }
        }
        if !self.shown.is_empty() {
            let _ = writeln!(text, "\nShown just before:");
            for starter in &self.shown {
                let prompt = clip(&starter.prompt, MAX_PROMPT_CHARS);
                let _ = writeln!(
                    text,
                    "- {}: {prompt}",
                    clip(&starter.title, MAX_TITLE_CHARS)
                );
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

/// One starter line of the model's answer.
struct Line<'a> {
    suggestion: PromptSuggestion,
    until: Option<&'a str>,
}

/// One `title | prompt | until` line, or `None` when it does not fit a chip.
fn starter(item: &str) -> Option<Line<'_>> {
    let mut parts = item.split('|').map(str::trim);
    let title = parts.next()?.trim_matches(['"', '*']).trim();
    let prompt = parts.next()?.trim_matches('"').trim();
    let until = parts
        .next()
        .map(|until| until.strip_prefix("until").unwrap_or(until).trim());
    let fits = |text: &str, max| !text.is_empty() && text.chars().count() <= max;
    (fits(title, MAX_TITLE_CHARS) && fits(prompt, MAX_PROMPT_CHARS)).then(|| {
        let suggestion = PromptSuggestion {
            title: title.to_owned(),
            prompt: prompt.to_owned(),
        };
        Line { suggestion, until }
    })
}

/// Parses the model's answer: a Markdown list of starters. Lines that do not
/// fit are skipped, and too few left is an error. An `until` that does not
/// parse is ignored, and one in the past counts as soon.
fn parse(answer: &str, now: &LocalTime) -> Result<Starters> {
    let parsed: Vec<_> = list_items(answer)
        .into_iter()
        .filter_map(starter)
        .take(COUNT)
        .collect();
    ensure!(
        parsed.len() >= MIN_COUNT,
        "fewer than {MIN_COUNT} usable starters"
    );
    let soonest = now.time + MIN_STALE;
    let stale_at = parsed
        .iter()
        .filter_map(|line| now.read(line.until?))
        .map(|at| at.max(soonest))
        .min();
    Ok(Starters {
        suggestions: parsed.into_iter().map(|line| line.suggestion).collect(),
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
        self.text_up_to(system, prompt, 4000).await
    }

    /// [`Self::text`] with room for an answer of up to `max_tokens`.
    pub async fn text_up_to(
        &self,
        system: &str,
        prompt: String,
        max_tokens: u32,
    ) -> Result<String> {
        let request = CompletionRequest {
            model: self.llm.model.clone(),
            messages: vec![Message::system(system), Message::user(prompt)],
            tools: Vec::new(),
            // Off leaves the provider default, and reasoning models then spend
            // a small budget thinking before writing any text.
            max_tokens: Some(max_tokens),
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

/// The items of a Markdown list answer: its lines with a leading `-`, `*`,
/// `•`, `1.` or `1)` removed. When some lines have one, the others, like an
/// intro sentence, are left out.
pub(crate) fn list_items(answer: &str) -> Vec<&str> {
    let lines: Vec<_> = unfence(answer)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| (strip_marker(line), line))
        .collect();
    let marked = lines.iter().any(|(item, line)| item.len() < line.len());
    lines
        .into_iter()
        .filter(|(item, line)| !marked || item.len() < line.len())
        .map(|(item, _)| item)
        .filter(|item| !item.is_empty())
        .collect()
}

/// `line` without a leading list marker.
fn strip_marker(line: &str) -> &str {
    if let Some(rest) = line.strip_prefix(['-', '*', '•']) {
        return rest.trim_start();
    }
    let digits = line.len() - line.trim_start_matches(|c: char| c.is_ascii_digit()).len();
    match line[digits..].strip_prefix(['.', ')']) {
        Some(rest) if digits > 0 => rest.trim_start(),
        _ => line,
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
    fn parses_the_list() {
        let good = "Here you go:\n\
- Plan the weekend | Help me plan a relaxed weekend.\n\
- **Gym check-in** | Log today's workout.\n\n\
* Dinner idea | Suggest a quick dinner.\n\
1. Inbox | Summarize my unread email.\n\
- Extra | One more than needed.";
        let now = singapore("2026-10-04T22:00:00+08:00");
        let parsed = parse(good, &now).unwrap();
        let titles: Vec<_> = parsed
            .suggestions
            .iter()
            .map(|s| s.title.as_str())
            .collect();
        assert_eq!(
            titles,
            ["Plan the weekend", "Gym check-in", "Dinner idea", "Inbox"]
        );
        assert_eq!(parsed.stale_at, None);

        let long = format!("- {} | p\n- a | b\n- No prompt", "x".repeat(29));
        assert!(parse(&long, &now).is_err());
        assert!(parse("Sure! Here are some ideas", &now).is_err());
    }

    fn singapore(time: &str) -> LocalTime {
        LocalTime {
            time: time.parse().expect("valid time"),
            timezone: Some(Tz::Asia__Singapore),
        }
    }

    #[test]
    fn list_items_skip_an_intro() {
        assert_eq!(
            list_items("Sure:\n\n- Why?\n1) How?\n- 3 ideas"),
            ["Why?", "How?", "3 ideas"]
        );
        assert_eq!(list_items("Why?\nHow?"), ["Why?", "How?"]);
        assert_eq!(list_items("```\n- Why?\n```"), ["Why?"]);
    }

    #[test]
    fn starters_go_stale_at_the_first_until() {
        let now = singapore("2026-10-04T22:00:00+08:00");
        let answer = "- a | b | until 2026-10-05T14:00\n\
- a | b | 2026-10-05T09:30\n\
- a | b\n\
- a | b | until soon";
        let stale_at = parse(answer, &now).unwrap().stale_at;
        assert_eq!(stale_at, Some("2026-10-05T01:30:00Z".parse().unwrap()));

        // A time already past waits a little instead of regenerating at once.
        let past = answer.replace("2026-10-05T09:30", "2026-10-04T08:00");
        let stale_at = parse(&past, &now).unwrap().stale_at;
        assert_eq!(stale_at, Some(now.time + MIN_STALE));
    }

    #[test]
    fn a_refresh_lists_the_set_it_replaces() {
        let context = StarterContext {
            memories: Vec::new(),
            recent_titles: Vec::new(),
            now: singapore("2026-10-07T02:00:00Z"),
            place: None,
            connected_apps: Vec::new(),
            done: Vec::new(),
            shown: vec![PromptSuggestion {
                title: "Lunch nearby".into(),
                prompt: "Find a quiet lunch spot near me.".into(),
            }],
        };
        let text = context.render();
        assert!(
            text.contains("Shown just before:\n- Lunch nearby: Find a quiet lunch spot near me.")
        );
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
