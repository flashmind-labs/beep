//! Chat sessions: the list, titles, transcripts and model contexts.

use super::{Store, seconds};
use crate::{protocol::Session, user::UserKey};
use anyhow::{Result, bail};
use chrono::{DateTime, Utc};
use flashmind::core::{Conversation, EntryKind};
use sqlx::{Row, postgres::PgRow};
use std::time::Duration;

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

/// A session `Store::search_sessions` found, with its transcript.
pub struct FoundSession {
    pub session: Session,
    pub conversation: Conversation,
}

/// One user's session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRef {
    pub user: String,
    pub id: String,
}

/// A session's model context with the stored text it was read from, so a
/// later write can check nothing changed it in between.
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

/// The times a removed turn spanned, in milliseconds since the epoch. `end`
/// is the next turn's user message, if there is one.
struct TurnSpan {
    start: i64,
    end: Option<i64>,
}

impl TurnSpan {
    fn contains(&self, millis: i64) -> bool {
        millis >= self.start && self.end.is_none_or(|end| millis < end)
    }
}

/// Removes the user message sent at `timestamp` and every entry up to the
/// next turn: the reply, its tool calls and their results, and the context
/// entries placed just before the message. With `TurnsToDelete::Onward` it
/// removes everything from there to the end.
fn remove_turn(
    conversation: &mut Conversation,
    timestamp: i64,
    which: TurnsToDelete,
) -> Option<TurnSpan> {
    let is_user = |kind: &EntryKind| matches!(kind, EntryKind::User { .. });
    // Each turn opens with developer context (time, place, apps). A
    // compaction summary is not part of any turn and stays.
    let is_context = |kind: &EntryKind| matches!(kind, EntryKind::Developer { tag, .. } if tag.as_deref() != Some("summary"));
    let entries = conversation.entries_mut();
    let asked = entries
        .iter()
        .position(|e| is_user(&e.kind) && e.timestamp.timestamp_millis() == timestamp)?;
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
    let span = TurnSpan {
        start: entries[start].timestamp.timestamp_millis(),
        end: entries.get(end).map(|e| e.timestamp.timestamp_millis()),
    };
    entries.drain(start..end);
    Some(span)
}

/// A session from the columns id, title, pinned and updated_at.
fn session(r: &PgRow) -> Result<Session> {
    Ok(Session {
        id: r.try_get(0)?,
        title: r.try_get(1)?,
        pinned: r.try_get(2)?,
        updated_at: r
            .try_get::<Option<DateTime<Utc>>, _>(3)?
            .map_or(0, |at| at.timestamp_millis()),
    })
}

/// `query` as it appears inside the stored JSON, with LIKE's wildcards
/// escaped, so a quote or a percent sign matches itself.
fn like_pattern(query: &str) -> String {
    let json = serde_json::to_string(query).unwrap_or_default();
    let inner = json
        .get(1..json.len().saturating_sub(1))
        .unwrap_or_default();
    let mut pattern = String::with_capacity(inner.len());
    for c in inner.chars() {
        if matches!(c, '\\' | '%' | '_') {
            pattern.push('\\');
        }
        pattern.push(c);
    }
    pattern
}

