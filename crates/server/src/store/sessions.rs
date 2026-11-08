//! Chat sessions: the list, titles, turns and model contexts.

use super::{
    Store,
    chats::{SESSION_TITLE, delete_chat, touch},
    updates::{Readers, UpdateOp, log_chat, log_messages},
};
use crate::{account::user::UserKey, protocol::Session};
use anyhow::{Result, bail};
use chrono::{DateTime, Utc};
use flashmind::core::{Conversation, EntryKind};
use sqlx::{Row, postgres::PgRow};
use std::time::Duration;
use uuid::Uuid;

/// Which turns `Store::delete_turn` removes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnsToDelete {
    /// Only the turn at the given time.
    One,
    /// That turn and every turn after it.
    Onward,
}

/// What `Store::delete_turn` did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeletedTurn {
    Deleted,
    /// No session, or no user message at that time.
    NotFound,
    /// A run is in progress and would save over the change.
    Busy,
}

/// A session `Store::search_sessions` found, with its latest matching message.
pub struct FoundSession {
    pub session: Session,
    pub text: String,
    /// The user wrote it, rather than Beep.
    pub from_user: bool,
}

/// One user's session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRef {
    pub user: String,
    pub id: Uuid,
}

/// A quiet session and how big its last prompt was.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuietSession {
    pub session: SessionRef,
    pub context_tokens: u32,
}

/// A session's model context plus the stored text it came from, for change checks on write.
pub struct StoredContext {
    raw: String,
    pub conversation: Conversation,
}

fn session_ref(row: &PgRow) -> Result<SessionRef> {
    Ok(SessionRef {
        user: row.try_get(0)?,
        id: row.try_get(1)?,
    })
}

/// Removes the turn sent at `timestamp` (or everything onward with `Onward`) from a context; says whether found.
fn remove_turn(conversation: &mut Conversation, timestamp: i64, which: TurnsToDelete) -> bool {
    let is_user = |kind: &EntryKind| matches!(kind, EntryKind::User { .. });
    // Each turn opens with developer context (time, place, apps). A
    // compaction summary is not part of any turn and stays.
    let is_context = |kind: &EntryKind| matches!(kind, EntryKind::Developer { tag, .. } if tag.as_deref() != Some("summary"));
    let entries = conversation.entries_mut();
    let asked = entries
        .iter()
        .position(|e| is_user(&e.kind) && e.timestamp.timestamp_millis() == timestamp);
    let Some(asked) = asked else {
        return false;
    };
    let context_before = |at: usize| {
        entries[..at]
            .iter()
            .rev()
            .take_while(|e| is_context(&e.kind))
            .count()
    };
    let start = asked - context_before(asked);
    let next = match which {
        TurnsToDelete::One => entries[asked + 1..]
            .iter()
            .position(|e| is_user(&e.kind))
            .map(|i| asked + 1 + i),
        TurnsToDelete::Onward => None,
    };
    let end = next.map_or(entries.len(), |next| {
        next - context_before(next).min(next - asked - 1)
    });
    entries.drain(start..end);
    true
}

/// Session id, title, pinned and updated_at columns, reading `chats c` and `chat_members m`.
const SESSION_COLUMNS: &str = "c.id, {title}, m.pinned, c.updated_at";

/// Listed once a turn is saved or its first run is going, so it shows everywhere at once.
const SHOWN: &str = "(EXISTS (SELECT 1 FROM sessions s WHERE s.chat_id = c.id)
    OR EXISTS (SELECT 1 FROM runs r WHERE r.chat_id = c.id AND r.status = 'running'))";

/// Which sessions a list or search takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Listing {
    /// Every session, alerts too (`ListSessions`, which released apps use).
    All,
    /// Sessions that are chats: alerts not replied to yet are left out.
    Chats,
}

impl Listing {
    /// A condition on `chats c`.
    fn condition(self) -> &'static str {
        match self {
            Listing::All => "TRUE",
            Listing::Chats => "NOT EXISTS (SELECT 1 FROM chat_alerts a WHERE a.chat_id = c.id)",
        }
    }
}

