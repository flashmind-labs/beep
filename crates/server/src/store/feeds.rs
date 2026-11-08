//! Sync state of the feeds read for memories.

use super::Store;
use crate::{
    account::user::UserKey,
    apps::feeds::{mail::Mailbox, slack::SLACK},
};
use anyhow::Result;
use chrono::{DateTime, Utc};
use std::{collections::HashSet, str::FromStr, time::Duration};
use strum::ParseError;

/// A mailbox or Slack synced daily for memories; its slug keys the sync tables (which keep their mail names).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Feed {
    Mail(Mailbox),
    Slack,
}

impl Feed {
    pub const ALL: [Feed; 4] = [
        Feed::Mail(Mailbox::Gmail),
        Feed::Mail(Mailbox::Fastmail),
        Feed::Mail(Mailbox::Outlook),
        Feed::Slack,
    ];
}

impl From<Feed> for &'static str {
    fn from(feed: Feed) -> Self {
        match feed {
            Feed::Mail(mailbox) => Into::<&'static str>::into(mailbox),
            Feed::Slack => SLACK,
        }
    }
}

impl AsRef<str> for Feed {
    fn as_ref(&self) -> &str {
        Into::<&'static str>::into(*self)
    }
}

impl FromStr for Feed {
    type Err = ParseError;

    fn from_str(slug: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|feed| feed.as_ref() == slug)
            .ok_or(ParseError::VariantNotFound)
    }
}

impl Store {
    /// Whether `feed` finished a sync within `every`.
    pub(crate) async fn feed_synced_recently(
        &self,
        user: &UserKey,
        feed: Feed,
        every: Duration,
    ) -> Result<bool> {
        Ok(sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM mail_sync WHERE user_id=$1 AND mailbox=$2
                 AND synced_at > now() - make_interval(secs => $3))",
        )
        .bind(user.as_ref())
        .bind(feed.as_ref())
        .bind(every.as_secs_f64())
        .fetch_one(&self.pool)
        .await?)
    }

    pub(crate) async fn mark_feed_synced(&self, user: &UserKey, feed: Feed) -> Result<()> {
        sqlx::query(
            "INSERT INTO mail_sync (user_id, mailbox, synced_at) VALUES ($1, $2, now())
             ON CONFLICT (user_id, mailbox) DO UPDATE SET synced_at=excluded.synced_at",
        )
        .bind(user.as_ref())
        .bind(feed.as_ref())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// When `feed` last finished a sync.
    pub(crate) async fn feed_synced_at(
        &self,
        user: &UserKey,
        feed: Feed,
    ) -> Result<Option<DateTime<Utc>>> {
        Ok(
            sqlx::query_scalar("SELECT synced_at FROM mail_sync WHERE user_id=$1 AND mailbox=$2")
                .bind(user.as_ref())
                .bind(feed.as_ref())
                .fetch_optional(&self.pool)
                .await?,
        )
    }

    /// Ids of the items of `feed` already processed.
    pub(crate) async fn feed_seen(&self, user: &UserKey, feed: Feed) -> Result<HashSet<String>> {
        let ids: Vec<String> =
            sqlx::query_scalar("SELECT message_id FROM mail_seen WHERE user_id=$1 AND mailbox=$2")
                .bind(user.as_ref())
                .bind(feed.as_ref())
                .fetch_all(&self.pool)
                .await?;
        Ok(ids.into_iter().collect())
    }

    pub(crate) async fn mark_feed_seen(
        &self,
        user: &UserKey,
        feed: Feed,
        id: &str,
        received: DateTime<Utc>,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO mail_seen (user_id, mailbox, message_id, received_at)
             VALUES ($1, $2, $3, $4) ON CONFLICT DO NOTHING",
        )
        .bind(user.as_ref())
        .bind(feed.as_ref())
        .bind(id)
        .bind(received)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Forgets processed ids older than `before`, which no run reads again.
    pub(crate) async fn prune_feed_seen(
        &self,
        user: &UserKey,
        feed: Feed,
        before: DateTime<Utc>,
    ) -> Result<usize> {
        let deleted = sqlx::query(
            "DELETE FROM mail_seen WHERE user_id=$1 AND mailbox=$2 AND received_at < $3",
        )
        .bind(user.as_ref())
        .bind(feed.as_ref())
        .bind(before)
        .execute(&self.pool)
        .await?
        .rows_affected();
        Ok(usize::try_from(deleted)?)
    }
}
