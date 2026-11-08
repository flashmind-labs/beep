//! The skills watcher: `crate::memory::skills`'s review, run on a finished conversation.

use super::{
    WatcherKind, gate,
    runner::{Review, Watcher},
};
use crate::{
    memory::skills::{self, Audience},
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
#[path = "../../tests/memory_watchers_skills.rs"]
mod tests;
