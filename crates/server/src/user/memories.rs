//! One user's memories.

use super::User;
use crate::{
    memories::{
        CURATE_EVERY, CURATE_SYSTEM, CURATE_TIMEOUT, Change, DUPLICATE, Exchange, Hit, LEARN_RULES,
        LEARN_SYSTEM, LEARN_TIMEOUT, LearnKind, LearnOp, Learned, MAX_CURATED,
        READING_LEARN_TIMEOUT, RECALLABLE, Reading, Remembered, Step, clip, core_ids,
        curate_prompt, hit, learn_prompt, parse_curation, parse_learn_ops, plan_curation,
        reading_prompt, settle,
    },
    protocol::MemorySource,
    starters::Generator,
    store::{Fact, MemoryRow, Origin},
};
use anyhow::Result;
use std::{collections::HashMap, time::Instant};

impl User {
    /// Memories that still hold, ranked against `query`. An empty query lists
    /// the most recently changed.
    pub async fn search_memories(
        &self,
        query: &str,
        category: Option<&str>,
        limit: usize,
    ) -> Result<Vec<Hit>> {
        let query = query.trim();
        let today = self.today().await;
        let holds = |row: &MemoryRow| !row.valid.ended(today);
        if query.is_empty() {
            return Ok(self
                .memory_rows()
                .await?
                .into_iter()
                .filter(holds)
                .filter(|row| category.is_none_or(|c| row.category == c))
                .take(limit)
                .map(hit)
                .collect());
        }
        // Ended memories still in the store are dropped after the search.
        let found = self
            .store
            .search_memories(&self.key, query, category, limit * 2)
            .await?;
        Ok(found
            .into_iter()
            .filter(|found| found.score >= RECALLABLE && holds(&found.row))
            .take(limit)
            .map(|found| hit(found.row))
            .collect())
    }

    /// Stores a fact. When it says the same as a memory, that one is replaced.
    pub async fn remember(&self, fact: Fact<'_>) -> Result<Remembered> {
        let closest = self
            .store
            .closest_memory(&self.key, fact.content, DUPLICATE)
            .await?;
        if let Some(row) = closest {
            // The new text replaces the old one whole, days included.
            let replacement = Fact {
                valid: Some(fact.valid.unwrap_or_default()),
                ..fact.clone()
            };
            if self.update_memory(&row.id, replacement).await? {
                return Ok(Remembered {
                    id: row.id,
                    replaced: true,
                });
            }
            // Deleted meanwhile: add it instead.
        }
        let id = self.add_memory(fact).await?;
        Ok(Remembered {
            id,
            replaced: false,
        })
    }

    /// The memories closest to the latest exchange. Jev and the learner both
    /// see them, so a fact already known neither scores high nor is stored again.
    pub async fn related_memories(&self, exchange: &Exchange) -> Result<Vec<Hit>> {
        let query = format!(
            "{}\n{}",
            exchange.latest.message,
            clip(&exchange.latest.reply, 300)
        );
        self.search_memories(&query, None, 15).await
    }

    /// Reads a finished exchange and updates memory with what it teaches about
    /// the user, returning the changes made. Failures are logged and dropped.
    /// `known` comes from `related_memories`.
    pub async fn learn(
        &self,
        generator: Generator<'_>,
        exchange: Exchange,
        known: &[Hit],
    ) -> Vec<Learned> {
        let attempt = async {
            if exchange.latest.message.split_whitespace().count() < 3 {
                return Ok(Vec::new());
            }
            let prompt = learn_prompt(known, &exchange);
            let answer = generator.text(LEARN_SYSTEM, prompt).await?;
            let ops = parse_learn_ops(&answer)?;
            anyhow::Ok(
                self.apply_all(known, ops, MemorySource::Learned {}, &exchange.session_id)
                    .await,
            )
        };
        settle(
            self.id(),
            tokio::time::timeout(LEARN_TIMEOUT, attempt).await,
        )
    }

