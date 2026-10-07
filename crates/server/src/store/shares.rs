//! Shared sessions: frozen copies other users open by token.

use super::Store;
use crate::user::UserKey;
use anyhow::Result;
use chrono::{DateTime, Utc};
use flashmind::core::Conversation;
use sqlx::Row;
use uuid::Uuid;

/// A shared copy of a session.
pub struct Share {
    pub token: String,
    pub title: String,
    pub conversation: Conversation,
    pub shared_at: DateTime<Utc>,
}

/// What `Store::copy_share` did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CopiedShare {
    Copied,
    /// No share has that token.
    NotFound,
    /// The user already has a session with the new id.
    Taken,
}

impl Store {
    /// Shares `session_id` as `conversation`, replacing an earlier copy of it
    /// and keeping its token. `None` when the user has no such session.
    pub(crate) async fn share_session(
        &self,
        user: &UserKey,
        session_id: &str,
        conversation: &Conversation,
    ) -> Result<Option<Share>> {
        let row = sqlx::query(
            "INSERT INTO shares (token, user_id, session_id, title, conversation)
             SELECT $1, s.user_id, s.id, COALESCE(t.title, (SELECT substr(message, 1, 80) FROM runs r
                 WHERE r.user_id=s.user_id AND r.session_id=s.id ORDER BY r.seq LIMIT 1), 'Conversation'), $4
             FROM sessions s LEFT JOIN session_titles t ON t.user_id=s.user_id AND t.session_id=s.id
             WHERE s.user_id=$2 AND s.id=$3
             ON CONFLICT (user_id, session_id) DO UPDATE SET title=excluded.title,
                 conversation=excluded.conversation, shared_at=now()
             RETURNING token, title, shared_at",
        )
        .bind(Uuid::new_v4().simple().to_string())
        .bind(user.as_str())
        .bind(session_id)
        .bind(serde_json::to_string(conversation)?)
        .fetch_optional(&self.pool)
        .await?;
        row.map(|row| {
            Ok(Share {
                token: row.try_get(0)?,
                title: row.try_get(1)?,
                conversation: conversation.clone(),
                shared_at: row.try_get(2)?,
            })
        })
        .transpose()
    }

    /// Stops sharing `session_id`; `false` when it was not shared.
    pub(crate) async fn unshare_session(&self, user: &UserKey, session_id: &str) -> Result<bool> {
        let deleted = sqlx::query("DELETE FROM shares WHERE user_id=$1 AND session_id=$2")
            .bind(user.as_str())
            .bind(session_id)
            .execute(&self.pool)
            .await?
            .rows_affected();
        Ok(deleted > 0)
    }

    /// The share with `token`, whoever made it.
    pub async fn share(&self, token: &str) -> Result<Option<Share>> {
        let row =
            sqlx::query("SELECT token, title, conversation, shared_at FROM shares WHERE token=$1")
                .bind(token)
                .fetch_optional(&self.pool)
                .await?;
        row.map(|row| {
            Ok(Share {
                token: row.try_get(0)?,
                title: row.try_get(1)?,
                conversation: serde_json::from_str(row.try_get(2)?)?,
                shared_at: row.try_get(3)?,
            })
        })
        .transpose()
    }

    /// Copies the share with `token` into a new session `session_id` of the
    /// user, under the share's title.
    pub(crate) async fn copy_share(
        &self,
        user: &UserKey,
        token: &str,
        session_id: &str,
    ) -> Result<CopiedShare> {
        let mut tx = self.pool.begin().await?;
        let row = sqlx::query("SELECT title, conversation FROM shares WHERE token=$1")
            .bind(token)
            .fetch_optional(&mut *tx)
            .await?;
        let Some(row) = row else {
            return Ok(CopiedShare::NotFound);
        };
        let title: String = row.try_get(0)?;
        let conversation: String = row.try_get(1)?;
        let inserted = sqlx::query(
            "INSERT INTO sessions (user_id, id, conversation, updated_at)
             VALUES ($1, $2, $3, now()) ON CONFLICT DO NOTHING",
        )
        .bind(user.as_str())
        .bind(session_id)
        .bind(conversation)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if inserted == 0 {
            return Ok(CopiedShare::Taken);
        }
        sqlx::query(
            "INSERT INTO session_titles (user_id, session_id, title, titled_at)
             VALUES ($1, $2, $3, now())
             ON CONFLICT (user_id, session_id) DO UPDATE SET title=excluded.title,
                 titled_at=excluded.titled_at",
        )
        .bind(user.as_str())
        .bind(session_id)
        .bind(title)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(CopiedShare::Copied)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{CreateRun, FinishedTurn, RunStatus, TestDb};
    use crate::user::User;
    use flashmind::core::ConversationEntry;

    async fn chat(user: &User, session: &str, text: &str) {
        let CreateRun::Created(run) = user
            .create_run(session, &format!("req-{session}"), text)
            .await
            .unwrap()
        else {
            panic!("run was not created");
        };
        let mut chat = Conversation::new();
        chat.add(ConversationEntry::user(text));
        let turn = FinishedTurn {
            transcript: &chat,
            context: &chat,
            prompt_tokens: 10,
        };
        user.finish(&run, Some(turn), RunStatus::Completed)
            .await
            .unwrap();
    }

    fn texts(conversation: &Conversation) -> Vec<&str> {
        conversation.entries().iter().map(|e| e.content()).collect()
    }

    #[tokio::test]
    async fn a_share_keeps_its_token_and_others_can_copy_it() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let alice = User::for_test(store.clone(), "alice");
        let bob = User::for_test(store.clone(), "bob");
        chat(&alice, "s1", "hello").await;

        let mut copy = Conversation::new();
        copy.add(ConversationEntry::user("first"));
        assert!(bob.share_session("s1", &copy).await.unwrap().is_none());
        let first = alice.share_session("s1", &copy).await.unwrap().unwrap();
        assert_eq!(first.title, "hello");
        copy.add(ConversationEntry::assistant("second"));
        let again = alice.share_session("s1", &copy).await.unwrap().unwrap();
        assert_eq!(again.token, first.token);
        let shared = store.share(&first.token).await.unwrap().unwrap();
        assert_eq!(texts(&shared.conversation), ["first", "second"]);

        assert_eq!(
            bob.copy_share(&first.token, "b1").await.unwrap(),
            CopiedShare::Copied
        );
        assert_eq!(
            bob.copy_share(&first.token, "b1").await.unwrap(),
            CopiedShare::Taken
        );
        assert_eq!(
            bob.copy_share("missing", "b2").await.unwrap(),
            CopiedShare::NotFound
        );
        let copied = bob.conversation("b1").await.unwrap().unwrap();
        assert_eq!(texts(&copied), ["first", "second"]);
        let sessions = bob.sessions().await.unwrap();
        assert_eq!(sessions[0].title, "hello");
        assert!(bob.rename_session("b1", "mine").await.unwrap());

        alice.delete_session("s1").await.unwrap();
        assert!(store.share(&first.token).await.unwrap().is_none());
        assert!(bob.conversation("b1").await.unwrap().is_some());
    }

    #[tokio::test]
    async fn unsharing_breaks_the_link() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let alice = User::for_test(store.clone(), "alice");
        chat(&alice, "s1", "hello").await;
        let share = alice
            .share_session("s1", &Conversation::new())
            .await
            .unwrap()
            .unwrap();
        assert!(alice.unshare_session("s1").await.unwrap());
        assert!(!alice.unshare_session("s1").await.unwrap());
        assert!(store.share(&share.token).await.unwrap().is_none());
    }
}
