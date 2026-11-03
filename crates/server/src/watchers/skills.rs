//! The skills watcher: `crate::skills`'s review, run on a finished conversation.

use super::{
    WatcherKind, gate,
    runner::{Review, Watcher},
};
use crate::{
    skills::{self, Audience},
    store::StoredMessage,
};
use anyhow::Result;
use async_trait::async_trait;

pub(crate) struct SkillsWatcher;

#[async_trait]
impl Watcher for SkillsWatcher {
    fn kind(&self) -> WatcherKind {
        WatcherKind::Skills
    }

    /// Only a stretch where Beep did real work can hold steps worth keeping.
    fn worth_reading(&self, messages: &[StoredMessage]) -> bool {
        gate::busy(messages)
    }

    async fn review(&self, review: &Review<'_>) -> Result<()> {
        // Steps the user repeats come from what they ask.
        if !review.transcript.talk {
            return Ok(());
        }
        let user = review.user;
        let done = skills::review(
            &review.app.store,
            review.generator,
            &user.skill_owner(),
            Audience::Personal,
            &review.transcript.text,
        )
        .await?;
        if !done.created.is_empty() || !done.updated.is_empty() {
            log::info!(
                "skills watcher: created {:?}, updated {:?}; user={user} chat={}",
                done.created,
                done.updated,
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
    use chrono::{TimeDelta, Utc};
    use flashmind::types::AgentLlmConfig;
    use std::{sync::Arc, time::Duration};

    fn skills_runner(h: &Harness) -> Runner {
        let slot = WatcherSettings {
            kind: WatcherKind::Skills,
            llm: AgentLlmConfig::new("openai:test".parse().expect("a model name")),
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
        Runner::new(&h.app, settings, vec![(Arc::new(SkillsWatcher), slot)])
    }

    #[tokio::test]
    async fn a_chat_with_few_tool_calls_is_marked_read_without_the_model() {
        let h = Harness::new(Scripted::default()).await;
        let chat = test_chat("hello");
        let last = h.turn_with_calls(&h.alice, chat, "hello", 2).await;
        let mut runner = skills_runner(&h);
        runner.tick(Utc::now() + TimeDelta::minutes(2)).await;
        assert!(runner.drain().await.is_empty());
        assert!(h.model.prompts().is_empty());
        let states = h.app.store.watcher_states(&[chat]).await.expect("states");
        let state = states
            .get(&(chat, WatcherKind::Skills))
            .expect("the place is kept");
        assert_eq!(state.reviewed_seq, last);
    }

    #[tokio::test]
    async fn a_finished_chat_becomes_a_skill() {
        let model = Scripted::default().answer(
            "You keep Beep's skills",
            r#"{"changes":[{"op":"create","name":"Book padel","description":"Booking a padel court","body":"1. Ask for 19:00 at Club Norte."}]}"#,
        );
        let h = Harness::new(model).await;
        let chat = test_chat("padel");
        h.turn_with_calls(&h.alice, chat, "book padel at Club Norte at 19:00", 3)
            .await;
        let mut runner = skills_runner(&h);
        runner.tick(Utc::now() + TimeDelta::minutes(2)).await;
        runner.drain().await;
        let skills = h
            .app
            .store
            .owned_skills(&h.alice.skill_owner())
            .await
            .unwrap();
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].name, "Book padel");
        let prompts = h.model.prompts();
        assert!(
            prompts
                .iter()
                .any(|p| p.contains("User: book padel at Club Norte"))
        );
    }
}
