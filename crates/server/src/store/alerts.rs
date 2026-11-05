//! Alerts: what a woken run told the user, kept as a chat the lists leave
//! out until the user replies in it.

use super::Store;
use crate::account::user::UserKey;
use anyhow::Result;
use chrono::{DateTime, Utc};
use sqlx::{PgConnection, Row};
use uuid::Uuid;

/// Alerts the list holds at most, newest first.
const LISTED: i64 = 50;

/// One alert as listed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlertRow {
    /// Its chat, which a reply turns into a normal one.
    pub chat: Uuid,
    pub title: String,
    pub source: String,
    pub text: String,
    pub read: bool,
    pub at: DateTime<Utc>,
}

/// Makes `chat` a normal chat again, when it was an alert.
pub(super) async fn clear_alert(tx: &mut PgConnection, chat: Uuid) -> Result<()> {
    sqlx::query("DELETE FROM chat_alerts WHERE chat_id = $1")
        .bind(chat)
        .execute(tx)
        .await?;
    Ok(())
}

impl Store {
    /// Makes the user's session `chat` an alert that says `text`.
    pub(crate) async fn add_alert(
        &self,
        user: &UserKey,
        chat: Uuid,
        source: &str,
        text: &str,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO chat_alerts (chat_id, source, text)
             SELECT c.id, $3, $4 FROM chats c
             JOIN chat_members m ON m.chat_id = c.id
             WHERE c.id = $1 AND m.user_id = $2 AND c.kind = 'session'
             ON CONFLICT DO NOTHING",
        )
        .bind(chat)
        .bind(user.as_str())
        .bind(source)
        .bind(text)
        .execute(&self.pool)
        .await?;
        // Past the list's cap an alert would be in no list at all, so the
        // oldest go with their chats.
        let expired: Vec<Uuid> = sqlx::query_scalar(
            "SELECT a.chat_id FROM chat_alerts a JOIN chat_members m ON m.chat_id = a.chat_id
             WHERE m.user_id = $1
             ORDER BY a.created_at DESC, a.chat_id OFFSET $2",
        )
        .bind(user.as_str())
        .bind(LISTED)
        .fetch_all(&self.pool)
        .await?;
        if !expired.is_empty() {
            self.delete_sessions(user, &expired).await?;
        }
        Ok(())
    }

    /// The user's alerts, newest first.
    pub(crate) async fn alerts(&self, user: &UserKey) -> Result<Vec<AlertRow>> {
        let rows = sqlx::query(
            "SELECT a.chat_id, COALESCE(c.title, a.source), a.source, a.text, a.read, a.created_at
             FROM chat_alerts a JOIN chats c ON c.id = a.chat_id
             JOIN chat_members m ON m.chat_id = c.id
             WHERE m.user_id = $1
             ORDER BY a.created_at DESC, a.chat_id LIMIT $2",
        )
        .bind(user.as_str())
        .bind(LISTED)
        .fetch_all(&self.pool)
        .await?;
        rows.iter()
            .map(|r| {
                Ok(AlertRow {
                    chat: r.try_get(0)?,
                    title: r.try_get(1)?,
                    source: r.try_get(2)?,
                    text: r.try_get(3)?,
                    read: r.try_get(4)?,
                    at: r.try_get(5)?,
                })
            })
            .collect()
    }

    /// Marks the user's alerts among `ids` read.
    pub(crate) async fn read_alerts(&self, user: &UserKey, ids: &[Uuid]) -> Result<()> {
        sqlx::query(
            "UPDATE chat_alerts a SET read = TRUE FROM chat_members m
             WHERE m.chat_id = a.chat_id AND m.user_id = $1 AND a.chat_id = ANY($2)",
        )
        .bind(user.as_str())
        .bind(ids)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Deletes the user's alerts among `ids` with their chats. A chat the
    /// user replied in is no alert any more, so it stays.
    pub(crate) async fn dismiss_alerts(&self, user: &UserKey, ids: &[Uuid]) -> Result<()> {
        let alerts: Vec<Uuid> = sqlx::query_scalar(
            "SELECT a.chat_id FROM chat_alerts a JOIN chat_members m ON m.chat_id = a.chat_id
             WHERE m.user_id = $1 AND a.chat_id = ANY($2)",
        )
        .bind(user.as_str())
        .bind(ids)
        .fetch_all(&self.pool)
        .await?;
        self.delete_sessions(user, &alerts).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{CreateRun, RunStatus, TestDb, test_chat};

    #[tokio::test]
    async fn alerts_past_the_cap_are_deleted_with_their_chats() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let alice = UserKey::for_test("alice");
        let mut chats = Vec::new();
        for n in 0..=LISTED {
            let chat = test_chat(&format!("alert-{n}"));
            let made = store
                .create_run(&alice, chat, &format!("req-{n}"), "wake")
                .await
                .unwrap();
            let CreateRun::Created(run) = made else {
                panic!("the run was not created");
            };
            store
                .finish(&alice, &run, None, RunStatus::Completed)
                .await
                .unwrap();
            store
                .add_alert(&alice, chat, "Apps", "Hello")
                .await
                .unwrap();
            chats.push(chat);
        }
        let listed = store.alerts(&alice).await.unwrap();
        assert_eq!(listed.len(), LISTED as usize);
        // The oldest is gone with its chat, so it is no stray chat in a list.
        assert!(listed.iter().all(|a| a.chat != chats[0]));
        assert!(!store.in_chat(&alice, chats[0]).await.unwrap());
    }
}
