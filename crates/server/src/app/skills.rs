//! When to review a chat for skills, and what the review reads.

use super::App;
use crate::{
    skills::{self, Audience, REVIEW_TOOL_CALLS, REVIEW_TURNS},
    store::{JoinedGroup, Review, SkillOwner},
    user::User,
};
use chrono::Utc;
use std::sync::Arc;
use uuid::Uuid;

/// The group messages a review reads at most.
const GROUP_MESSAGES: i64 = 200;

impl App {
    /// Counts a finished turn of `user`'s chat that made `tool_calls`
    /// calls and, once it is time or when the user `corrected` how Beep
    /// went about a task, reviews the turns since the last review in the
    /// background.
    pub(super) fn count_for_skills(
        self: &Arc<Self>,
        user: &Arc<User>,
        chat: Uuid,
        tool_calls: usize,
        corrected: bool,
    ) {
        let app = self.clone();
        let user = user.clone();
        tokio::spawn(async move {
            let Some(review) = app.claim_review(chat, tool_calls, corrected).await else {
                return;
            };
            let context = match user.context(chat).await {
                Ok(Some(context)) => context.conversation,
                Ok(None) => return,
                Err(error) => {
                    log::warn!("skill review could not load the chat: {error:#}; user={user}");
                    return;
                }
            };
            let read = context
                .entries()
                .iter()
                .filter(|entry| review.since.is_none_or(|since| entry.timestamp > since));
            let transcript = skills::transcript(read);
            let generator = app.generator(user.id());
            let owner = user.skill_owner();
            match skills::review(
                &app.store,
                generator,
                &owner,
                Audience::Personal,
                &transcript,
            )
            .await
            {
                Ok(done) => log::info!(
                    "skill review: created {:?}, updated {:?}; user={user} chat={chat}",
                    done.created,
                    done.updated
                ),
                Err(error) => log::warn!("skill review failed: {error:#}; user={user}"),
            }
        });
    }

    /// Counts Beep's reply in a group and, every `REVIEW_TURNS` replies,
    /// reviews the messages since the last review for the group's skills.
    pub(super) fn count_for_group_skills(
        self: &Arc<Self>,
        actor: &Arc<User>,
        joined: &JoinedGroup,
    ) {
        let app = self.clone();
        let generator_user = actor.id().to_owned();
        let group_id = joined.group.id;
        tokio::spawn(async move {
            let Some(review) = app.claim_review(group_id, 0, false).await else {
                return;
            };
            let messages = match app
                .store
                .group_messages_until(group_id, i64::MAX, GROUP_MESSAGES)
                .await
            {
                Ok(messages) => messages,
                Err(error) => {
                    log::warn!(
                        "skill review could not load the group: {error:#}; group={group_id}"
                    );
                    return;
                }
            };
            let mut transcript = String::new();
            for message in messages
                .iter()
                .filter(|message| review.since.is_none_or(|since| message.sent_at > since))
            {
                let author = match message.author_id {
                    Some(_) => message.author_name.as_str(),
                    None => "Beep",
                };
                transcript.push_str(&format!("\n\n{author}: {}", message.content));
            }
            let generator = app.generator(&generator_user);
            let owner = SkillOwner::Group(group_id);
            match skills::review(
                &app.store,
                generator,
                &owner,
                Audience::Group,
                transcript.trim(),
            )
            .await
            {
                Ok(done) => log::info!(
                    "group skill review: created {:?}, updated {:?}; group={group_id}",
                    done.created,
                    done.updated
                ),
                Err(error) => log::warn!("group skill review failed: {error:#}; group={group_id}"),
            }
        });
    }

    /// Counts a turn in `chat` and, when a review is due, starts the count
    /// again and says what it should read. Claiming first keeps two turns
    /// that end together from reviewing the same text.
    async fn claim_review(&self, chat: Uuid, tool_calls: usize, corrected: bool) -> Option<Review> {
        let counted = self.store.count_skill_turn(chat).await;
        let review = match counted {
            Ok(review) => review,
            Err(error) => {
                log::warn!("unable to count a turn for skills: {error:#}; chat={chat}");
                return None;
            }
        };
        if review.turns < REVIEW_TURNS && tool_calls < REVIEW_TOOL_CALLS && !corrected {
            return None;
        }
        if let Err(error) = self.store.reviewed_skills(chat, Utc::now()).await {
            log::warn!("unable to start a skill review: {error:#}; chat={chat}");
            return None;
        }
        Some(review)
    }
}