impl Store {
    pub(crate) async fn sessions(&self, user: &UserKey) -> Result<Vec<Session>> {
        let rows = sqlx::query(
            "SELECT s.id, COALESCE(t.title, (SELECT substr(message, 1, 80) FROM runs r
                 WHERE r.user_id=s.user_id AND r.session_id=s.id ORDER BY r.seq LIMIT 1), 'Conversation'),
                 s.pinned, s.updated_at
             FROM sessions s LEFT JOIN session_titles t ON t.user_id=s.user_id AND t.session_id=s.id
             WHERE s.user_id=$1 ORDER BY s.pinned DESC, s.seq DESC LIMIT 200",
        )
        .bind(user.as_str())
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(session).collect()
    }

    /// Up to `limit` sessions whose transcript holds `query` anywhere,
    /// ignoring case, most recently changed first. Tool calls and prompts
    /// count too, so the caller checks the messages.
    pub(crate) async fn search_sessions(
        &self,
        user: &UserKey,
        query: &str,
        limit: i64,
    ) -> Result<Vec<FoundSession>> {
        let rows = sqlx::query(
            "SELECT s.id, COALESCE(t.title, (SELECT substr(message, 1, 80) FROM runs r
                 WHERE r.user_id=s.user_id AND r.session_id=s.id ORDER BY r.seq LIMIT 1), 'Conversation'),
                 s.pinned, s.updated_at, s.conversation
             FROM sessions s LEFT JOIN session_titles t ON t.user_id=s.user_id AND t.session_id=s.id
             WHERE s.user_id=$1 AND s.conversation ILIKE $2 ESCAPE '\\'
             ORDER BY s.updated_at DESC NULLS LAST LIMIT $3",
        )
        .bind(user.as_str())
        .bind(format!("%{}%", like_pattern(query)))
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        rows.iter()
            .map(|r| {
                Ok(FoundSession {
                    session: session(r)?,
                    conversation: serde_json::from_str(&r.try_get::<String, _>(4)?)?,
                })
            })
            .collect()
    }

    pub(crate) async fn rename_session(
        &self,
        user: &UserKey,
        session_id: &str,
        title: &str,
    ) -> Result<bool> {
        if title.is_empty() || title.len() > 200 {
            bail!("session title must be 1-200 characters");
        }
        // A session copied from a share has no runs yet.
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM runs WHERE user_id=$1 AND session_id=$2)
                 OR EXISTS(SELECT 1 FROM sessions WHERE user_id=$1 AND id=$2)",
        )
        .bind(user.as_str())
        .bind(session_id)
        .fetch_one(&self.pool)
        .await?;
        if exists {
            // A title the user chose is never replaced by a generated one.
            sqlx::query(
                "INSERT INTO session_titles (user_id, session_id, title, manual, titled_at)
                 VALUES ($1, $2, $3, TRUE, now())
                 ON CONFLICT (user_id, session_id) DO UPDATE SET title=excluded.title,
                     manual=TRUE, titled_at=excluded.titled_at",
            )
            .bind(user.as_str())
            .bind(session_id)
            .bind(title)
            .execute(&self.pool)
            .await?;
        }
        Ok(exists)
    }

    /// Stores a generated title, unless the user named the session. Returns
    /// whether it was stored.
    pub async fn set_generated_title(&self, session: &SessionRef, title: &str) -> Result<bool> {
        let stored = sqlx::query(
            "INSERT INTO session_titles (user_id, session_id, title, titled_at)
             VALUES ($1, $2, $3, now())
             ON CONFLICT (user_id, session_id) DO UPDATE SET title=excluded.title,
                 titled_at=excluded.titled_at WHERE NOT session_titles.manual",
        )
        .bind(&session.user)
        .bind(&session.id)
        .bind(title)
        .execute(&self.pool)
        .await?
        .rows_affected();
        Ok(stored == 1)
    }

    /// Whether `session` has only its first run and no title yet.
    pub async fn needs_first_title(&self, session: &SessionRef) -> Result<bool> {
        Ok(sqlx::query_scalar(
            "SELECT (SELECT COUNT(*) FROM runs WHERE user_id=$1 AND session_id=$2) = 1
                AND NOT EXISTS(SELECT 1 FROM session_titles WHERE user_id=$1 AND session_id=$2)",
        )
        .bind(&session.user)
        .bind(&session.id)
        .fetch_one(&self.pool)
        .await?)
    }

    /// Sessions that changed since their title was written, have had no
    /// activity for `idle`, and have no run in progress. Sessions untouched
    /// for a day keep the title they have, so old chats are not all retitled
    /// at once.
    pub async fn sessions_to_title(&self, idle: Duration) -> Result<Vec<SessionRef>> {
        let rows = sqlx::query(
            "SELECT s.user_id, s.id FROM sessions s
             LEFT JOIN session_titles t ON t.user_id = s.user_id AND t.session_id = s.id
             WHERE s.updated_at <= now() - make_interval(secs => $1)
               AND s.updated_at >= now() - interval '1 day'
               AND (t.titled_at IS NULL OR t.titled_at < s.updated_at)
               AND NOT COALESCE(t.manual, FALSE)
               AND NOT EXISTS (SELECT 1 FROM runs r WHERE r.user_id = s.user_id
                   AND r.session_id = s.id AND r.status = 'running')
             LIMIT 20",
        )
        .bind(seconds(idle))
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(session_ref).collect()
    }

    /// The ones of `ids` that are the user's sessions.
    pub(crate) async fn existing_sessions(
        &self,
        user: &UserKey,
        ids: Vec<String>,
    ) -> Result<Vec<String>> {
        Ok(
            sqlx::query_scalar("SELECT id FROM sessions WHERE user_id=$1 AND id = ANY($2)")
                .bind(user.as_str())
                .bind(&ids)
                .fetch_all(&self.pool)
                .await?,
        )
    }

    pub(crate) async fn delete_session(&self, user: &UserKey, session_id: &str) -> Result<bool> {
        Ok(self
            .delete_sessions(user, vec![session_id.to_owned()])
            .await?
            > 0)
    }

    /// Deletes sessions with their runs and returns how many existed.
    pub(crate) async fn delete_sessions(&self, user: &UserKey, ids: Vec<String>) -> Result<u64> {
        let mut tx = self.pool.begin().await?;
        for table in [
            "runs",
            "tool_failures",
            "memory_scores",
            "turn_models",
            "session_titles",
            "shares",
        ] {
            sqlx::query(&format!(
                "DELETE FROM {table} WHERE user_id=$1 AND session_id = ANY($2)"
            ))
            .bind(user.as_str())
            .bind(&ids)
            .execute(&mut *tx)
            .await?;
        }
        let files: Vec<String> = sqlx::query_scalar(
            "DELETE FROM files WHERE user_id=$1 AND session_id = ANY($2) AND group_seq IS NULL
             RETURNING id",
        )
        .bind(user.as_str())
        .bind(&ids)
        .fetch_all(&mut *tx)
        .await?;
        let deleted = sqlx::query("DELETE FROM sessions WHERE user_id=$1 AND id = ANY($2)")
            .bind(user.as_str())
            .bind(&ids)
            .execute(&mut *tx)
            .await?
            .rows_affected();
        tx.commit().await?;
        self.blobs.delete(&files).await?;
        Ok(deleted)
    }

    /// Removes the turn that starts at the user message sent at `timestamp`,
    /// in milliseconds since the epoch, from the transcript and the context,
    /// and with `TurnsToDelete::Onward` every turn after it too.
    pub(crate) async fn delete_turn(
        &self,
        user: &UserKey,
        session_id: &str,
        timestamp: i64,
        which: TurnsToDelete,
    ) -> Result<DeletedTurn> {
        let mut tx = self.pool.begin().await?;
        let row = sqlx::query(
            "SELECT conversation, context FROM sessions WHERE user_id=$1 AND id=$2 FOR UPDATE",
        )
        .bind(user.as_str())
        .bind(session_id)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(row) = row else {
            return Ok(DeletedTurn::NotFound);
        };
        let busy: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM runs WHERE user_id=$1 AND session_id=$2 AND status='running')",
        )
        .bind(user.as_str())
        .bind(session_id)
        .fetch_one(&mut *tx)
        .await?;
        if busy {
            return Ok(DeletedTurn::Busy);
        }
        let mut transcript: Conversation = serde_json::from_str(row.try_get("conversation")?)?;
        let Some(turn) = remove_turn(&mut transcript, timestamp, which) else {
            return Ok(DeletedTurn::NotFound);
        };
        // A context that was never compacted is the transcript itself.
        let context: Option<String> = row.try_get("context")?;
        let context = match context {
            Some(raw) => {
                let mut context: Conversation = serde_json::from_str(&raw)?;
                // A summary may hold part of the turn; it stays.
                context.entries_mut().retain(|entry| {
                    entry.is_summary() || !turn.contains(entry.timestamp.timestamp_millis())
                });
                Some(serde_json::to_string(&context)?)
            }
            None => None,
        };
        sqlx::query(
            "UPDATE sessions SET conversation=$1, context=$2, updated_at=now()
             WHERE user_id=$3 AND id=$4",
        )
        .bind(serde_json::to_string(&transcript)?)
        .bind(context)
        .bind(user.as_str())
        .bind(session_id)
        .execute(&mut *tx)
        .await?;
        let at = match which {
            TurnsToDelete::One => "=",
            TurnsToDelete::Onward => ">=",
        };
        for table in ["memory_scores", "turn_models"] {
            sqlx::query(&format!(
                "DELETE FROM {table} WHERE user_id=$1 AND session_id=$2 AND turn_at{at}$3"
            ))
            .bind(user.as_str())
            .bind(session_id)
            .bind(timestamp)
            .execute(&mut *tx)
            .await?;
        }
        let pictures: Vec<String> = sqlx::query_scalar(&format!(
            "DELETE FROM files WHERE user_id=$1 AND session_id=$2 AND turn_at{at}$3 RETURNING id"
        ))
        .bind(user.as_str())
        .bind(session_id)
        .bind(timestamp)
        .fetch_all(&mut *tx)
        .await?;
        tx.commit().await?;
        self.blobs.delete(&pictures).await?;
        Ok(DeletedTurn::Deleted)
    }

    pub(crate) async fn pin_session(
        &self,
        user: &UserKey,
        session_id: &str,
        pinned: bool,
    ) -> Result<bool> {
        let changed = sqlx::query("UPDATE sessions SET pinned=$1 WHERE user_id=$2 AND id=$3")
            .bind(pinned)
            .bind(user.as_str())
            .bind(session_id)
            .execute(&self.pool)
            .await?
            .rows_affected();
        Ok(changed > 0)
    }

    pub(crate) async fn conversation(
        &self,
        user: &UserKey,
        session: &str,
    ) -> Result<Option<Conversation>> {
        self.conversation_of(user.as_str(), session).await
    }

    /// The conversation of a session the store picked for a sweep.
    pub async fn session_conversation(&self, session: &SessionRef) -> Result<Option<Conversation>> {
        self.conversation_of(&session.user, &session.id).await
    }

    async fn conversation_of(&self, user: &str, session: &str) -> Result<Option<Conversation>> {
        let raw: Option<String> =
            sqlx::query_scalar("SELECT conversation FROM sessions WHERE user_id=$1 AND id=$2")
                .bind(user)
                .bind(session)
                .fetch_optional(&self.pool)
                .await?;
        raw.map(|s| serde_json::from_str(&s).map_err(Into::into))
            .transpose()
    }

    /// What the model sees for a session: the compacted context, or the
    /// transcript when the session was never compacted.
    pub(crate) async fn context(
        &self,
        user: &UserKey,
        session: &str,
    ) -> Result<Option<StoredContext>> {
        self.context_of(user.as_str(), session).await
    }

    /// The context of a session the store picked for a sweep.
    pub async fn session_context(&self, session: &SessionRef) -> Result<Option<StoredContext>> {
        self.context_of(&session.user, &session.id).await
    }

    async fn context_of(&self, user: &str, session: &str) -> Result<Option<StoredContext>> {
        let raw: Option<String> = sqlx::query_scalar(
            "SELECT COALESCE(context, conversation) FROM sessions WHERE user_id=$1 AND id=$2",
        )
        .bind(user)
        .bind(session)
        .fetch_optional(&self.pool)
        .await?;
        raw.map(|raw| {
            let conversation = serde_json::from_str(&raw)?;
            Ok(StoredContext { raw, conversation })
        })
        .transpose()
    }

    /// Replaces the context with its compacted form, unless a run changed it
    /// since `before` was loaded. Returns whether it was replaced.
    pub async fn save_compacted(
        &self,
        session: &SessionRef,
        before: &StoredContext,
        compacted: &Conversation,
    ) -> Result<bool> {
        let compacted = serde_json::to_string(compacted)?;
        let changed = sqlx::query(
            "UPDATE sessions SET context=$1, context_tokens=0
             WHERE user_id=$2 AND id=$3 AND COALESCE(context, conversation)=$4",
        )
        .bind(compacted)
        .bind(&session.user)
        .bind(&session.id)
        .bind(&before.raw)
        .execute(&self.pool)
        .await?
        .rows_affected();
        Ok(changed == 1)
    }

    /// Sessions whose last prompt reached `min_tokens` and that have had no
    /// activity for `idle`, with no run in progress.
    pub async fn sessions_to_compact(
        &self,
        min_tokens: u32,
        idle: Duration,
    ) -> Result<Vec<SessionRef>> {
        let rows = sqlx::query(
            "SELECT user_id, id FROM sessions s
             WHERE context_tokens >= $1
               AND updated_at <= now() - make_interval(secs => $2)
               AND NOT EXISTS (SELECT 1 FROM runs r WHERE r.user_id = s.user_id
                   AND r.session_id = s.id AND r.status = 'running')",
        )
        .bind(i64::from(min_tokens))
        .bind(seconds(idle))
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(session_ref).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flashmind::core::ConversationEntry;

    fn turn(conversation: &mut Conversation, at: i64, prompt: &str) {
        let time = |ms| DateTime::from_timestamp_millis(ms).unwrap();
        let mut entries = vec![
            ConversationEntry::system_message("context"),
            ConversationEntry::user(prompt),
            ConversationEntry::assistant_with_tool_calls("", vec![], None),
            ConversationEntry::tool("call", "done"),
            ConversationEntry::assistant(format!("re: {prompt}")),
        ];
        for (i, entry) in entries.iter_mut().enumerate() {
            entry.timestamp = time(at + i as i64);
        }
        conversation.entries_mut().extend(entries);
    }

    #[test]
    fn remove_turn_takes_the_reply_and_tools_with_the_prompt() {
        let mut chat = Conversation::new();
        turn(&mut chat, 1000, "one");
        turn(&mut chat, 2000, "two");
        turn(&mut chat, 3000, "three");

        let span = remove_turn(&mut chat, 2001, TurnsToDelete::One).unwrap();
        assert_eq!((span.start, span.end), (2000, Some(3000)));
        let times: Vec<_> = chat
            .entries()
            .iter()
            .map(|e| e.timestamp.timestamp_millis())
            .collect();
        assert_eq!(
            times,
            [1000, 1001, 1002, 1003, 1004, 3000, 3001, 3002, 3003, 3004]
        );

        let span = remove_turn(&mut chat, 3001, TurnsToDelete::One).unwrap();
        assert_eq!((span.start, span.end), (3000, None));
        assert_eq!(chat.entries().len(), 5);
        assert!(remove_turn(&mut chat, 1002, TurnsToDelete::One).is_none());
    }

    #[test]
    fn remove_turn_onward_takes_every_later_turn() {
        let mut chat = Conversation::new();
        turn(&mut chat, 1000, "one");
        turn(&mut chat, 2000, "two");
        turn(&mut chat, 3000, "three");

        let span = remove_turn(&mut chat, 1001, TurnsToDelete::Onward).unwrap();
        assert_eq!((span.start, span.end), (1000, None));
        assert!(chat.entries().is_empty());
    }

    #[tokio::test]
    async fn search_finds_words_inside_the_stored_json() {
        use crate::store::{CreateRun, FinishedTurn, RunStatus, TestDb};
        use crate::user::User;

        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let alice = User::for_test(store.clone(), "alice");
        let CreateRun::Created(run) = alice.create_run("s1", "req-1", "hi").await.unwrap() else {
            panic!("run was not created");
        };
        let mut chat = Conversation::new();
        chat.add(ConversationEntry::user("Book the \"Kimpton\" for 100%"));
        let turn = FinishedTurn {
            transcript: &chat,
            context: &chat,
            prompt_tokens: 10,
        };
        alice
            .finish(&run, Some(turn), RunStatus::Completed)
            .await
            .unwrap();

        let ids = |found: Vec<FoundSession>| -> Vec<String> {
            found.into_iter().map(|f| f.session.id).collect()
        };
        assert_eq!(
            ids(alice.search_sessions("\"kimpton\"", 10).await.unwrap()),
            ["s1"]
        );
        assert_eq!(
            ids(alice.search_sessions("100%", 10).await.unwrap()),
            ["s1"]
        );
        assert!(alice.search_sessions("10_%", 10).await.unwrap().is_empty());
        let bob = User::for_test(store.clone(), "bob");
        assert!(bob.search_sessions("kimpton", 10).await.unwrap().is_empty());
    }
}
