//! Reads the model's learn and curate answers and plans the changes to the stored memories.

use super::{MAX_CORE, MAX_CURATE_OPS, MAX_LEARN_OPS, MAX_PROFILE_CHARS, PROFILE_HEADER, days};
use crate::{
    memory::starters::unfence,
    protocol::MemorySource,
    store::{MemoryRow, Origin, Scope, Validity},
    tools::group_memory::shareable,
};
use anyhow::{Result, ensure};
use chrono::NaiveDate;
use serde::Deserialize;
use std::{
    collections::{HashMap, HashSet},
    slice,
};

pub(super) fn date(text: Option<&str>) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(text?.trim(), "%Y-%m-%d").ok()
}

/// Days a model gave, or `None` when it gave neither.
pub(super) fn validity(from: Option<&str>, until: Option<&str>) -> Option<Validity> {
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
    /// What the fact is about, such as `project:beep`; only the memory watcher sets it.
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

    /// Where the fact came from, `reference` naming the chat, email or thread.
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
pub(super) struct Ops<T> {
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

/// The profile's ids after `steps`: merged ids follow their target, forgotten or unknown ones are dropped.
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
pub(super) fn current(valid: &Validity, today: NaiveDate) -> bool {
    valid.until.is_some_and(|until| until >= today) && valid.from.is_none_or(|from| from <= today)
}

/// The profile's facts: what holds now, then the user's core memories, each with its days.
pub(crate) fn profile_facts(rows: &[MemoryRow], today: NaiveDate) -> Vec<String> {
    let about_user = || rows.iter().filter(|row| row.scope == Scope::User);
    let now = about_user().filter(|row| current(&row.valid, today));
    let core = about_user()
        .filter(|row| row.core && !row.valid.ended(today) && !current(&row.valid, today));
    now.chain(core)
        .map(|row| format!("{}{}", row.content, days(&row.valid)))
        .collect()
}

/// The newest memory saying which languages the user speaks.
pub(crate) fn languages(rows: &[MemoryRow], today: NaiveDate) -> Option<&str> {
    rows.iter()
        .filter(|row| row.scope == Scope::User && !row.valid.ended(today))
        .map(|row| row.content.as_str())
        .find(|content| {
            let content = content.to_lowercase();
            content.starts_with("speaks ") || content.contains(" speaks ")
        })
}

/// `profile_facts` cut to `MAX_PROFILE_CHARS`, or `None` when empty.
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

/// The days a merge of `rows` gets, never cutting a fact still holding short.
/// `None` refuses merging a lasting fact with a dated one that still holds.
pub(super) fn merged_days(
    rows: &[&MemoryRow],
    asked: Option<Validity>,
    today: NaiveDate,
) -> Option<Validity> {
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

/// Checks the model's curation ops: unknown and reused ids dropped, stated memories forgotten only once over.
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

/// Points each supersede at what its `by` ends up as, or drops it when `by` is gone.
pub(super) fn follow_supersedes(steps: Vec<Step>) -> Vec<Step> {
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
