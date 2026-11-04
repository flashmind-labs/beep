use super::*;
use crate::{
    memory::memories,
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

// Under the word embedder these two score above `DUPLICATE`.
const AT_ACME: &str =
    "Works remotely for Acme building mobile apps with the design team from Lisbon every week.";
const MOVED: &str =
    "Works remotely for Acme building mobile apps with the design team from Porto every week.";

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

    // The answer's user memory and profile do not belong to a machine's curation.
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
