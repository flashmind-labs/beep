//! The skills watcher: the skill review of `crate::skills`, run on a
//! finished conversation instead of every few turns.

use super::{
    WatcherKind,
    runner::{Review, Watcher},
};
use crate::skills::{self, Audience};
use anyhow::Result;
use async_trait::async_trait;

pub(crate) struct SkillsWatcher;

#[async_trait]
impl Watcher for SkillsWatcher {
    fn kind(&self) -> WatcherKind {
        WatcherKind::Skills
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

    #[tokio::test]
    async fn a_finished_chat_becomes_a_skill() {
        let model = Scripted::default().answer(
            "You keep Beep's skills",
            r#"{"changes":[{"op":"create","name":"Book padel","description":"Booking a padel court","body":"1. Ask for 19:00 at Club Norte."}]}"#,
        );
        let h = Harness::new(model).await;
        let chat = test_chat("padel");
        h.turn(&h.alice, chat, "book padel at Club Norte at 19:00")
            .await;
        let slot = WatcherSettings {
            kind: WatcherKind::Skills,
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
        let mut runner = Runner::new(&h.app, settings, vec![(Arc::new(SkillsWatcher), slot)]);
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
