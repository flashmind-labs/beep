//! The improvement watcher: finds where Beep failed the user and proposes fixes, applying nothing.

use super::{
    WatcherKind, gate,
    runner::{Review, Watcher},
};
use crate::{
    memories::clip,
    starters::unfence,
    store::{FindingRow, NewFinding, Sighting, StoredMessage},
};
use anyhow::Result;
use async_trait::async_trait;
use chrono::Utc;
use serde::Deserialize;
use std::{collections::HashSet, fmt::Write};
use strum::{EnumString, IntoStaticStr};

/// The most findings one review keeps.
const MAX_FINDINGS: usize = 3;
/// Open findings the prompt lists, so the same problem gets the same name.
const KNOWN_FINDINGS: i64 = 40;
const SUMMARY_MAX: usize = 300;
const STEPS_MAX: usize = 1_000;
const DETAIL_MAX: usize = 500;
const FIX_MAX: usize = 1_000;
const NAME_MAX: usize = 80;

/// What went wrong.
#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumString, IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum FindingKind {
    /// The user corrected Beep, as in "no, I meant".
    Correction,
    /// A tool call failed or had to be retried.
    FailedTool,
    /// Beep used the wrong tool for the job.
    WrongTool,
    /// Beep asked for something it could have found itself.
    CouldHaveFound,
    /// Beep went round in circles.
    Loop,
    /// A turn took much longer than it needed to.
    Slow,
    /// The user gave up or did it themselves.
    GaveUp,
    Other,
}

/// Where the problem likely comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumString, IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum Cause {
    Prompt,
    ToolSchema,
    MissingTool,
    Model,
    Other,
}

/// Where the user left a finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumString, IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum FindingStatus {
    Open,
    Resolved,
    Dismissed,
}

const SYSTEM: &str = "You review conversations between a user and Beep, a personal assistant app, to find where \
Beep failed the user, so its developers can fix Beep itself. Look for: the user correcting Beep (\"no, I meant\"), \
tool calls that failed or were retried, the wrong tool for the job, Beep asking the user for something it could \
have looked up, Beep going round in circles, turns that took far longer than needed, and the user giving up or \
doing it themselves. Only report real failures the conversation shows; most conversations have none, and then \
reply {\"findings\": []}. Never report the user's own mistakes or things outside Beep's control.
For each failure give:
- kind: correction, failed_tool, wrong_tool, could_have_found, loop, slow, gave_up or other.
- summary: one or two sentences on what went wrong, about Beep, not about the user's life.
- steps: how a developer could make it happen again, as short numbered steps with a made-up example request \
that has the same shape as the user's, not their real details.
- cause: prompt, tool_schema, missing_tool, model or other; cause_detail: one sentence on why you think so, \
naming the tool or the part of the prompt.
- fix: what to change in Beep, concretely.
- area: the tool name, such as GMAIL_SEND_EMAIL, or the part of Beep, such as system prompt or memory.
- slug: 3 to 6 lowercase words naming the problem, the same words every time it comes up. When it is one of the \
known findings listed, copy its kind, area and slug exactly.
- first_seq and last_seq: the numbers of the first and last messages that show it.
Never copy secrets, codes, passwords, tokens, card or account numbers, addresses, phone numbers or email \
addresses into a finding, and quote the user only as far as needed to show the problem.
Reply with only JSON: {\"findings\": [{\"kind\": string, \"summary\": string, \"steps\": string, \"cause\": string, \
\"cause_detail\": string, \"fix\": string, \"area\": string, \"slug\": string, \"first_seq\": number, \
\"last_seq\": number}]}. At most 3 findings, the most important first.";

/// A finding as the model proposed it.
#[derive(Debug, Deserialize)]
struct Proposed {
    kind: String,
    #[serde(default)]
    summary: String,
    #[serde(default)]
    steps: String,
    #[serde(default)]
    cause: String,
    #[serde(default)]
    cause_detail: String,
    #[serde(default)]
    fix: String,
    #[serde(default)]
    area: String,
    #[serde(default)]
    slug: String,
    #[serde(default)]
    first_seq: Option<i64>,
    #[serde(default)]
    last_seq: Option<i64>,
}

#[derive(Deserialize)]
struct Answer {
    #[serde(default)]
    findings: Vec<Proposed>,
}

/// A checked finding, ready to store.
#[derive(Debug, Clone, PartialEq)]
struct Checked {
    fingerprint: String,
    kind: FindingKind,
    summary: String,
    steps: String,
    cause: Cause,
    cause_detail: String,
    fix: String,
    first_seq: i64,
    last_seq: i64,
}

/// Lowercase words of `text` joined by dashes.
fn words(text: &str) -> String {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_lowercase)
        .collect::<Vec<_>>()
        .join("-")
}