    /// Reads an email or a Slack conversation and updates memory with what
    /// it teaches about the user, the same way `learn` does for a chat.
    pub async fn learn_reading(&self, generator: Generator<'_>, reading: &Reading) -> Vec<Learned> {
        let started = Instant::now();
        let attempt = async {
            let mut known = self.search_memories(&reading.query, None, 15).await?;
            // Anything can mention the user's name or family, which a search
            // on its text would not bring up, so those facts always come along.
            for category in ["identity", "people"] {
                for hit in self.search_memories("", Some(category), 15).await? {
                    if !known.iter().any(|k| k.id == hit.id) {
                        known.push(hit);
                    }
                }
            }
            let system = format!("{}\n{LEARN_RULES}", reading.kind.intro());
            let answer = generator
                .text(&system, reading_prompt(&known, reading))
                .await?;
            let ops = parse_learn_ops(&answer)?;
            anyhow::Ok(
                self.apply_all(&known, ops, reading.kind.source(), &reading.id)
                    .await,
            )
        };
        let learned = settle(
            self.id(),
            tokio::time::timeout(READING_LEARN_TIMEOUT, attempt).await,
        );
        log::debug!(
            "memory {} learning took {:.1}s; id={}",
            reading.kind.source().as_str(),
            started.elapsed().as_secs_f32(),
            reading.id
        );
        learned
    }

    /// Applies the learner's `ops` from the chat, email or thread named by
    /// `reference`, filing their facts in one Jev request. One op that fails
    /// leaves the others, and those already saved are still reported.
    async fn apply_all(
        &self,
        known: &[Hit],
        ops: Vec<LearnOp>,
        source: MemorySource,
        reference: &str,
    ) -> Vec<Learned> {
        let facts: Vec<&str> = ops.iter().filter_map(LearnOp::fact).collect();
        let mut filed = self.file_each(&facts).await.into_iter();
        let mut learned = Vec::new();
        for op in ops {
            let category = if op.fact().is_some() {
                filed.next().flatten()
            } else {
                None
            };
            let origin = op.origin(reference);
            match self
                .apply_learned(known, op, source, origin, category)
                .await
            {
                Ok(change) => learned.extend(change),
                Err(error) => log::warn!("memories: unable to learn: {error:#}; user={self}"),
            }
        }
        learned
    }

    /// `category` files the fact; `None` leaves it to Jev.
    async fn apply_learned(
        &self,
        known: &[Hit],
        op: LearnOp,
        source: MemorySource,
        origin: Origin,
        category: Option<&str>,
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
            let forgotten = self.forget(&hit.id).await?;
            return Ok(forgotten.then(|| Learned {
                change: Change::Forgotten,
                id: hit.id.clone(),
                content: hit.content.clone(),
            }));
        }
        let Some(content) = op.fact() else {
            return Ok(None);
        };
        let fact = Fact {
            content,
            category,
            source,
            valid: op.validity(),
            origin: Some(origin),
        };
        // An update of a memory it was not shown is a new fact.
        if let Some(hit) = shown
            && self.update_memory(&hit.id, fact.clone()).await?
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
        let remembered = self.remember(fact).await?;
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
    pub async fn curate_memories(&self, generator: Generator<'_>) -> Result<usize> {
        let rows: Vec<MemoryRow> = self
            .memory_rows()
            .await?
            .into_iter()
            .take(MAX_CURATED)
            .collect();
        if rows.len() < 2 {
            return Ok(0);
        }
        let today = self.today().await;
        let attempt = async {
            let answer = generator
                .text(CURATE_SYSTEM, curate_prompt(&rows, today))
                .await?;
            parse_curation(&answer)
        };
        let curation = tokio::time::timeout(CURATE_TIMEOUT, attempt)
            .await
            .map_err(|_| anyhow::anyhow!("curation timed out"))??;
        let by_id: HashMap<&str, &MemoryRow> = rows.iter().map(|r| (r.id.as_str(), r)).collect();
        let plan = plan_curation(curation.ops, &by_id, today);
        let core = curation.core.map(|core| core_ids(core, &plan, &by_id));
        let mut changed = 0;
        for step in plan {
            match step {
                Step::Forget(id) => {
                    changed += usize::from(self.forget(&id).await?);
                }
                Step::Rewrite { keep, fact, drop } => {
                    let fact = Fact {
                        content: &fact.content,
                        category: None,
                        source: fact.source,
                        valid: fact.valid,
                        origin: None,
                    };
                    if !self.update_memory(&keep, fact).await? {
                        continue;
                    }
                    changed += 1;
                    for id in drop {
                        changed += usize::from(self.forget(&id).await?);
                    }
                }
            }
        }
        // Only the memories curation read move in or out of the profile.
        if let Some(core) = core {
            for row in rows.iter().filter(|row| row.core != core.contains(&row.id)) {
                changed += usize::from(self.store.set_core(&self.key, &row.id, !row.core).await?);
            }
        }
        Ok(changed)
    }