fn session_columns() -> String {
    SESSION_COLUMNS.replace("{title}", SESSION_TITLE)
}

/// A session from the columns id, title, pinned and updated_at.
fn session(r: &PgRow) -> Result<Session> {
    Ok(Session {
        id: r.try_get::<Uuid, _>(0)?.to_string(),
        title: r.try_get(1)?,
        pinned: r.try_get(2)?,
        updated_at: r.try_get::<DateTime<Utc>, _>(3)?.timestamp_millis(),
    })
}

/// `query` with LIKE's wildcards escaped, so a percent sign matches itself.
fn like_pattern(query: &str) -> String {
    let mut pattern = String::with_capacity(query.len());
    for c in query.chars() {
        if matches!(c, '\\' | '%' | '_') {
            pattern.push('\\');
        }
        pattern.push(c);
    }
    pattern
}

impl Store {
    pub(crate) async fn sessions(&self, user: &UserKey) -> Result<Vec<Session>> {
        self.listed_sessions(user, Listing::All).await
    }

    /// The sessions that are chats, as `sessions` lists them.
    pub(crate) async fn chats(&self, user: &UserKey) -> Result<Vec<Session>> {
        self.listed_sessions(user, Listing::Chats).await
    }

    async fn listed_sessions(&self, user: &UserKey, listing: Listing) -> Result<Vec<Session>> {
        let rows = sqlx::query(&format!(
            "SELECT {} FROM chats c
             JOIN chat_members m ON m.chat_id = c.id
             WHERE m.user_id = $1 AND c.kind = 'session' AND {SHOWN} AND {}
             ORDER BY m.pinned DESC, c.seq DESC LIMIT 200",
            session_columns(),
            listing.condition()
        ))
        .bind(user.as_ref())
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(session).collect()
    }

    /// Session `chat` of `user` as listed; `None` when not theirs.
    pub(crate) async fn session_of(&self, user: &UserKey, chat: Uuid) -> Result<Option<Session>> {
        let row = sqlx::query(&format!(
            "SELECT {} FROM chats c
             JOIN chat_members m ON m.chat_id = c.id
             WHERE m.user_id = $1 AND c.id = $2 AND c.kind = 'session'",
            session_columns()
        ))
        .bind(user.as_ref())
        .bind(chat)
        .fetch_optional(&self.pool)
        .await?;
        row.as_ref().map(session).transpose()
    }

    /// Up to `limit` sessions with a message containing `query` (case-insensitive), newest first, with the latest match.
    pub(crate) async fn search_sessions(
        &self,
        user: &UserKey,
        query: &str,
        limit: i64,
        listing: Listing,
    ) -> Result<Vec<FoundSession>> {
        let rows = sqlx::query(&format!(
            "SELECT DISTINCT ON (c.updated_at, c.id) {}, msg.content, msg.role = 'user'
             FROM chats c
             JOIN chat_members m ON m.chat_id = c.id
             JOIN messages msg ON msg.chat_id = c.id
             WHERE m.user_id = $1 AND c.kind = 'session'
               AND msg.role IN ('user', 'assistant') AND msg.content ILIKE $2 ESCAPE '\\'
               AND {}
             ORDER BY c.updated_at DESC, c.id, msg.id DESC LIMIT $3",
            session_columns(),
            listing.condition()
        ))
        .bind(user.as_ref())
        .bind(format!("%{}%", like_pattern(query)))
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        rows.iter()
            .map(|r| {
                Ok(FoundSession {
                    session: session(r)?,
                    text: r.try_get(4)?,
                    from_user: r.try_get(5)?,
                })
            })
            .collect()
    }

