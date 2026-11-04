//! Condensing a source's notes into facts to remember.

use super::{CONCURRENCY, DOSSIER_SYSTEM, Source, pack, parse_day, read_reference};
use crate::{
    account::user::User,
    memory::memories::clip,
    memory::starters::{Generator, unfence},
    protocol::MemorySource,
    store::{Fact, Origin, Scope, Validity},
};
use anyhow::{Context, Result};
use chrono::Utc;
use futures::{StreamExt, stream};
use serde::Deserialize;
use std::fmt::Write;

/// Notes per condensing call, and the facts it may end with.
pub(super) const CONDENSE_CHARS: usize = 60_000;

pub(super) const MAX_FACTS: usize = 80;

/// Answer room for a dossier and for the final facts.
pub(super) const DOSSIER_TOKENS: u32 = 6000;

pub(super) const FACTS_TOKENS: u32 = 12_000;

pub(super) const FACTS_SYSTEM: &str = "You keep the long-term memory of a personal assistant app. You get dossiers \
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

#[derive(Deserialize)]
pub(super) struct Facts {
    #[serde(default)]
    pub(super) facts: Vec<RawFact>,
}

#[derive(Deserialize)]
pub(super) struct RawFact {
    pub(super) content: String,
    #[serde(default)]
    pub(super) valid_from: Option<String>,
    #[serde(default)]
    pub(super) valid_until: Option<String>,
    #[serde(default)]
    pub(super) evidence: Option<String>,
}

/// Condenses every source's notes into memories tagged with `source`; returns how many were saved.
pub(super) async fn condense(
    generator: Generator<'_>,
    user: &User,
    source: Source,
) -> Result<usize> {
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
