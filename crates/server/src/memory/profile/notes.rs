//! Turning raw mail and events into notes about the user.

use crate::{
    memory::memories::clip,
    memory::starters::{Generator, unfence},
    store::ProfileNote,
};
use anyhow::{Context, Result, bail};
use chrono::NaiveDate;
use futures::{StreamExt, stream};
use serde::Deserialize;
use std::mem;

/// Text per extraction call.
pub(super) const BATCH_CHARS: usize = 16_000;

/// Extraction calls at once.
pub(super) const CONCURRENCY: usize = 8;

pub(super) const NOTE_RULES: &str = "Note anything that tells who the user is and how they live: their name, \
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

pub(super) const DOSSIER_SYSTEM: &str = "You help a personal assistant app get to know its user. You get notes \
taken from their mail and calendar over the years, plus counts, oldest first. Merge them into a dossier on \
the user: identity and languages, home over time, work over time, people and how they relate (and who \
appears in both mail and calendar), shopping (shops, brands, sizes, colors), travel, subscriptions, hobbies, \
fitness, food, routines and tastes, homes and property, health and insurance, beliefs, religion and politics, \
money (banks, investments, debts), residency and visas. Keep counts, names, places and dates, and a timeline \
wherever something changed. Drop repeats. Plain text, at most 1500 words.";

#[derive(Deserialize)]
pub(super) struct Notes {
    #[serde(default)]
    pub(super) notes: Vec<RawNote>,
}

#[derive(Deserialize)]
pub(super) struct RawNote {
    #[serde(default)]
    pub(super) date: Option<String>,
    pub(super) note: String,
}

/// Packs `texts` into batches and asks for notes on each, several at once.
pub(super) async fn extract(
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
pub(super) fn pack(texts: &[String], chars: usize) -> Vec<String> {
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

pub(super) fn parse_notes(answer: &str) -> Result<Vec<ProfileNote>> {
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
pub(super) fn complete_notes(answer: &str) -> Option<Vec<RawNote>> {
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

pub(super) fn parse_day(text: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(text.get(..10)?, "%Y-%m-%d").ok()
}