    pub(crate) async fn rename_session(
        &self,
        user: &UserKey,
        chat: Uuid,
        title: &str,
    ) -> Result<bool> {
        if title.is_empty() || title.chars().count() > 200 {
            bail!("session title must be 1-200 characters");
        }
        if !self.owns_session(user, chat).await? {
            return Ok(false);
        }
        // A title the user chose is never replaced by a generated one.
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "UPDATE chats SET title = $2, title_manual = TRUE, titled_at = now() WHERE id = $1",
        )
        .bind(chat)
        .bind(title)
        .execute(&mut *tx)
        .await?;
        let bumped = log_chat(&mut tx, chat, UpdateOp::Chat, Readers::Members).await?;
        tx.commit().await?;
        self.announce(&bumped);
        Ok(true)
    }

    /// Stores a generated title unless the user named the session; returns whether stored.
    pub async fn set_generated_title(&self, session: &SessionRef, title: &str) -> Result<bool> {
        let mut tx = self.pool.begin().await?;
        let stored = sqlx::query(
            "UPDATE chats SET title = $2, titled_at = now()
             WHERE id = $1 AND kind = 'session' AND NOT title_manual",
        )
        .bind(session.id)
        .bind(title)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        let bumped = if stored == 1 {
            log_chat(&mut tx, session.id, UpdateOp::Chat, Readers::Members).await?
        } else {
            Vec::new()
        };
        tx.commit().await?;
        self.announce(&bumped);
        Ok(stored == 1)
    }

    /// Whether `session` has only its first run and no title yet.
    pub async fn needs_first_title(&self, session: &SessionRef) -> Result<bool> {
        Ok(sqlx::query_scalar(
            "SELECT (SELECT COUNT(*) FROM runs WHERE chat_id = $1) = 1
                AND EXISTS(SELECT 1 FROM chats WHERE id = $1 AND title IS NULL)",
        )
        .bind(session.id)
        .fetch_one(&self.pool)
        .await?)
    }

    /// Sessions changed since titling, idle for `idle`, with no run; ones untouched for a day are skipped.
    pub async fn sessions_to_title(&self, idle: Duration) -> Result<Vec<SessionRef>> {
        let rows = sqlx::query(
            "SELECT m.user_id, c.id FROM chats c
             JOIN chat_members m ON m.chat_id = c.id
             JOIN sessions s ON s.chat_id = c.id
             WHERE c.kind = 'session'
               AND c.updated_at <= now() - make_interval(secs => $1)
               AND c.updated_at >= now() - interval '1 day'
               AND (c.titled_at IS NULL OR c.titled_at < c.updated_at)
               AND NOT c.title_manual
               AND NOT EXISTS (SELECT 1 FROM runs r WHERE r.chat_id = c.id AND r.status = 'running')
             LIMIT 20",
        )
        .bind(idle.as_secs_f64())
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(session_ref).collect()
    }

    /// The ones of `ids` that are the user's sessions.
    pub(crate) async fn existing_sessions(
        &self,
        user: &UserKey,
        ids: &[Uuid],
    ) -> Result<Vec<Uuid>> {
        Ok(sqlx::query_scalar(&format!(
            "SELECT c.id FROM chats c JOIN chat_members m ON m.chat_id = c.id
                 WHERE m.user_id = $1 AND c.kind = 'session' AND c.id = ANY($2) AND {SHOWN}"
        ))
        .bind(user.as_ref())
        .bind(ids)
        .fetch_all(&self.pool)
        .await?)
    }

    pub(crate) async fn delete_session(&self, user: &UserKey, chat: Uuid) -> Result<bool> {
        Ok(self.delete_sessions(user, &[chat]).await? > 0)
    }

    /// Deletes sessions with their runs, messages and files; returns how many existed.
    pub(crate) async fn delete_sessions(&self, user: &UserKey, ids: &[Uuid]) -> Result<u64> {
        let mut tx = self.pool.begin().await?;
        let owned: Vec<Uuid> = sqlx::query_scalar(
            "SELECT c.id FROM chats c JOIN chat_members m ON m.chat_id = c.id
             WHERE m.user_id = $1 AND c.kind = 'session' AND c.id = ANY($2)
             ORDER BY c.id FOR UPDATE OF c",
        )
        .bind(user.as_ref())
        .bind(ids)
        .fetch_all(&mut *tx)
        .await?;
        let mut files = Vec::new();
        let mut bumped = Vec::new();
        for &chat in &owned {
            let gone = delete_chat(&mut tx, chat).await?;
            files.extend(gone.files);
            bumped.extend(gone.bumped);
        }
        tx.commit().await?;
        self.announce(&bumped);
        self.blobs.delete(&files).await?;
        Ok(owned.len() as u64)
    }

    /// Atomically deletes `chat` if it shows nothing and has no run; false when kept or gone.
    pub(crate) async fn delete_empty_session(&self, user: &UserKey, chat: Uuid) -> Result<bool> {
        let mut tx = self.pool.begin().await?;
        // Locking the chat holds off a run or message that would name it.
        let empty: Option<bool> = sqlx::query_scalar(
            "SELECT NOT EXISTS(SELECT 1 FROM messages WHERE chat_id = c.id)
                 AND NOT EXISTS(SELECT 1 FROM runs WHERE chat_id = c.id AND status = 'running')
             FROM chats c JOIN chat_members m ON m.chat_id = c.id
             WHERE c.id = $1 AND m.user_id = $2 AND c.kind = 'session' FOR UPDATE OF c",
        )
        .bind(chat)
        .bind(user.as_ref())
        .fetch_optional(&mut *tx)
        .await?;
        if empty != Some(true) {
            tx.rollback().await?;
            return Ok(false);
        }
        let gone = delete_chat(&mut tx, chat).await?;
        tx.commit().await?;
        self.announce(&gone.bumped);
        self.blobs.delete(&gone.files).await?;
        Ok(true)
    }

    /// Removes the turn sent at `timestamp` (ms) from messages and context, and later turns with `Onward`.
    pub(crate) async fn delete_turn(
        &self,
        user: &UserKey,
        chat: Uuid,
        timestamp: i64,
        which: TurnsToDelete,
    ) -> Result<DeletedTurn> {
        let mut tx = self.pool.begin().await?;
        let raw: Option<String> = sqlx::query_scalar(
            "SELECT s.context FROM sessions s JOIN chat_members m ON m.chat_id = s.chat_id
             WHERE s.chat_id = $1 AND m.user_id = $2 FOR UPDATE OF s",
        )
        .bind(chat)
        .bind(user.as_ref())
        .fetch_optional(&mut *tx)
        .await?;
        let Some(raw) = raw else {
            tx.rollback().await?;
            return Ok(DeletedTurn::NotFound);
        };
        let busy: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM runs WHERE chat_id = $1 AND status = 'running')",
        )
        .bind(chat)
        .fetch_one(&mut *tx)
        .await?;
        if busy {
            tx.rollback().await?;
            return Ok(DeletedTurn::Busy);
        }
        // Times are stored to the microsecond and asked for to the millisecond.
        let opener: Option<i64> = sqlx::query_scalar(
            "SELECT id FROM messages WHERE chat_id = $1 AND turn_id = id
               AND sent_at >= $2 AND sent_at < $2 + interval '1 millisecond'
             ORDER BY id DESC LIMIT 1",
        )
        .bind(chat)
        .bind(DateTime::from_timestamp_millis(timestamp))
        .fetch_optional(&mut *tx)
        .await?;
        let Some(opener) = opener else {
            tx.rollback().await?;
            return Ok(DeletedTurn::NotFound);
        };
        // The context may no longer hold the turn when it was compacted; a
        // summary of it stays.
        let mut context: Conversation = serde_json::from_str(&raw)?;
        if remove_turn(&mut context, timestamp, which) {
            sqlx::query("UPDATE sessions SET context = $2 WHERE chat_id = $1")
                .bind(chat)
                .bind(serde_json::to_string(&context)?)
                .execute(&mut *tx)
                .await?;
        }
        let which_messages = match which {
            TurnsToDelete::One => "turn_id = $2",
            TurnsToDelete::Onward => "id >= $2",
        };
        let gone: Vec<i64> = sqlx::query_scalar(&format!(
            "SELECT id FROM messages WHERE chat_id = $1 AND ({which_messages}) ORDER BY id"
        ))
        .bind(chat)
        .bind(opener)
        .fetch_all(&mut *tx)
        .await?;
        let pictures: Vec<String> =
            sqlx::query_scalar("DELETE FROM files WHERE message_id = ANY($1) RETURNING id")
                .bind(&gone)
                .fetch_all(&mut *tx)
                .await?;
        sqlx::query("DELETE FROM messages WHERE id = ANY($1)")
            .bind(&gone)
            .execute(&mut *tx)
            .await?;
        let bumped =
            log_messages(&mut tx, chat, &gone, UpdateOp::Deleted, Readers::Members).await?;
        touch(&mut tx, chat).await?;
        tx.commit().await?;
        self.announce(&bumped);
        self.blobs.delete(&pictures).await?;
        Ok(DeletedTurn::Deleted)
    }

    pub(crate) async fn pin_session(
        &self,
        user: &UserKey,
        chat: Uuid,
        pinned: bool,
    ) -> Result<bool> {
        if !self.owns_session(user, chat).await? {
            return Ok(false);
        }
        self.set_member_flag(user, chat, "pinned", pinned).await
    }

    /// What the model sees for a session.
    pub(crate) async fn context(
        &self,
        user: &UserKey,
        chat: Uuid,
    ) -> Result<Option<StoredContext>> {
        self.context_of(user.as_ref(), chat).await
    }

    /// The context of a session the store picked for a sweep.
    pub async fn session_context(&self, session: &SessionRef) -> Result<Option<StoredContext>> {
        self.context_of(&session.user, session.id).await
    }

    async fn context_of(&self, user: &str, chat: Uuid) -> Result<Option<StoredContext>> {
        let raw: Option<String> = sqlx::query_scalar(
            "SELECT s.context FROM sessions s JOIN chat_members m ON m.chat_id = s.chat_id
             WHERE s.chat_id = $1 AND m.user_id = $2",
        )
        .bind(chat)
        .bind(user)
        .fetch_optional(&self.pool)
        .await?;
        raw.map(|raw| {
            let conversation = serde_json::from_str(&raw)?;
            Ok(StoredContext { raw, conversation })
        })
        .transpose()
    }

    /// Replaces the context with its compacted form unless changed since `before`; returns whether replaced.
    pub async fn save_compacted(
        &self,
        session: &SessionRef,
        before: &StoredContext,
        compacted: &Conversation,
    ) -> Result<bool> {
        let compacted = serde_json::to_string(compacted)?;
        let changed = sqlx::query(
            "UPDATE sessions SET context = $1, context_tokens = 0
             WHERE chat_id = $2 AND context = $3",
        )
        .bind(compacted)
        .bind(session.id)
        .bind(&before.raw)
        .execute(&self.pool)
        .await?
        .rows_affected();
        Ok(changed == 1)
    }

    /// Sessions whose last prompt reached `min_tokens`, idle for `idle`, with no run.
    pub async fn sessions_to_compact(
        &self,
        min_tokens: u32,
        idle: Duration,
    ) -> Result<Vec<QuietSession>> {
        let rows = sqlx::query(
            "SELECT m.user_id, c.id, s.context_tokens FROM sessions s
             JOIN chats c ON c.id = s.chat_id
             JOIN chat_members m ON m.chat_id = c.id
             WHERE s.context_tokens >= $1
               AND c.updated_at <= now() - make_interval(secs => $2)
               AND NOT EXISTS (SELECT 1 FROM runs r WHERE r.chat_id = c.id AND r.status = 'running')",
        )
        .bind(i64::from(min_tokens))
        .bind(idle.as_secs_f64())
        .fetch_all(&self.pool)
        .await?;
        rows.iter()
            .map(|row| {
                let tokens: i64 = row.try_get(2)?;
                Ok(QuietSession {
                    session: session_ref(row)?,
                    context_tokens: u32::try_from(tokens).unwrap_or(u32::MAX),
                })
            })
            .collect()
    }
}

#[cfg(test)]
#[path = "../tests/store_sessions.rs"]
mod tests;
