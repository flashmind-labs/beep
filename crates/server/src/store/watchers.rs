//! What watchers know about each chat: its progress and how far each watcher read.

use super::Store;
use crate::watchers::{ChatActivity, WatcherKind, WatcherState};
use anyhow::Result;
use chrono::{DateTime, Utc};
use sqlx::{Row, postgres::PgRow};
use std::collections::HashMap;
use uuid::Uuid;

/// The columns `activity` reads, from `chats c` and `chat_members m`.
const ACTIVITY: &str = "c.id, m.user_id, c.updated_at,
    COALESCE((SELECT max(id) FROM messages WHERE chat_id = c.id), 0),
    EXISTS (SELECT 1 FROM runs r WHERE r.chat_id = c.id AND r.status = 'running')";

fn activity(row: &PgRow) -> Result<ChatActivity> {
    Ok(ChatActivity {
        chat: row.try_get(0)?,
        user: row.try_get(1)?,
        last_at: row.try_get(2)?,
        last_seq: row.try_get(3)?,
        running: row.try_get(4)?,
    })
}

impl Store {
    /// Every user's sessions whose messages changed since `since`, with their user.
    pub(crate) async fn watched_chats(&self, since: DateTime<Utc>) -> Result<Vec<ChatActivity>> {
        let rows = sqlx::query(&format!(
            "SELECT {ACTIVITY} FROM chats c JOIN chat_members m ON m.chat_id = c.id
             WHERE c.kind = 'session' AND c.updated_at >= $1
             ORDER BY c.updated_at"
        ))
        .bind(since)
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(activity).collect()
    }

    /// `user`'s session `chat`, `None` when it is not one.
    pub(crate) async fn watched_chat(
        &self,
        user: &str,
        chat: Uuid,
    ) -> Result<Option<ChatActivity>> {
        let row = sqlx::query(&format!(
            "SELECT {ACTIVITY} FROM chats c JOIN chat_members m ON m.chat_id = c.id
             WHERE c.kind = 'session' AND m.user_id = $1 AND c.id = $2"
        ))
        .bind(user)
        .bind(chat)
        .fetch_optional(&self.pool)
        .await?;
        row.as_ref().map(activity).transpose()
    }

    /// How far each watcher read each of `chats`; missing means none.
    pub(crate) async fn watcher_states(
        &self,
        chats: &[Uuid],
    ) -> Result<HashMap<(Uuid, WatcherKind), WatcherState>> {
        let rows = sqlx::query(
            "SELECT chat_id, watcher, reviewed_seq, failures, failed_at
             FROM watcher_reviews WHERE chat_id = ANY($1)",
        )
        .bind(chats)
        .fetch_all(&self.pool)
        .await?;
        let mut states = HashMap::new();
        for row in rows {
            let watcher: String = row.try_get(1)?;
            // A watcher this server no longer runs is left out.
            let Ok(kind) = watcher.parse::<WatcherKind>() else {
                continue;
            };
            states.insert(
                (row.try_get(0)?, kind),
                WatcherState {
                    reviewed_seq: row.try_get(2)?,
                    failures: row.try_get(3)?,
                    failed_at: row.try_get(4)?,
                },
            );
        }
        Ok(states)
    }