    /// Curates the memories if they changed since the last pass, at most
    /// once a day.
    pub(crate) async fn curate_if_due(&self, generator: Generator<'_>) {
        match self.curation_due(CURATE_EVERY).await {
            Ok(true) => {}
            Ok(false) => return,
            Err(error) => {
                log::warn!("unable to check memory curation: {error}; user={self}");
                return;
            }
        }
        match self.curate_memories(generator).await {
            Ok(0) => {}
            Ok(changed) => log::info!("memory curation changed {changed}; user={self}"),
            Err(error) => log::info!("memory curation skipped: {error}; user={self}"),
        }
        // Marked even after a failure, so a bad answer is retried tomorrow
        // rather than every hour.
        if let Err(error) = self.mark_curated().await {
            log::warn!("unable to mark memories curated: {error}; user={self}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::TestDb;

    fn fact(content: &str) -> Fact<'_> {
        Fact {
            content,
            category: None,
            source: MemorySource::User {},
            valid: None,
            origin: None,
        }
    }

    #[tokio::test]
    async fn memories_dedupe_within_a_user_and_stay_apart_across_users() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let alice = User::for_test(store.clone(), "alice");
        let bob = User::for_test(store, "bob");

        let first = alice.remember(fact("I live in Lisbon")).await.unwrap();
        assert!(!first.replaced);
        let again = alice.remember(fact("I live in Lisbon")).await.unwrap();
        assert!(again.replaced);
        assert_eq!(again.id, first.id);

        // The same words from another user are a new memory, not a replacement.
        let theirs = bob.remember(fact("I live in Lisbon")).await.unwrap();
        assert!(!theirs.replaced);
        assert_ne!(theirs.id, first.id);

        let found = alice.search_memories("Lisbon", None, 10).await.unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].id, first.id);
        assert!(!bob.forget(&first.id).await.unwrap());
        assert_eq!(alice.list_memories().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn a_memory_keeps_where_it_came_from() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let alice = User::for_test(store, "alice");
        let origin = Origin {
            reference: "email-1".into(),
            quote: Some("see you in Lisbon".into()),
        };
        let stored = alice
            .remember(Fact {
                origin: Some(origin.clone()),
                ..fact("Is going to Lisbon.")
            })
            .await
            .unwrap();
        // An update that names no origin keeps the one it had.
        assert!(
            alice
                .update_memory(&stored.id, fact("Is going to Lisbon with Ana."))
                .await
                .unwrap()
        );
        let rows = alice.memory_rows().await.unwrap();
        assert_eq!(rows[0].origin, Some(origin));
        let listed = alice.list_memories().await.unwrap();
        assert_eq!(listed[0].quote.as_deref(), Some("see you in Lisbon"));
    }
}
