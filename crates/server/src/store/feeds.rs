//! Sync state of the feeds read for memories.

use super::{Store, seconds};
use crate::{
    feeds::{mail::Mailbox, slack::SLACK},
    user::UserKey,
};
use anyhow::Result;
use chrono::{DateTime, Utc};
use std::{collections::HashSet, time::Duration};

/// Something synced once a day for memories: a mailbox or Slack. Its slug
/// keys the sync state tables, which predate Slack and keep their mail names.
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

    /// The feed of the app with `slug`, if that app is synced.
    pub fn from_slug(slug: &str) -> Option<Feed> {
        Feed::ALL.into_iter().find(|feed| feed.slug() == slug)
    }

    pub fn slug(self) -> &'static str {
        match self {
            Feed::Mail(mailbox) => mailbox.slug(),
            Feed::Slack => SLACK,
        }
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
        .bind(user.as_str())
        .bind(feed.slug())
        .bind(seconds(every))
        .fetch_one(&self.pool)
        .await?)
    }

    pub(crate) async fn mark_feed_synced(&self, user: &UserKey, feed: Feed) -> Result<()> {
        sqlx::query(
            "INSERT INTO mail_sync (user_id, mailbox, synced_at) VALUES ($1, $2, now())
             ON CONFLICT (user_id, mailbox) DO UPDATE SET synced_at=excluded.synced_at",
        )
        .bind(user.as_str())
        .bind(feed.slug())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// When the newest processed item of `feed` arrived.
    pub(crate) async fn feed_cursor(
        &self,
        user: &UserKey,
        feed: Feed,
    ) -> Result<Option<DateTime<Utc>>> {
        Ok(sqlx::query_scalar(
            "SELECT max(received_at) FROM mail_seen WHERE user_id=$1 AND mailbox=$2",
        )
        .bind(user.as_str())
        .bind(feed.slug())
        .fetch_one(&self.pool)
        .await?)
    }

    /// Ids of the items of `feed` already processed.
    pub(crate) async fn feed_seen(&self, user: &UserKey, feed: Feed) -> Result<HashSet<String>> {
        let ids: Vec<String> =
            sqlx::query_scalar("SELECT message_id FROM mail_seen WHERE user_id=$1 AND mailbox=$2")
                .bind(user.as_str())
                .bind(feed.slug())
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
        .bind(user.as_str())
        .bind(feed.slug())
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
        // The newest row is the cursor and always stays.
        let deleted = sqlx::query(
            "DELETE FROM mail_seen WHERE user_id=$1 AND mailbox=$2 AND received_at < $3
             AND received_at < (SELECT max(received_at) FROM mail_seen
                                WHERE user_id=$1 AND mailbox=$2)",
        )
        .bind(user.as_str())
        .bind(feed.slug())
        .bind(before)
        .execute(&self.pool)
        .await?
        .rows_affected();
        Ok(usize::try_from(deleted)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::TestDb;
    use chrono::NaiveDate;

    #[tokio::test]
    async fn feed_cursor_follows_seen_items_and_survives_pruning() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let day = |d: u32| {
            NaiveDate::from_ymd_opt(2026, 3, d)
                .unwrap()
                .and_hms_opt(12, 0, 0)
                .unwrap()
                .and_utc()
        };
        let gmail = Feed::Mail(Mailbox::Gmail);
        assert!(
            store
                .feed_cursor(&UserKey::for_test("alice"), gmail)
                .await
                .unwrap()
                .is_none()
        );
        for (id, d) in [("a", 1), ("b", 5), ("c", 3)] {
            store
                .mark_feed_seen(&UserKey::for_test("alice"), gmail, id, day(d))
                .await
                .unwrap();
        }
        store
            .mark_feed_seen(&UserKey::for_test("alice"), gmail, "a", day(9))
            .await
            .unwrap();
        assert_eq!(
            store
                .feed_cursor(&UserKey::for_test("alice"), gmail)
                .await
                .unwrap(),
            Some(day(5))
        );
        assert!(
            store
                .feed_cursor(&UserKey::for_test("alice"), Feed::Slack)
                .await
                .unwrap()
                .is_none()
        );

        // Everything is older than the cutoff, but the newest row stays.
        let pruned = store
            .prune_feed_seen(&UserKey::for_test("alice"), gmail, day(20))
            .await
            .unwrap();
        assert_eq!(pruned, 2);
        assert_eq!(
            store
                .feed_cursor(&UserKey::for_test("alice"), gmail)
                .await
                .unwrap(),
            Some(day(5))
        );
        let seen = store
            .feed_seen(&UserKey::for_test("alice"), gmail)
            .await
            .unwrap();
        assert_eq!(seen, HashSet::from(["b".to_owned()]));

        let every = Duration::from_secs(60);
        assert!(
            !store
                .feed_synced_recently(&UserKey::for_test("alice"), gmail, every)
                .await
                .unwrap()
        );
        store
            .mark_feed_synced(&UserKey::for_test("alice"), gmail)
            .await
            .unwrap();
        assert!(
            store
                .feed_synced_recently(&UserKey::for_test("alice"), gmail, every)
                .await
                .unwrap()
        );
        assert!(
            !store
                .feed_synced_recently(&UserKey::for_test("bob"), gmail, every)
                .await
                .unwrap()
        );
    }
}