/// What groups findings across chats: the kind, the area and the slug.
pub(crate) fn fingerprint(kind: FindingKind, area: &str, slug: &str) -> String {
    let kind: &str = kind.into();
    format!(
        "{kind}:{}:{}",
        clip(&words(area), NAME_MAX),
        clip(&words(slug), NAME_MAX)
    )
}

/// `text` with emails, long digit runs and long mixed tokens masked.
pub(crate) fn redact(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut word = String::new();
    for c in text.chars() {
        if c.is_whitespace() {
            out.push_str(&redact_word(&word));
            word.clear();
            out.push(c);
        } else {
            word.push(c);
        }
    }
    out.push_str(&redact_word(&word));
    out
}

fn redact_word(word: &str) -> String {
    let core = word.trim_matches(|c: char| !c.is_alphanumeric());
    if core.contains('@') && core.contains('.') {
        return word.replace(core, "[email]");
    }
    let letters = core.chars().filter(|c| c.is_alphabetic()).count();
    let digits = core.chars().filter(char::is_ascii_digit).count();
    let token_like = core.len() >= 20 && letters > 0 && digits > 0;
    let prefixed = ["sk-", "ghp_", "gho_", "xox", "AKIA", "eyJ"]
        .iter()
        .any(|prefix| core.starts_with(prefix) && core.len() >= 12);
    if token_like || prefixed {
        return word.replace(core, "[secret]");
    }
    // Numbers: a group of six or more digits, such as a code, or nine or
    // more split by dashes or dots, such as 4111-1111-1111-1111. Dates and
    // times stay.
    let mut out = String::new();
    let mut run = String::new();
    for c in word.chars() {
        if c.is_ascii_digit() || (!run.is_empty() && matches!(c, '-' | '.')) {
            run.push(c);
        } else {
            mask_number(&mut run, &mut out);
            out.push(c);
        }
    }
    mask_number(&mut run, &mut out);
    out
}

/// Moves `run` onto `out`, masked when it is a number worth hiding.
fn mask_number(run: &mut String, out: &mut String) {
    let number = run.trim_end_matches(|c: char| !c.is_ascii_digit());
    let tail = &run[number.len()..];
    let digits = number.chars().filter(char::is_ascii_digit).count();
    let longest = number
        .split(|c: char| !c.is_ascii_digit())
        .map(str::len)
        .max()
        .unwrap_or(0);
    if longest >= 6 || digits >= 9 {
        out.push_str("[number]");
    } else {
        out.push_str(number);
    }
    out.push_str(tail);
    run.clear();
}

/// Validates the model's findings: known kinds, trimmed and masked, one per fingerprint.
fn check(answer: &str, first_seq: i64, last_seq: i64) -> Result<Vec<Checked>> {
    let Answer { findings } = serde_json::from_str(unfence(answer))?;
    let mut seen = HashSet::new();
    let mut checked = Vec::new();
    for proposed in findings.into_iter().take(MAX_FINDINGS * 2) {
        let Ok(kind) = proposed.kind.trim().parse::<FindingKind>() else {
            continue;
        };
        let summary = redact(&clip(proposed.summary.trim(), SUMMARY_MAX));
        let slug = if proposed.slug.trim().is_empty() {
            &proposed.summary
        } else {
            &proposed.slug
        };
        if summary.is_empty() || words(slug).is_empty() {
            continue;
        }
        let fingerprint = fingerprint(kind, &proposed.area, slug);
        if !seen.insert(fingerprint.clone()) {
            continue;
        }
        let within = |seq: Option<i64>, or: i64| seq.unwrap_or(or).clamp(first_seq, last_seq);
        let (from, to) = (
            within(proposed.first_seq, first_seq),
            within(proposed.last_seq, last_seq),
        );
        checked.push(Checked {
            fingerprint,
            kind,
            summary,
            steps: redact(&clip(proposed.steps.trim(), STEPS_MAX)),
            cause: proposed.cause.trim().parse().unwrap_or(Cause::Other),
            cause_detail: redact(&clip(proposed.cause_detail.trim(), DETAIL_MAX)),
            fix: redact(&clip(proposed.fix.trim(), FIX_MAX)),
            first_seq: from.min(to),
            last_seq: from.max(to),
        });
        if checked.len() == MAX_FINDINGS {
            break;
        }
    }
    Ok(checked)
}

