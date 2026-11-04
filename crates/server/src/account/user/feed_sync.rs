//! The user's mail feeds: sync claims, seen items, the digest and attention items.

use super::User;
use crate::{
    apps::feeds::mail::Mailbox,
    store::{AttentionItem, Feed, ProfileNote, ProfileRead},
};
use anyhow::Result;
use chrono::{DateTime, Utc};
use flashmind::cron::CronJob;
use serde_json::Value;
use std::{collections::HashSet, time::Duration};

impl User {
    /// Whether `feed` finished a sync within `every`.
    pub async fn feed_synced_recently(&self, feed: Feed, every: Duration) -> Result<bool> {
        self.store
            .feed_synced_recently(&self.key, feed, every)
            .await
    }

    /// Records that `feed` was just synced.
    pub async fn mark_feed_synced(&self, feed: Feed) -> Result<()> {
        self.store.mark_feed_synced(&self.key, feed).await
    }

    /// How far the profile pass got through `source`.
    pub(crate) async fn profile_read(&self, source: &str) -> Result<ProfileRead> {
        self.store.profile_read(&self.key, source).await
    }

    /// Saves what the profile pass noted and how far it read.
    pub(crate) async fn add_profile_notes(
        &self,
        source: &str,
        notes: &[ProfileNote],
        read_before: Option<DateTime<Utc>>,
    ) -> Result<()> {
        self.store
            .add_profile_notes(&self.key, source, notes, read_before)
            .await
    }

    pub(crate) async fn finish_profile_read(&self, source: &str) -> Result<()> {
        self.store.finish_profile_read(&self.key, source).await
    }

    pub(crate) async fn reset_profile_read(&self, source: &str) -> Result<()> {
        self.store.reset_profile_read(&self.key, source).await
    }

    pub(crate) async fn profile_notes(&self) -> Result<Vec<ProfileNote>> {
        self.store.profile_notes(&self.key).await
    }

    /// The input schema of `tool` on MCP server `server`, once it has connected.
    pub(crate) async fn mcp_tool_schema(&self, server: &str, tool: &str) -> Option<Value> {
        let tools = self.mcp_registry().await.current_mcp_tools();
        tools
            .get(server)?
            .iter()
            .find(|def| def.name == tool)
            .map(|def| def.input_schema.clone())
    }

    /// When `feed` last finished a sync.
    pub async fn feed_synced_at(&self, feed: Feed) -> Result<Option<DateTime<Utc>>> {
        self.store.feed_synced_at(&self.key, feed).await
    }

    /// Ids of the items of `feed` already processed.
    pub async fn feed_seen(&self, feed: Feed) -> Result<HashSet<String>> {
        self.store.feed_seen(&self.key, feed).await
    }

    /// Records an item of `feed` as read, so the next sync skips it.
    pub async fn mark_feed_seen(
        &self,
        feed: Feed,
        id: &str,
        received: DateTime<Utc>,
    ) -> Result<()> {
        self.store
            .mark_feed_seen(&self.key, feed, id, received)
            .await
    }

    /// Forgets processed ids older than `before`, which no run reads again.
    pub async fn prune_feed_seen(&self, feed: Feed, before: DateTime<Utc>) -> Result<usize> {
        self.store.prune_feed_seen(&self.key, feed, before).await
    }

    /// Makes the digest routine `job` unless one was made before.
    pub async fn offer_digest(&self, job: &CronJob) -> Result<bool> {
        self.store.offer_digest(&self.key, job).await
    }

    /// Whether the user has a digest routine switched on.
    pub async fn digest_enabled(&self) -> Result<bool> {
        self.store.digest_enabled(&self.key).await
    }

    /// Keeps an email for the evening digest.
    pub async fn flag_attention(&self, item: &AttentionItem) -> Result<()> {
        self.store.flag_attention(&self.key, item).await
    }

    /// Emails waiting for the digest, oldest first; older than `keep` are dropped.
    pub async fn attention_items(&self, keep: Duration) -> Result<Vec<AttentionItem>> {
        self.store.attention_items(&self.key, keep).await
    }

    /// Forgets flagged emails of `mailbox` the digest dealt with.
    pub async fn clear_attention(&self, mailbox: Mailbox, ids: &[String]) -> Result<()> {
        self.store.clear_attention(&self.key, mailbox, ids).await
    }
}
