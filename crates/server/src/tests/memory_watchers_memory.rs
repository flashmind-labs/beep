use super::*;
use crate::{
    memory::watchers::{
        runner::{Runner, Settings, TimingSettings, WatcherSettings},
        testing::{Harness, Scripted},
    },
    store::test_chat,
};
use chrono::{TimeDelta, Utc};
use flashmind::types::AgentLlmConfig;
use iroh::SecretKey;
use std::{sync::Arc, time::Duration};

fn machine(name: &str, key: &EndpointId) -> NodeRow {
    NodeRow {
        endpoint: key.to_string(),
        name: name.to_owned(),
        version: String::new(),
        env: None,
        seen_at: Utc::now(),
    }
}

#[test]
fn reads_the_scopes_it_offers() {
    let key = SecretKey::generate().public();
    let machines = [machine("build-box", &key)];
    assert_eq!(parse_scope("user", &machines), Some(Scope::User));
    assert_eq!(
        parse_scope("project: Beep", &machines),
        Some(Scope::Project("beep".into()))
    );
    assert_eq!(
        parse_scope("account:GitHub", &machines),
        Some(Scope::Account("github".into()))
    );
    assert_eq!(
        parse_scope("machine:Build-Box", &machines),
        Some(Scope::Machine(key))
    );
    assert_eq!(parse_scope("machine:other-box", &machines), None);
    assert_eq!(parse_scope("planet:mars", &machines), None);
    assert_eq!(parse_scope("project:", &machines), None);
    for scope in [
        Scope::User,
        Scope::Machine(key),
        Scope::Project("beep".into()),
        Scope::Account("github".into()),
    ] {
        assert_eq!(
            parse_scope(&scope_name(&scope, &machines), &machines),
            Some(scope)
        );
    }
}

#[tokio::test]
async fn files_what_a_conversation_teaches_under_its_scope() {
    let model = Scripted::default().answer(
            "You keep the long-term memory of Beep, a personal assistant app. \
A conversation",
            r#"{"ops":[
                {"op":"add","content":"The beep repository is tested with just test.","scope":"project:beep","quote":"just test"},
                {"op":"add","content":"Has a sister named Ana.","scope":"user"},
                {"op":"add","content":"Lives on Mars.","scope":"planet:mars"}
            ]}"#,
        );
    let h = Harness::new(model).await;
    let chat = test_chat("memory");
    h.turn(
        &h.alice,
        chat,
        "run just test in the beep repo, my sister Ana asked",
    )
    .await;
    let slot = WatcherSettings {
        kind: WatcherKind::Memory,
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
        concurrency: 1,
        watchers: Vec::new(),
    };
    let mut runner = Runner::new(&h.app, settings, vec![(Arc::new(MemoryWatcher), slot)]);
    runner.tick(Utc::now() + TimeDelta::minutes(2)).await;
    assert!(runner.drain().await.is_empty());

    let rows = h.alice.memory_rows().await.unwrap();
    let scope_of = |content: &str| {
        rows.iter()
            .find(|row| row.content == content)
            .map(|row| row.scope.clone())
    };
    assert_eq!(
        scope_of("The beep repository is tested with just test."),
        Some(Scope::Project("beep".into()))
    );
    assert_eq!(scope_of("Has a sister named Ana."), Some(Scope::User));
    // A scope it cannot read files the fact under the user.
    assert_eq!(scope_of("Lives on Mars."), Some(Scope::User));
    let states = h.app.store.watcher_states(&[chat]).await.unwrap();
    assert!(states[&(chat, WatcherKind::Memory)].reviewed_seq > 0);
    let prompt = h.model.prompts().pop().unwrap_or_default();
    assert!(prompt.contains("The user's machines:\n(none)"));
    assert!(prompt.contains("User: run just test"));
}