fn prompt(known: &[FindingRow], transcript: &str) -> String {
    let mut text = String::from("Known findings:\n");
    if known.is_empty() {
        text.push_str("(none)\n");
    }
    for finding in known {
        // The fingerprint is kind:area:slug.
        let mut parts = finding.fingerprint.splitn(3, ':');
        let (kind, area, slug) = (
            parts.next().unwrap_or_default(),
            parts.next().unwrap_or_default(),
            parts.next().unwrap_or_default(),
        );
        let _ = writeln!(
            text,
            "- kind {kind}, area {area}, slug {slug}: {}",
            clip(&finding.summary, 200)
        );
    }
    let _ = write!(text, "\nConversation:\n{transcript}\n");
    text
}

pub(crate) struct ImprovementWatcher;

#[async_trait]
impl Watcher for ImprovementWatcher {
    fn kind(&self) -> WatcherKind {
        WatcherKind::Improvement
    }

    /// Stretches with real work or a failed or denied call.
    fn worth_reading(&self, messages: &[StoredMessage]) -> bool {
        gate::busy(messages) || gate::friction(messages)
    }

    async fn review(&self, review: &Review<'_>) -> Result<()> {
        let transcript = review.transcript;
        if !transcript.talk {
            return Ok(());
        }
        let store = &review.app.store;
        let user = review.user;
        let known = store
            .findings(user.id(), Some(FindingStatus::Open), KNOWN_FINDINGS)
            .await?;
        let answer = review
            .generator
            .text(SYSTEM, prompt(&known, &transcript.text))
            .await?;
        let findings = check(&answer, transcript.first_seq, transcript.last_seq)?;
        let now = Utc::now();
        for finding in &findings {
            let new = NewFinding {
                fingerprint: &finding.fingerprint,
                kind: finding.kind,
                summary: &finding.summary,
                steps: &finding.steps,
                cause: finding.cause,
                cause_detail: &finding.cause_detail,
                fix: &finding.fix,
            };
            let seen = Sighting {
                user: user.id(),
                chat: review.chat,
                first_seq: finding.first_seq,
                last_seq: finding.last_seq,
            };
            store.record_finding(&new, &seen, now).await?;
        }
        if !findings.is_empty() {
            let kinds: Vec<&str> = findings.iter().map(|f| f.kind.into()).collect();
            log::info!(
                "improvement watcher found {kinds:?}; user={user} chat={}",
                review.chat
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        store::test_chat,
        watchers::{
            runner::{Runner, Settings, TimingSettings, WatcherSettings},
            testing::{Harness, Scripted},
        },
    };
    use chrono::TimeDelta;
    use flashmind::types::AgentLlmConfig;
    use std::{sync::Arc, time::Duration};

    #[test]
    fn one_problem_named_differently_has_one_fingerprint() {
        assert_eq!(
            fingerprint(
                FindingKind::WrongTool,
                "GMAIL_SEARCH",
                "Calendar event, searched in mail!"
            ),
            fingerprint(
                FindingKind::WrongTool,
                "gmail search",
                "calendar  event searched in MAIL"
            ),
        );
        assert_eq!(
            fingerprint(FindingKind::Loop, "web_search", "same query again"),
            "loop:web-search:same-query-again"
        );
        assert_ne!(
            fingerprint(FindingKind::Loop, "web_search", "same query again"),
            fingerprint(FindingKind::Slow, "web_search", "same query again"),
        );
    }

    #[test]
    fn masks_what_looks_like_a_secret() {
        assert_eq!(
            redact("mail ana@example.com, card 4111-1111-1111-1111 and code 482913."),
            "mail [email], card [number] and code [number]."
        );
        assert_eq!(
            redact("key sk-live-abcdef123456 token a1b2c3d4e5f6g7h8i9j0k1\nbye"),
            "key [secret] token [secret]\nbye"
        );
        assert_eq!(
            redact("Meeting on 2026-10-30 at 19:00 in room 12"),
            "Meeting on 2026-10-30 at 19:00 in room 12"
        );
        assert_eq!(redact("call +34600123456"), "call +[number]");
    }

    #[test]
    fn keeps_known_kinds_within_what_was_read() {
        let answer = r#"```json
        {"findings": [
          {"kind": "correction", "summary": "Booked the wrong day; user said 123456789 is their code.",
           "steps": "1. Ask to book padel tomorrow", "cause": "prompt", "cause_detail": "dates",
           "fix": "Resolve relative dates first.", "area": "system prompt", "slug": "Relative dates",
           "first_seq": 1, "last_seq": 99},
          {"kind": "correction", "summary": "Same again", "area": "System Prompt", "slug": "relative-dates"},
          {"kind": "rant", "summary": "Not a kind", "slug": "x"},
          {"kind": "loop", "summary": "", "slug": "empty summary"},
          {"kind": "slow", "summary": "Took long", "cause": "weather", "slug": "slow search"}
        ]}
        ```"#;
        let checked = check(answer, 10, 20).unwrap();
        assert_eq!(checked.len(), 2);
        assert_eq!(
            checked[0].fingerprint,
            "correction:system-prompt:relative-dates"
        );
        assert_eq!((checked[0].first_seq, checked[0].last_seq), (10, 20));
        assert!(checked[0].summary.contains("[number]"));
        assert_eq!(checked[1].kind, FindingKind::Slow);
        assert_eq!(checked[1].cause, Cause::Other);
        assert!(check("no json", 1, 2).is_err());
    }

    #[tokio::test]
    async fn a_problem_seen_in_two_chats_counts_twice() {
        let model = Scripted::default().answer(
            "You review conversations",
            r#"{"findings":[{"kind":"could_have_found","summary":"Asked for the user's address instead of reading it from memory.","steps":"1. Ask for a taxi home","cause":"prompt","cause_detail":"recall is not suggested","fix":"Tell Beep to recall places before asking.","area":"memory","slug":"asked for known address","first_seq":0,"last_seq":0}]}"#,
        );
        let h = Harness::new(model).await;
        let (first, second) = (test_chat("taxi-1"), test_chat("taxi-2"));
        h.turn_with_calls(&h.alice, first, "get me a taxi home", 3)
            .await;
        h.turn_with_calls(&h.alice, second, "taxi home please", 3)
            .await;
        let slot = WatcherSettings {
            kind: WatcherKind::Improvement,
            llm: AgentLlmConfig::new("openai:test".parse().unwrap()),
            timeout: Duration::from_secs(30),
        };
        let settings = Settings {
            timing: TimingSettings {
                quiet: Duration::from_secs(60),
                retry: Duration::from_secs(60),
            },
            sweep_every: Duration::from_secs(60),
            lookback: Duration::from_secs(3600),
            concurrency: 2,
            watchers: Vec::new(),
        };
        let mut runner = Runner::new(&h.app, settings, vec![(Arc::new(ImprovementWatcher), slot)]);
        runner.tick(Utc::now() + TimeDelta::minutes(2)).await;
        assert!(runner.drain().await.is_empty());

        let found = h.app.store.findings("alice", None, 10).await.unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].seen, 2);
        assert_eq!(found[0].kind, FindingKind::CouldHaveFound);
        assert_eq!(
            found[0].fingerprint,
            "could_have_found:memory:asked-for-known-address"
        );
        let seen = found[0].sighting.as_ref().expect("seen in alice's chats");
        assert!([first, second].contains(&seen.chat));
        assert!(seen.first_seq > 0, "kept within the messages read");
    }
}
