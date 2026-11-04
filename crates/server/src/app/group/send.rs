//! Posting to a group: members' messages and reminders.

use super::{super::App, FAILED, Typing, group_message, mentions_beep, said};
use crate::{
    account::user::User,
    agent::images::Picture,
    protocol::GroupMessage,
    store::{NewMessage, PicturesOf},
};
use anyhow::Result;
use std::sync::Arc;
use uuid::Uuid;

impl App {
    /// Posts `user`'s message to a group and starts Beep's reply on @beep; `None` when not a member.
    pub async fn send_to_group(
        self: &Arc<Self>,
        user: &Arc<User>,
        group_id: Uuid,
        request_id: Uuid,
        text: &str,
        pictures: &[Picture],
    ) -> Result<Option<GroupMessage>> {
        let posted = user
            .post_to_group(NewMessage {
                group_id,
                request_id,
                content: text,
            })
            .await?;
        let Some(posted) = posted else {
            return Ok(None);
        };
        let message = group_message(&posted.message, user.id());
        if !posted.new {
            return Ok(Some(message));
        }
        // Kept before anyone hears of the message, so they find its pictures.
        if !pictures.is_empty() {
            let of = PicturesOf {
                chat: group_id,
                message: posted.message.id,
            };
            if let Err(error) = user.save_pictures(of, pictures).await {
                log::warn!(
                    "unable to keep a group message's pictures; group={group_id}: {error:#}"
                );
            }
        }
        let Some(joined) = user.group(group_id).await? else {
            return Ok(Some(message));
        };
        self.group_notice(
            &joined.members,
            &joined,
            Some(user.id()),
            &said(&posted.message.author_name, text),
        );
        if self.decision().is_some() {
            let app = self.clone();
            let joined = joined.clone();
            let seq = posted.message.id;
            tokio::spawn(async move { app.learn_from_group(&joined, seq).await });
        }
        let mentioned = mentions_beep(text);
        if !mentioned && self.decision().is_none() {
            return Ok(Some(message));
        }
        let app = self.clone();
        let actor = user.clone();
        let mention = posted.message.id;
        tokio::spawn(async move {
            let group_id = joined.group.id;
            if !mentioned && !app.called_without_mention(&joined, mention).await {
                return;
            }
            if !app.start_typing(group_id) {
                return;
            }
            app.group_changed(&joined.members, group_id);
            let typing = Typing {
                app: app.clone(),
                group_id,
            };
            let reply = match app.answer_group(&actor, &joined, mention).await {
                Ok(reply) => reply,
                Err(error) => {
                    log::warn!("group run failed; group={group_id}: {error:#}");
                    FAILED.to_owned()
                }
            };
            let posted = app.store.post_beep(group_id, &reply).await;
            drop(typing);
            // Someone may have left or joined during the run.
            let members = match app.store.group_members(group_id).await {
                Ok(members) => members,
                Err(error) => {
                    log::warn!("unable to load members; group={group_id}: {error:#}");
                    return;
                }
            };
            match posted {
                Ok(_) => {
                    app.group_notice(&members, &joined, None, &said("Beep", &reply));
                    app.count_for_group_skills(&actor, &joined);
                }
                Err(error) => {
                    log::warn!("unable to post Beep's reply; group={group_id}: {error:#}");
                    app.group_changed(&members, group_id);
                }
            }
        });
        Ok(Some(message))
    }

    /// Posts `user`'s reminder into the group and wakes every member, unless `user` has left.
    pub(crate) async fn post_group_reminder(
        &self,
        user: &User,
        group_id: Uuid,
        message: &str,
    ) -> Result<()> {
        let Some(joined) = user.group(group_id).await? else {
            log::info!("group reminder skipped, its maker left; group={group_id}");
            return Ok(());
        };
        self.store.post_beep(group_id, message).await?;
        self.group_notice(&joined.members, &joined, None, &said("Beep", message));
        Ok(())
    }
}
