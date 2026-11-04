//! The keys of the user's account, invites and the actions log.

use super::User;
use crate::store::{AccountKey, ActionRow, Invite, Merge, MergeInto};
use anyhow::Result;
use iroh::EndpointId;
use std::time::Duration;

impl User {
    /// The account's primary key, then the keys linked to it.
    pub async fn account_keys(&self) -> Result<Vec<AccountKey>> {
        self.store.account_keys(&self.key).await
    }

    /// A code another device joins this account with, shown by `endpoint` under `name`.
    pub async fn create_invite(
        &self,
        endpoint: &EndpointId,
        name: &str,
        ttl: Duration,
    ) -> Result<Invite> {
        self.store
            .create_invite(&self.key, endpoint, name, ttl)
            .await
    }

    /// Moves this account into the one that made the code.
    pub async fn merge_into(&self, into: MergeInto<'_>) -> Result<Merge> {
        self.store.merge_into(&self.key, into).await
    }

    /// Stops a linked key signing in; `false` when the account has no such key.
    pub async fn revoke_key(&self, endpoint_id: &str) -> Result<bool> {
        self.store.revoke_key(&self.key, endpoint_id).await
    }

    /// Keeps what a turn did in the user's apps.
    pub async fn record_actions(&self, descriptions: &[String]) -> Result<()> {
        self.store.record_actions(&self.key, descriptions).await
    }

    /// The latest things Beep did in the user's apps, newest first.
    pub async fn recent_actions(&self, limit: i64) -> Result<Vec<ActionRow>> {
        self.store.recent_actions(&self.key, limit).await
    }
}
