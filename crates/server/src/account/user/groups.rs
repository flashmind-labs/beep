//! The group chats the user is in.

use super::User;
use crate::store::{Join, JoinedGroup, NewMessage, Posted};
use anyhow::Result;
use uuid::Uuid;

impl User {
    /// Makes the group `id` with the user in it; `false` when the id is taken.
    pub async fn create_group(&self, id: Uuid, name: &str, member_name: &str) -> Result<bool> {
        self.store
            .create_group(&self.key, id, name, member_name)
            .await
    }

    /// The groups the user is in, most recently active first.
    pub async fn groups(&self) -> Result<Vec<JoinedGroup>> {
        self.store.groups(&self.key).await
    }

    /// The group `id`, if the user is in it.
    pub async fn group(&self, id: Uuid) -> Result<Option<JoinedGroup>> {
        self.store.group(&self.key, id).await
    }

    /// Posts to a group the user is in; `None` when they are not.
    pub async fn post_to_group(&self, message: NewMessage<'_>) -> Result<Option<Posted>> {
        self.store.post_to_group(&self.key, message).await
    }

    /// The invite token of a group the user is in, a new one with `reset`.
    pub async fn group_invite(&self, id: Uuid, reset: bool) -> Result<Option<String>> {
        self.store.group_invite(&self.key, id, reset).await
    }

    /// Joins the group behind `token`.
    pub async fn join_group(&self, token: &str, name: &str) -> Result<Join> {
        self.store.join_group(&self.key, token, name).await
    }

    /// Pins or unpins a group; `false` when the user is not in it.
    pub async fn pin_group(&self, id: Uuid, pinned: bool) -> Result<bool> {
        self.store.pin_group(&self.key, id, pinned).await
    }

    /// Lets Beep use the user's apps in a group when others ask; `false` when not a member.
    pub async fn share_group_apps(&self, id: Uuid, shared: bool) -> Result<bool> {
        self.store.share_group_apps(&self.key, id, shared).await
    }

    /// Leaves a group; `false` when the user was not in it.
    pub async fn leave_group(&self, id: Uuid) -> Result<bool> {
        self.store.leave_group(&self.key, id).await
    }
}
