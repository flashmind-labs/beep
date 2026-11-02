//! One user's memories.

use super::User;
use crate::{
    memories::{
        CURATE_TIMEOUT, Change, CurateOp, DUPLICATE, Exchange, Hit, LEARN_RULES, LEARN_SYSTEM,
        LEARN_TIMEOUT, LearnKind, LearnOp, Learned, MAX_CURATED, MAX_SCOPES_PER_SWEEP, NEAR,
        READING_LEARN_TIMEOUT, RECALLABLE, Reading, Remembered, Signal, Step, clip, core_ids,
        crowded_scopes, curate_prompt, curate_system, curation_needs, hit, learn_prompt,
        parse_curation, parse_learn_ops, plan_curation, reading_prompt, settle,
    },
    protocol::MemorySource,
    starters::Generator,
    store::{Fact, MAX_MEMORY_ROWS, MemoryRow, Origin, Scope, Scored, Superseded},
};
use anyhow::{Result, ensure};
use chrono::Utc;
use std::{collections::HashMap, time::Instant};

impl User {
    /// Memories for automatic prompt context. The store's vector score is
    /// 1 - L2 distance / 2; embeddings are unit vectors.
    pub(crate) async fn prompt_memories(&self, query: &str) -> Result<Vec<Scored>> {
        const MIN_COSINE: f32 = 0.7;
        const MAX_HITS: usize = 8;
        const CANDIDATES: usize = 32;
        let query = query.trim();
        if query.is_empty() {
            return Ok(Vec::new());
        }
        let threshold = 1.0 - ((1.0 - MIN_COSINE) / 2.0).sqrt();
        let today = self.today().await;
        Ok(self
            .store
            .similar_memories(&self.key, query, threshold, CANDIDATES)
            .await?
            .into_iter()
            .filter(|found| {
                !found.row.valid.ended(today)
                    && found.row.valid.from.is_none_or(|from| from <= today)
            })
            .take(MAX_HITS)
            .collect())
    }

