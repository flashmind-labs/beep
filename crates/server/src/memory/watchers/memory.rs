//! The memory watcher: stores, updates or forgets memories from a finished conversation. Memory text is never logged.

use super::{
    WatcherKind,
    runner::{Review, Watcher},
};
use crate::{
    memory::memories::{Hit, LEARN_RULES, clip, days, parse_learn_ops},
    protocol::MemorySource,
    store::{NodeRow, Scope},
};
use anyhow::Result;
use async_trait::async_trait;
use iroh::EndpointId;
use std::fmt::Write;

/// How much of each side of the conversation the memory searches read.
const QUERY_CHARS: usize = 2_000;
/// Known memories each search brings.
const KNOWN_PER_SEARCH: usize = 20;

const INTRO: &str = "You keep the long-term memory of Beep, a personal assistant app. \
A conversation between the user and Beep just finished. You get all of it since it was last read, each \
message numbered, with the tool calls Beep made and what they returned. Decide what it teaches that is \
worth remembering. A quicker learner already read each turn as it ended and stored the obvious facts; \
they are among the known memories, so do not add them again, but fix one when the whole conversation shows \
it is wrong or incomplete. Most conversations teach little; then reply {\"ops\": []}.";

const SCOPES: &str = "Besides facts about the user, keep lasting facts about the user's projects, \
machines and accounts that Beep will need again, such as how a repository is built and tested, what runs \
on which machine, or how an account is set up. Never keep how one task went. \
Give every add and update a \"scope\" saying what the fact is about: \"user\" for the user and their \
life, \"project:<name>\" for a project such as a repository, \"machine:<name>\" for one of the user's \
machines listed below, by its name, or \"account:<app>\" for one of their accounts, such as \
\"account:github\". Known memories show their scope; an update may move a memory to a better one.";

pub(crate) struct MemoryWatcher;

#[async_trait]
impl Watcher for MemoryWatcher {
    fn kind(&self) -> WatcherKind {
        WatcherKind::Memory
    }

    async fn review(&self, review: &Review<'_>) -> Result<()> {
        if !review.transcript.talk {
            return Ok(());
        }
        let user = review.user;
        let machines = user.nodes().await?;
        let known = known(review).await?;
        let system = format!("{INTRO}\n{LEARN_RULES}\n{SCOPES}");
        let today = user.today().await;
        let prompt = prompt(
            &today.format("%A %Y-%m-%d").to_string(),
            &known,
            &machines,
            &review.transcript.text,
        );
        let answer = review.generator.text(&system, prompt).await?;
        let ops = parse_learn_ops(&answer)?;
        let resolve = |op: &crate::memory::memories::LearnOp| {
            op.scope
                .as_deref()
                .and_then(|scope| parse_scope(scope, &machines))
        };
        let learned = user
            .apply_all(
                &known,
                ops,
                MemorySource::Learned {},
                &review.chat.to_string(),
                &resolve,
            )
            .await;
        if !learned.is_empty() {
            log::info!(
                "memory watcher made {} change(s); user={user} chat={}",
                learned.len(),
                review.chat
            );
        }
        Ok(())
    }
}

/// Memories close to what the user and Beep said, plus who the user is and knows.
async fn known(review: &Review<'_>) -> Result<Vec<Hit>> {
    let user = review.user;
    let text = &review.transcript.text;
    let (said, replied) = sides(text);
    let mut known: Vec<Hit> = Vec::new();
    let mut add = |hits: Vec<Hit>| {
        for hit in hits {
            if !known.iter().any(|k| k.id == hit.id) {
                known.push(hit);
            }
        }
    };
    for query in [said, replied] {
        if !query.trim().is_empty() {
            add(user
                .search_memories(&query, None, None, KNOWN_PER_SEARCH)
                .await?);
        }
    }
    for category in ["identity", "people"] {
        add(user
            .search_memories("", Some(category), Some(&Scope::User), 15)
            .await?);
    }
    Ok(known)
}

/// What the user said and what Beep said in `transcript`, each clipped.
fn sides(transcript: &str) -> (String, String) {
    let (mut said, mut replied) = (String::new(), String::new());
    for line in transcript.lines() {
        let line = line.split_once("] ").map_or(line, |(_, rest)| rest);
        if let Some(text) = line.strip_prefix("User: ") {
            said.push_str(text);
            said.push('\n');
        } else if let Some(text) = line.strip_prefix("Beep: ") {
            replied.push_str(text);
            replied.push('\n');
        }
    }
    (clip(&said, QUERY_CHARS), clip(&replied, QUERY_CHARS))
}

fn scope_name(scope: &Scope, machines: &[NodeRow]) -> String {
    match scope {
        Scope::User => "user".to_owned(),
        Scope::Project(name) => format!("project:{name}"),
        Scope::Account(app) => format!("account:{app}"),
        Scope::Machine(id) => {
            let key = id.to_string();
            let name = machines
                .iter()
                .find(|machine| machine.endpoint == key)
                .map_or(key.as_str(), |machine| machine.name.as_str());
            format!("machine:{name}")
        }
    }
}

/// The scope the model named; a machine is named by its name.
fn parse_scope(text: &str, machines: &[NodeRow]) -> Option<Scope> {
    let text = text.trim();
    if text.eq_ignore_ascii_case("user") {
        return Some(Scope::User);
    }
    let (kind, name) = text.split_once(':')?;
    match kind.trim().to_ascii_lowercase().as_str() {
        "project" => Scope::project(name).ok(),
        "account" => Scope::account(name).ok(),
        "machine" => machines
            .iter()
            .find(|machine| machine.name.eq_ignore_ascii_case(name.trim()))
            .and_then(|machine| machine.endpoint.parse::<EndpointId>().ok())
            .map(Scope::Machine),
        _ => None,
    }
}

fn prompt(today: &str, known: &[Hit], machines: &[NodeRow], transcript: &str) -> String {
    let mut text = String::new();
    let _ = writeln!(text, "Today: {today}");
    let _ = writeln!(text, "\nThe user's machines:");
    if machines.is_empty() {
        let _ = writeln!(text, "(none)");
    }
    for machine in machines {
        let _ = writeln!(text, "- {}", machine.name);
    }
    let _ = writeln!(text, "\nKnown memories:");
    if known.is_empty() {
        let _ = writeln!(text, "(none related)");
    }
    for hit in known {
        let _ = writeln!(
            text,
            "- {} [{}, {}, {}, scope {}]{}: {}",
            hit.id,
            hit.category,
            hit.source.as_str(),
            hit.date,
            scope_name(&hit.scope, machines),
            days(&hit.valid),
            hit.content
        );
    }
    let _ = write!(text, "\nConversation:\n{transcript}\n");
    text
}

#[cfg(test)]
mod tests {
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
}