    /// Records `watcher` read `chat` up to `seq` at `at` and clears failures; never moves back.
    pub(crate) async fn mark_watched(
        &self,
        chat: Uuid,
        watcher: WatcherKind,
        seq: i64,
        at: DateTime<Utc>,
    ) -> Result<()> {
        let watcher: &str = watcher.into();
        sqlx::query(
            "INSERT INTO watcher_reviews (chat_id, watcher, reviewed_seq, reviewed_at)
             VALUES ($1, $2, $3, $4)
             ON CONFLICT (chat_id, watcher) DO UPDATE SET
                reviewed_seq = GREATEST(watcher_reviews.reviewed_seq, EXCLUDED.reviewed_seq),
                reviewed_at = EXCLUDED.reviewed_at, failures = 0, failed_at = NULL",
        )
        .bind(chat)
        .bind(watcher)
        .bind(seq)
        .bind(at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Counts a failed review of `chat` by `watcher` at `at`; returns consecutive failures.
    pub(crate) async fn watch_failed(
        &self,
        chat: Uuid,
        watcher: WatcherKind,
        at: DateTime<Utc>,
    ) -> Result<i32> {
        let watcher: &str = watcher.into();
        Ok(sqlx::query_scalar(
            "INSERT INTO watcher_reviews (chat_id, watcher, failures, failed_at)
             VALUES ($1, $2, 1, $3)
             ON CONFLICT (chat_id, watcher) DO UPDATE SET
                failures = watcher_reviews.failures + 1, failed_at = EXCLUDED.failed_at
             RETURNING failures",
        )
        .bind(chat)
        .bind(watcher)
        .bind(at)
        .fetch_one(&self.pool)
        .await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        store::{TestDb, test_chat, test_turn},
        user::UserKey,
    };
    use chrono::TimeDelta;

    #[tokio::test]
    async fn keeps_each_watchers_place() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let alice = UserKey::for_test("alice");
        let chat = test_chat("watched");
        test_turn(&store, &alice, chat, "hello there", 1_000)
            .await
            .unwrap();
        let last = test_turn(&store, &alice, chat, "and again", 2_000)
            .await
            .unwrap();
        test_turn(
            &store,
            &UserKey::for_test("bob"),
            test_chat("bobs"),
            "hi",
            3_000,
        )
        .await
        .unwrap();

        let long_ago = Utc::now() - TimeDelta::days(1);
        let chats = store.watched_chats(long_ago).await.unwrap();
        assert_eq!(chats.len(), 2);
        let bobs = chats.iter().find(|found| found.user == "bob").unwrap();
        assert_eq!(bobs.chat, test_chat("bobs"));
        let alices = chats.iter().find(|found| found.user == "alice").unwrap();
        assert_eq!(alices.chat, chat);
        // The reply comes after the message that opened the last turn.
        assert!(alices.last_seq > last);
        assert!(!alices.running);
        let none = store.watched_chats(Utc::now() + TimeDelta::minutes(1));
        assert!(none.await.unwrap().is_empty());
        assert!(store.watched_chat("bob", chat).await.unwrap().is_none());

        let at = Utc::now();
        store
            .mark_watched(chat, WatcherKind::Memory, last, at)
            .await
            .unwrap();
        assert_eq!(
            store
                .watch_failed(chat, WatcherKind::Skills, at)
                .await
                .unwrap(),
            1
        );
        assert_eq!(
            store
                .watch_failed(chat, WatcherKind::Skills, at)
                .await
                .unwrap(),
            2
        );
        // An older place never moves it back.
        store
            .mark_watched(chat, WatcherKind::Memory, 1, at)
            .await
            .unwrap();
        let states = store.watcher_states(&[chat]).await.unwrap();
        let memory = &states[&(chat, WatcherKind::Memory)];
        assert_eq!(memory.reviewed_seq, last);
        assert_eq!(memory.failures, 0);
        let skills = &states[&(chat, WatcherKind::Skills)];
        assert_eq!(skills.reviewed_seq, 0);
        assert_eq!(skills.failures, 2);
        assert!(skills.failed_at.is_some());
        assert!(!states.contains_key(&(chat, WatcherKind::Improvement)));

        // Reading clears the failures.
        store
            .mark_watched(chat, WatcherKind::Skills, last, at)
            .await
            .unwrap();
        let states = store.watcher_states(&[chat]).await.unwrap();
        assert_eq!(states[&(chat, WatcherKind::Skills)].failures, 0);
        assert_eq!(states[&(chat, WatcherKind::Skills)].failed_at, None);

        let read = store
            .chat_messages_after(chat, last - 1, DateTime::UNIX_EPOCH, 10)
            .await
            .unwrap();
        assert_eq!(read.first().map(|m| m.id), Some(last));
        let later = store
            .chat_messages_after(chat, 0, Utc::now() + TimeDelta::minutes(1), 10)
            .await
            .unwrap();
        assert!(later.is_empty());
    }
}