    /// Memories that still hold, ranked against `query`, in `scope` when
    /// given. An empty query lists the most recently changed.
    pub async fn search_memories(
        &self,
        query: &str,
        category: Option<&str>,
        scope: Option<&Scope>,
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
                .filter(|row| scope.is_none_or(|scope| row.scope == *scope))
                .take(limit)
                .map(hit)
                .collect());
        }
        // Ended memories still in the store are dropped after the search.
        let found = self
            .store
            .search_memories(&self.key, query, category, scope, limit * 2)
            .await?;
        for crowded in crowded_scopes(&found) {
            self.note_signal(&crowded, Signal::Crowded).await;
        }
        Ok(found
            .into_iter()
            .filter(|found| found.score >= RECALLABLE && holds(&found.row))
            .take(limit)
            .map(|found| hit(found.row))
            .collect())
    }

    /// Stores a fact. Identical text in the same scope is reused; similarity
    /// alone only signals that the scope may need curation.
    pub async fn remember(&self, fact: Fact<'_>) -> Result<Remembered> {
        let scope = fact.scope.clone().unwrap_or_default();
        let closest = self
            .store
            .closest_memory(&self.key, fact.content, &scope, NEAR)
            .await?;
        let close = closest.is_some();
        if let Some(found) = closest.filter(|found| {
            found.score >= DUPLICATE
                && found
                    .row
                    .content
                    .trim()
                    .eq_ignore_ascii_case(fact.content.trim())
                && found.row.valid == fact.valid.unwrap_or_default()
        }) {
            // The new text replaces the old one whole, days included.
            let replacement = Fact {
                valid: Some(fact.valid.unwrap_or_default()),
                ..fact.clone()
            };
            if self.replace_memory(&found.row.id, replacement).await? {
                return Ok(Remembered {
                    id: found.row.id,
                    replaced: true,
                });
            }
            // Deleted meanwhile: add it instead.
        }
        let id = self.add_memory(fact).await?;
        if close {
            self.note_signal(&scope, Signal::Close).await;
        }
        Ok(Remembered {
            id,
            replaced: false,
        })
    }

    /// `update_memory` that keeps what the memory said before as history.
    async fn replace_memory(&self, id: &str, fact: Fact<'_>) -> Result<bool> {
        let fact = Fact {
            category: self.category_of(&fact).await,
            ..fact
        };
        self.store.replace_memory(&self.key, id, fact).await
    }

    /// The earlier versions of the memory `id`, newest first.
    pub async fn memory_history(&self, id: &str) -> Result<Vec<Superseded>> {
        self.store.memory_history(&self.key, id).await
    }

    /// Counts a curation signal. A failure only delays curation, so it is
    /// logged rather than failing the store or recall that saw it.
    async fn note_signal(&self, scope: &Scope, signal: Signal) {
        if let Err(error) = self.store.note_signal(&self.key, scope, signal).await {
            log::warn!("unable to note a memory signal: {error}; user={self}");
        }
    }

    /// The memories closest to the latest exchange. Jev and the learner both
    /// see them, so a fact already known neither scores high nor is stored again.
    pub async fn related_memories(&self, exchange: &Exchange) -> Result<Vec<Hit>> {
        let query = format!(
            "{}\n{}",
            exchange.latest.message,
            clip(&exchange.latest.reply, 300)
        );
        self.search_memories(&query, None, None, 15).await
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
                self.apply_all(
                    known,
                    ops,
                    MemorySource::Learned {},
                    &exchange.session_id,
                    &|_| None,
                )
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
            let mut known = self
                .search_memories(&reading.query, None, Some(&Scope::User), 15)
                .await?;
            // Anything can mention the user's name or family, which a search
            // on its text would not bring up, so those facts always come along.
            for category in ["identity", "people"] {
                for hit in self
                    .search_memories("", Some(category), Some(&Scope::User), 15)
                    .await?
                {
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
                self.apply_all(&known, ops, reading.kind.source(), &reading.id, &|_| None)
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
    /// `reference`, filing their facts in one Jev request. `scope` says what
    /// each op's fact is about; `None` keeps an updated memory's scope and
    /// makes a new one about the user. One op that fails leaves the others,
    /// and those already saved are still reported.
    pub(crate) async fn apply_all(
        &self,
        known: &[Hit],
        ops: Vec<LearnOp>,
        source: MemorySource,
        reference: &str,
        scope: &(dyn Fn(&LearnOp) -> Option<Scope> + Sync),
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
            let scope = scope(&op);
            match self
                .apply_learned(known, op, source, origin, category, scope)
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
        scope: Option<Scope>,
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
            scope,
        };
        // An update of a memory it was not shown is a new fact.
        if let Some(hit) = shown
            && self.replace_memory(&hit.id, fact.clone()).await?
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

    /// Merges duplicates, settles contradictions and drops stale memories in
    /// one scope, returning how many memories changed. What a merge or
    /// rewrite replaces is kept as history of the memory that replaced it.
    /// Only the user's own scope picks the profile.
    pub async fn curate_scope(&self, generator: Generator<'_>, scope: &Scope) -> Result<usize> {
        let rows: Vec<MemoryRow> = self
            .memory_rows()
            .await?
            .into_iter()
            .filter(|row| row.scope == *scope)
            .take(MAX_CURATED)
            .collect();
        self.curate_rows(generator, scope, rows, false).await
    }

    /// Reviews every existing memory in bounded batches, applying only splits.
    pub(crate) async fn split_existing_memories(&self, generator: Generator<'_>) -> Result<usize> {
        let rows = self.memory_rows().await?;
        ensure!(
            rows.len() < MAX_MEMORY_ROWS,
            "too many memories for a complete split pass"
        );
        let mut scopes: HashMap<Scope, Vec<MemoryRow>> = HashMap::new();
        for row in rows {
            scopes.entry(row.scope.clone()).or_default().push(row);
        }
        let mut changed = 0;
        for (scope, rows) in scopes {
            for batch in rows.chunks(20) {
                changed += self
                    .curate_rows(generator, &scope, batch.to_vec(), true)
                    .await?;
            }
        }
        Ok(changed)
    }

    async fn curate_rows(
        &self,
        generator: Generator<'_>,
        scope: &Scope,
        rows: Vec<MemoryRow>,
        split_only: bool,
    ) -> Result<usize> {
        if rows.is_empty() {
            return Ok(0);
        }
        let today = self.today().await;
        let attempt = async {
            let mut system = curate_system(scope);
            if split_only {
                system.push_str(" This pass only splits memories. Return only split operations, or an empty ops list. Do not merge, update, supersede or forget anything.");
            }
            let answer = generator.text(&system, curate_prompt(&rows, today)).await?;
            parse_curation(&answer)
        };
        let curation = tokio::time::timeout(CURATE_TIMEOUT, attempt)
            .await
            .map_err(|_| anyhow::anyhow!("curation timed out"))??;
        let by_id: HashMap<&str, &MemoryRow> = rows.iter().map(|r| (r.id.as_str(), r)).collect();
        let ops = curation
            .ops
            .into_iter()
            .filter(|op| !split_only || matches!(op, CurateOp::Split { .. }))
            .collect();
        let plan = plan_curation(ops, &by_id, today);
        let core = curation
            .core
            .filter(|_| !split_only && *scope == Scope::User)
            .map(|core| core_ids(core, &plan, &by_id));
        let mut changed = 0;
        for step in plan {
            match step {
                Step::Split { id, contents } => {
                    let Some(row) = by_id.get(id.as_str()) else {
                        continue;
                    };
                    // A quote for the combined memory may contain facts the
                    // user later deletes separately.
                    let origin = row.origin.as_ref().map(|origin| Origin {
                        reference: origin.reference.clone(),
                        quote: None,
                    });
                    // Save the new facts first, so an interrupted split keeps the original.
                    for content in contents.iter().skip(1) {
                        self.store
                            .add_memory(
                                &self.key,
                                Fact {
                                    content,
                                    category: Some(&row.category),
                                    source: row.source,
                                    valid: Some(row.valid),
                                    origin: origin.clone(),
                                    scope: Some(row.scope.clone()),
                                },
                            )
                            .await?;
                        changed += 1;
                    }
                    let fact = Fact {
                        content: &contents[0],
                        category: Some(&row.category),
                        source: row.source,
                        valid: Some(row.valid),
                        origin,
                        scope: Some(row.scope.clone()),
                    };
                    // Keeping the combined text as history would make a later
                    // deletion of one split fact leave that fact in history.
                    changed += usize::from(self.store.update_memory(&self.key, &id, fact).await?);
                }
                Step::Forget(id) => {
                    changed += usize::from(self.forget(&id).await?);
                }
                Step::Supersede { id, by } => {
                    changed += usize::from(self.store.retire_memory(&self.key, &id, &by).await?);
                }
                Step::Rewrite { keep, fact, drop } => {
                    let fact = Fact {
                        content: &fact.content,
                        category: None,
                        source: fact.source,
                        valid: fact.valid,
                        origin: None,
                        scope: None,
                    };
                    if !self.replace_memory(&keep, fact).await? {
                        continue;
                    }
                    changed += 1;
                    for id in drop {
                        changed +=
                            usize::from(self.store.retire_memory(&self.key, &id, &keep).await?);
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

    /// The scopes whose memories need curating now, most pressing first.
    #[cfg(test)]
    pub(crate) async fn curation_needs(&self) -> Result<Vec<crate::memories::ScopeNeed>> {
        let rows = self.memory_rows().await?;
        let states = self.store.curation_states(&self.key).await?;
        Ok(curation_needs(&rows, &states, Utc::now().naive_utc()))
    }

    /// Curates the scopes that need it, a few per call.
    pub(crate) async fn curate_if_needed(&self, generator: Generator<'_>) {
        let read = tokio::try_join!(self.memory_rows(), self.store.curation_states(&self.key));
        let (rows, states) = match read {
            Ok(read) => read,
            Err(error) => {
                log::warn!("unable to check memory curation: {error}; user={self}");
                return;
            }
        };
        let needs = curation_needs(&rows, &states, Utc::now().naive_utc());
        let needs = &needs[..needs.len().min(MAX_SCOPES_PER_SWEEP)];
        for need in needs {
            let (kind, why) = (need.scope.kind(), need.need.as_str());
            match self.curate_scope(generator, &need.scope).await {
                Ok(changed) => {
                    log::info!(
                        "memory curation ({why}) changed {changed}; scope={kind} user={self}"
                    )
                }
                Err(error) => {
                    log::info!("memory curation ({why}) skipped: {error}; scope={kind} user={self}")
                }
            }
        }
        if needs.is_empty() {
            return;
        }
        // Marked even after a failure, so a bad answer waits for the next
        // need rather than coming back every hour.
        let rows = match self.memory_rows().await {
            Ok(rows) => rows,
            Err(error) => {
                log::warn!("unable to mark memories curated: {error}; user={self}");
                return;
            }
        };
        for need in needs {
            let size = rows.iter().filter(|row| row.scope == need.scope).count();
            let counted = states.get(&need.scope).cloned().unwrap_or_default();
            if let Err(error) = self
                .store
                .mark_curated(&self.key, &need.scope, size, &counted)
                .await
            {
                log::warn!("unable to mark memories curated: {error}; user={self}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        memories,
        store::{TestDb, Validity},
    };
    use async_trait::async_trait;
    use flashmind::types::{
        AgentLlmConfig, CompletionRequest, CompletionStream, FinishReason, LlmProvider, Provider,
        StreamEvent,
    };

    /// A model that always gives the same answer.
    struct Answer(String);

    #[async_trait]
    impl LlmProvider for Answer {
        fn name(&self) -> &str {
            "test"
        }
        fn provider(&self) -> Provider {
            Provider::OpenAi
        }
        fn complete(&self, _request: CompletionRequest) -> CompletionStream {
            let answer = self.0.clone();
            Box::pin(async_stream::stream! {
                yield Ok(StreamEvent::ContentDelta(answer));
                yield Ok(StreamEvent::Finished(FinishReason::Stop));
            })
        }
    }

    fn llm() -> AgentLlmConfig {
        AgentLlmConfig::new("openai:test".parse().unwrap())
    }

    fn fact(content: &str) -> Fact<'_> {
        Fact {
            content,
            category: None,
            source: MemorySource::User {},
            valid: None,
            origin: None,
            scope: None,
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

        let found = alice
            .search_memories("Lisbon", None, None, 10)
            .await
            .unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].id, first.id);
        assert!(!bob.forget(&first.id).await.unwrap());
        assert_eq!(alice.list_memories().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn the_same_words_in_two_scopes_are_two_memories() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let alice = User::for_test(store, "alice");
        let mac = Scope::Machine(iroh::SecretKey::generate().public());
        let server = Scope::Machine(iroh::SecretKey::generate().public());
        let on = |scope: &Scope| Fact {
            scope: Some(scope.clone()),
            ..fact("gh is signed in.")
        };
        let first = alice.remember(on(&mac)).await.unwrap();
        let other = alice.remember(on(&server)).await.unwrap();
        assert!(!other.replaced, "another machine's fact is its own");
        let again = alice.remember(on(&mac)).await.unwrap();
        assert!(again.replaced);
        assert_eq!(again.id, first.id);
        let about_user = alice.remember(fact("gh is signed in.")).await.unwrap();
        assert!(!about_user.replaced);

        let found = alice
            .search_memories("gh signed in", None, Some(&mac), 10)
            .await
            .unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].id, first.id);
        let found = alice
            .search_memories("gh signed in", None, Some(&Scope::User), 10)
            .await
            .unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].id, about_user.id);
        let all = alice
            .search_memories("gh signed in", None, None, 10)
            .await
            .unwrap();
        assert_eq!(all.len(), 3);

        let mut rows = alice.memory_rows().await.unwrap();
        // Only memories about the user make the profile, even when core.
        for row in &mut rows {
            row.core = true;
        }
        assert_eq!(
            memories::profile_facts(&rows, alice.today().await),
            ["gh is signed in."]
        );
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

    // Under the word embedder these two score above `DUPLICATE`.
    const AT_ACME: &str =
        "Works remotely for Acme building mobile apps with the design team from Lisbon every week.";
    const MOVED: &str =
        "Works remotely for Acme building mobile apps with the design team from Porto every week.";

    #[tokio::test]
    async fn similar_memories_do_not_replace_each_other() {
        let db = TestDb::new().await.unwrap();
        let alice = User::for_test(db.store().await.unwrap(), "alice");
        let first = alice.remember(fact(AT_ACME)).await.unwrap();
        let second = alice.remember(fact(MOVED)).await.unwrap();
        assert!(!second.replaced);
        assert_ne!(first.id, second.id);
        let rows = alice.memory_rows().await.unwrap();
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().any(|row| row.content == AT_ACME));
        assert!(rows.iter().any(|row| row.content == MOVED));
    }

    #[tokio::test]
    async fn automatic_recall_uses_cosine_threshold_and_current_owned_memories() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let alice = User::for_test(store.clone(), "alice");
        let bob = User::for_test(store, "bob");
        let today = alice.today().await;
        let high = alice
            .store
            .add_memory(&alice.key, fact("alpha bravo echo golf"))
            .await
            .unwrap();
        alice
            .store
            .add_memory(&alice.key, fact("alpha bravo golf charlie"))
            .await
            .unwrap();
        alice
            .store
            .add_memory(
                &alice.key,
                Fact {
                    valid: Some(Validity {
                        from: Some(today.succ_opt().unwrap()),
                        until: None,
                    }),
                    ..fact("alpha bravo echo foxtrot")
                },
            )
            .await
            .unwrap();
        alice
            .store
            .add_memory(
                &alice.key,
                Fact {
                    valid: Some(Validity {
                        from: None,
                        until: Some(today.pred_opt().unwrap()),
                    }),
                    ..fact("alpha bravo echo foxtrot")
                },
            )
            .await
            .unwrap();
        bob.store
            .add_memory(&bob.key, fact("alpha bravo echo foxtrot"))
            .await
            .unwrap();
        let found = alice
            .prompt_memories("alpha bravo echo foxtrot")
            .await
            .unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].row.id, high);
        assert!(alice.prompt_memories("  ").await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn curation_splits_a_memory_and_preserves_its_metadata() {
        let db = TestDb::new().await.unwrap();
        let alice = User::for_test(db.store().await.unwrap(), "alice");
        let scope = Scope::project("example").unwrap();
        let origin = Origin {
            reference: "session-1".into(),
            quote: Some("two facts".into()),
        };
        let id = alice
            .remember(Fact {
                content: "The project uses Rust and runs on Linux.",
                category: Some("work"),
                source: MemorySource::Learned {},
                valid: None,
                origin: Some(origin.clone()),
                scope: Some(scope.clone()),
            })
            .await
            .unwrap()
            .id;
        let answer = Answer(format!(
            r#"{{"ops":[{{"op":"forget","id":"{id}"}},{{"op":"split","id":"{id}","contents":["The project uses Rust.","The project runs on Linux."]}}]}}"#
        ));
        let llm = llm();
        let generator = Generator {
            provider: &answer,
            llm: &llm,
            user: "alice",
        };
        alice.split_existing_memories(generator).await.unwrap();
        let rows = alice.memory_rows().await.unwrap();
        assert_eq!(rows.len(), 2);
        for content in ["The project uses Rust.", "The project runs on Linux."] {
            let row = rows.iter().find(|row| row.content == content).unwrap();
            assert_eq!(
                row.origin.as_ref().map(|origin| origin.reference.as_str()),
                Some(origin.reference.as_str())
            );
            assert!(
                row.origin
                    .as_ref()
                    .is_some_and(|origin| origin.quote.is_none())
            );
            assert_eq!(row.scope, scope);
            assert_eq!(row.category, "work");
            assert_eq!(row.source, MemorySource::Learned {});
        }
        assert!(alice.memory_history(&id).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_replaced_memory_is_kept_as_hidden_history() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let alice = User::for_test(store, "alice");
        let first = alice.remember(fact(AT_ACME)).await.unwrap();
        let again = alice.remember(fact(AT_ACME)).await.unwrap();
        assert!(again.replaced);
        assert!(
            alice.memory_history(&first.id).await.unwrap().is_empty(),
            "the same words change nothing to keep"
        );
        assert!(alice.replace_memory(&first.id, fact(MOVED)).await.unwrap());

        let history = alice.memory_history(&first.id).await.unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].row.content, AT_ACME);
        assert!(!history[0].superseded_at.is_empty());

        // Recall, the Memories screen and the profile see only the current one.
        let found = alice
            .search_memories("Acme Lisbon design team", None, None, 10)
            .await
            .unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].content, MOVED);
        assert_eq!(alice.list_memories().await.unwrap().len(), 1);
        let mut rows = alice.memory_rows().await.unwrap();
        assert_eq!(rows.len(), 1);
        rows[0].core = true;
        assert_eq!(memories::profile_facts(&rows, alice.today().await), [MOVED]);
        // Another user cannot read it.
        let bob = User::for_test(alice.store.clone(), "bob");
        assert!(bob.memory_history(&first.id).await.unwrap().is_empty());

        // Forgetting a memory forgets its history.
        assert!(alice.forget(&first.id).await.unwrap());
        assert!(alice.memory_history(&first.id).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn an_edit_from_the_memories_screen_keeps_the_old_text() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let alice = User::for_test(store, "alice");
        let id = alice.remember(fact("Likes tea.")).await.unwrap().id;
        assert!(alice.edit_memory(&id, "Likes green tea.").await.unwrap());
        let history = alice.memory_history(&id).await.unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].row.content, "Likes tea.");
        assert!(!alice.edit_memory("missing", "Likes coffee.").await.unwrap());
    }

    #[tokio::test]
    async fn history_is_capped_per_memory() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let alice = User::for_test(store, "alice");
        let first = alice.remember(fact(AT_ACME)).await.unwrap();
        for round in 0..8 {
            let text = if round % 2 == 0 { MOVED } else { AT_ACME };
            alice.replace_memory(&first.id, fact(text)).await.unwrap();
        }
        assert_eq!(alice.memory_history(&first.id).await.unwrap().len(), 5);
    }

    #[tokio::test]
    async fn a_close_memory_counts_toward_curating_its_scope() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let alice = User::for_test(store, "alice");
        let mac = Scope::Machine(iroh::SecretKey::generate().public());
        let on = |content| Fact {
            scope: Some(mac.clone()),
            ..fact(content)
        };
        alice
            .remember(on("The build box has Docker installed."))
            .await
            .unwrap();
        let close = alice
            .remember(on("The build box has Podman installed."))
            .await
            .unwrap();
        assert!(!close.replaced, "close is not a duplicate");
        let states = alice.store.curation_states(&alice.key).await.unwrap();
        assert_eq!(states[&mac].close, 1);
        assert!(!states.contains_key(&Scope::User));
    }

    #[tokio::test]
    async fn curation_merges_within_one_scope_and_keeps_what_it_replaced() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let alice = User::for_test(store, "alice");
        let mac = Scope::Machine(iroh::SecretKey::generate().public());
        let on = |content| Fact {
            scope: Some(mac.clone()),
            ..fact(content)
        };
        let docker = alice.remember(on("Has Docker.")).await.unwrap().id;
        let compose = alice.remember(on("Runs Compose stacks.")).await.unwrap().id;
        let mine = alice.remember(fact("Lives in Lisbon.")).await.unwrap().id;
        for _ in 0..3 {
            alice
                .store
                .note_signal(&alice.key, &mac, Signal::Close)
                .await
                .unwrap();
        }
        let needs = alice.curation_needs().await.unwrap();
        assert_eq!(needs.len(), 1);
        assert_eq!(needs[0].scope, mac);

        // The answer also names the user's memory and a profile; neither
        // belongs to a machine's curation.
        let answer = Answer(format!(
            r#"{{"ops":[{{"op":"merge","ids":["{docker}","{compose}"],"content":"Runs Compose stacks on Docker."}},
            {{"op":"forget","id":"{mine}"}}],"core":["{docker}","{mine}"]}}"#
        ));
        let llm = llm();
        let generator = Generator {
            provider: &answer,
            llm: &llm,
            user: "alice",
        };
        alice.curate_if_needed(generator).await;

        let rows = alice.memory_rows().await.unwrap();
        let contents: Vec<&str> = rows.iter().map(|row| row.content.as_str()).collect();
        assert_eq!(rows.len(), 2, "{contents:?}");
        let merged = rows.iter().find(|row| row.scope == mac).unwrap();
        assert_eq!(merged.content, "Runs Compose stacks on Docker.");
        assert!(!merged.core);
        let user_row = rows.iter().find(|row| row.scope == Scope::User).unwrap();
        assert_eq!(user_row.id, mine);
        assert!(!user_row.core);

        let mut was: Vec<String> = alice
            .memory_history(&merged.id)
            .await
            .unwrap()
            .into_iter()
            .map(|old| old.row.content)
            .collect();
        was.sort();
        assert_eq!(was, ["Has Docker.", "Runs Compose stacks."]);
        let dropped = if merged.id == docker {
            &compose
        } else {
            &docker
        };
        assert!(
            !alice.forget(dropped).await.unwrap(),
            "merged away, not a memory"
        );

        // Curated: the signals are cleared and the size recorded.
        let states = alice.store.curation_states(&alice.key).await.unwrap();
        assert_eq!(states[&mac].close, 0);
        assert_eq!(states[&mac].size, 1);
        assert!(states[&mac].curated_at.is_some());
        assert!(alice.curation_needs().await.unwrap().is_empty());
    }
}
