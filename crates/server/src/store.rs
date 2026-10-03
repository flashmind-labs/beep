use crate::{
    mail::Mailbox,
    protocol::{Cron, Integration, Memory, MemorySource, PhoneNotification, Run, Session},
};
use anyhow::{Result, bail};
use chrono::{DateTime, NaiveDate, SecondsFormat, Utc};
use flashmind::core::Conversation;
use rusqlite::{OptionalExtension, params};
use serde_json::{Value, json};
use std::{collections::HashSet, time::Duration};
use tokio_rusqlite::Connection;
use uuid::Uuid;

#[derive(Clone)]
pub struct Store(Connection);

/// One row of `mcp_servers`.
pub struct McpServerRow {
    pub name: String,
    pub config: Value,
    pub credentials: Option<Value>,
}

pub enum CreateRun {
    Created(Run),
    Existing(Run),
    Conflict,
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

/// What a finished run saves for its session.
pub struct FinishedTurn<'a> {
    /// The full history, shown to the user.
    pub transcript: &'a Conversation,
    /// What the model sees next turn.
    pub context: &'a Conversation,
    /// Prompt size of the run's last model call.
    pub prompt_tokens: u32,
}

pub(crate) const MEMORY_CATEGORIES: &[&str] = &[
    "personal",
    "preference",
    "work",
    "location",
    "routine",
    "general",
];

/// The days a fact holds, like the dates of a trip. Both open by default.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Validity {
    pub from: Option<NaiveDate>,
    pub until: Option<NaiveDate>,
}

impl Validity {
    /// Whether the fact no longer holds on `today`.
    pub fn ended(&self, today: NaiveDate) -> bool {
        self.until.is_some_and(|until| until < today)
    }
}

/// A fact to store or to replace a memory's text with.
#[derive(Clone)]
pub struct Fact<'a> {
    pub content: &'a str,
    /// `None` keeps the current category, or `general` for a new memory.
    pub category: Option<&'a str>,
    pub source: MemorySource,
    /// `None` keeps the current days on an update, or leaves a new memory open.
    pub valid: Option<Validity>,
    /// Embedding of `content`, when one was computed.
    pub embedding: Option<Vec<f32>>,
}

/// A stored memory with its embedding, if it has one yet.
#[derive(Debug, Clone)]
pub struct MemoryRow {
    pub id: String,
    pub content: String,
    pub category: String,
    pub source: MemorySource,
    /// SQLite `datetime('now')` text.
    pub updated_at: String,
    pub valid: Validity,
    pub embedding: Option<Vec<f32>>,
}

impl MemoryRow {
    /// `YYYY-MM-DD` of the last change.
    pub fn updated(&self) -> &str {
        self.updated_at.get(..10).unwrap_or_default()
    }
}

/// An embedding computed for one version of a memory's text.
pub struct Embedded {
    pub id: String,
    pub content: String,
    pub embedding: Vec<f32>,
}

/// A short random id for a new memory; models copy it back into tool calls.
fn memory_id() -> String {
    Uuid::new_v4().simple().to_string()[..12].to_owned()
}

fn parse_date(text: Option<String>) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(text.as_deref()?, "%Y-%m-%d").ok()
}

fn format_date(date: Option<NaiveDate>) -> Option<String> {
    date.map(|d| d.format("%Y-%m-%d").to_string())
}

const MEMORY_COLUMNS: &str =
    "id, content, category, source, updated_at, valid_from, valid_until, embedding";

fn memory_row(r: &rusqlite::Row) -> rusqlite::Result<MemoryRow> {
    let source: String = r.get(3)?;
    let blob: Option<Vec<u8>> = r.get(7)?;
    Ok(MemoryRow {
        id: r.get(0)?,
        content: r.get(1)?,
        category: r.get(2)?,
        source: MemorySource::parse(&source),
        updated_at: r.get(4)?,
        valid: Validity {
            from: parse_date(r.get(5)?),
            until: parse_date(r.get(6)?),
        },
        embedding: blob.map(|b| decode_embedding(&b)),
    })
}

fn encode_embedding(embedding: &[f32]) -> Vec<u8> {
    embedding.iter().flat_map(|x| x.to_le_bytes()).collect()
}

fn decode_embedding(blob: &[u8]) -> Vec<f32> {
    blob.as_chunks::<4>()
        .0
        .iter()
        .map(|b| f32::from_le_bytes(*b))
        .collect()
}

fn check_memory(content: &str) -> Result<()> {
    if content.trim().is_empty() || content.len() > 4000 {
        bail!("memory content length out of range");
    }
    Ok(())
}

pub(crate) fn validate_category(category: Option<&str>) -> Result<String> {
    let cat = category.unwrap_or("general");
    if MEMORY_CATEGORIES.contains(&cat) {
        Ok(cat.to_owned())
    } else {
        bail!(
            "invalid category '{}', must be one of: {}",
            cat,
            MEMORY_CATEGORIES.join(", ")
        );
    }
}

impl Store {
    pub fn raw(&self) -> &Connection {
        &self.0
    }

    pub async fn open(path: impl AsRef<std::path::Path>) -> Result<Self> {
        let db = Connection::open(path).await?;
        db.call::<_, _, rusqlite::Error>(|db| {
            db.execute_batch(
                "PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;
                CREATE TABLE IF NOT EXISTS sessions (
                    user_id TEXT NOT NULL, id TEXT NOT NULL, conversation TEXT NOT NULL,
                    pinned INTEGER NOT NULL DEFAULT 0,
                    PRIMARY KEY(user_id, id));
                CREATE TABLE IF NOT EXISTS runs (
                    id TEXT PRIMARY KEY, user_id TEXT NOT NULL, session_id TEXT NOT NULL,
                    request_id TEXT NOT NULL, message TEXT NOT NULL, status TEXT NOT NULL,
                    UNIQUE(user_id, request_id));
                CREATE UNIQUE INDEX IF NOT EXISTS one_active_run
                    ON runs(user_id, session_id) WHERE status = 'running';
                DROP TABLE IF EXISTS events;
                CREATE TABLE IF NOT EXISTS memories (
                    user_id TEXT NOT NULL, id TEXT NOT NULL, content TEXT NOT NULL,
                    category TEXT NOT NULL DEFAULT 'general',
                    source TEXT NOT NULL DEFAULT 'user',
                    created_at TEXT NOT NULL DEFAULT (datetime('now')),
                    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
                    valid_from TEXT, valid_until TEXT, embedding BLOB,
                    PRIMARY KEY(user_id, id));
                CREATE TABLE IF NOT EXISTS memory_curation (
                    user_id TEXT PRIMARY KEY, curated_at TEXT NOT NULL);
                CREATE TABLE IF NOT EXISTS crons (
                    user_id TEXT NOT NULL, id TEXT NOT NULL, label TEXT NOT NULL,
                    prompt TEXT NOT NULL, schedule TEXT NOT NULL,
                    enabled INTEGER NOT NULL DEFAULT 1,
                    run_once INTEGER NOT NULL DEFAULT 0,
                    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                    PRIMARY KEY(user_id, id));
                CREATE TABLE IF NOT EXISTS cron_logs (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    user_id TEXT NOT NULL, cron_id TEXT NOT NULL,
                    run_id TEXT, started_at TEXT NOT NULL, finished_at TEXT,
                    status TEXT NOT NULL DEFAULT 'running',
                    error TEXT NOT NULL DEFAULT '');
                CREATE TABLE IF NOT EXISTS devices (
                    user_id TEXT NOT NULL, device_id TEXT NOT NULL, fcm_token TEXT NOT NULL,
                    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                    PRIMARY KEY(user_id, device_id));
                CREATE TABLE IF NOT EXISTS locations (
                    user_id TEXT PRIMARY KEY, latitude REAL NOT NULL, longitude REAL NOT NULL,
                    accuracy REAL NOT NULL, timezone TEXT NOT NULL DEFAULT 'UTC',
                    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')));
                CREATE TABLE IF NOT EXISTS mcp_connections (
                    user_id TEXT NOT NULL, id TEXT NOT NULL,
                    provider TEXT NOT NULL, label TEXT NOT NULL,
                    config TEXT NOT NULL DEFAULT '{}',
                    credentials TEXT NOT NULL DEFAULT '{}',
                    status TEXT NOT NULL DEFAULT 'disconnected',
                    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                    PRIMARY KEY(user_id, id));
                CREATE TABLE IF NOT EXISTS mcp_servers (
                    user_id TEXT NOT NULL, name TEXT NOT NULL,
                    config TEXT NOT NULL DEFAULT '{}',
                    credentials TEXT,
                    PRIMARY KEY(user_id, name));
                CREATE INDEX IF NOT EXISTS idx_cron_logs_user_cron
                    ON cron_logs(user_id, cron_id);
                CREATE INDEX IF NOT EXISTS idx_runs_user_session
                    ON runs(user_id, session_id);
                CREATE TABLE IF NOT EXISTS notifications (
                    id TEXT PRIMARY KEY,
                    user_id TEXT NOT NULL,
                    app TEXT NOT NULL,
                    title TEXT,
                    text TEXT,
                    timestamp INTEGER NOT NULL,
                    synced_at TEXT NOT NULL DEFAULT (datetime('now')),
                    UNIQUE(user_id, app, timestamp, title));
                CREATE INDEX IF NOT EXISTS idx_notifications_user_ts
                    ON notifications(user_id, timestamp DESC);
                CREATE TABLE IF NOT EXISTS habits (
                    id TEXT PRIMARY KEY,
                    user_id TEXT NOT NULL,
                    name TEXT NOT NULL,
                    frequency TEXT NOT NULL DEFAULT 'daily',
                    created_at TEXT NOT NULL DEFAULT (datetime('now')),
                    archived INTEGER NOT NULL DEFAULT 0,
                    UNIQUE(user_id, name));
                CREATE TABLE IF NOT EXISTS habit_logs (
                    id TEXT PRIMARY KEY,
                    habit_id TEXT NOT NULL REFERENCES habits(id),
                    user_id TEXT NOT NULL,
                    value REAL NOT NULL DEFAULT 1.0,
                    note TEXT,
                    logged_at TEXT NOT NULL DEFAULT (datetime('now')));
                CREATE INDEX IF NOT EXISTS idx_habit_logs_habit
                    ON habit_logs(habit_id, logged_at);
                CREATE TABLE IF NOT EXISTS session_titles (
                    user_id TEXT NOT NULL, session_id TEXT NOT NULL, title TEXT NOT NULL,
                    manual INTEGER NOT NULL DEFAULT 0, titled_at TEXT NOT NULL,
                    PRIMARY KEY(user_id, session_id));
                CREATE TABLE IF NOT EXISTS tool_failures (
                    user_id TEXT NOT NULL, session_id TEXT NOT NULL, call_id TEXT NOT NULL,
                    PRIMARY KEY(user_id, session_id, call_id));
                CREATE TABLE IF NOT EXISTS mail_sync (
                    user_id TEXT NOT NULL, mailbox TEXT NOT NULL, synced_at TEXT NOT NULL,
                    PRIMARY KEY(user_id, mailbox));
                CREATE TABLE IF NOT EXISTS mail_seen (
                    user_id TEXT NOT NULL, mailbox TEXT NOT NULL, message_id TEXT NOT NULL,
                    received_at TEXT NOT NULL,
                    PRIMARY KEY(user_id, mailbox, message_id));
                CREATE INDEX IF NOT EXISTS idx_mail_seen_received
                    ON mail_seen(user_id, mailbox, received_at);",
            )?;
            // Migrations for existing databases
            let has_col = |table: &str, col: &str| -> rusqlite::Result<bool> {
                let mut stmt = db.prepare(&format!("PRAGMA table_info({})", table))?;
                let found = stmt
                    .query_map([], |r| r.get::<_, String>(1))?
                    .any(|r| r.as_deref() == Ok(col));
                Ok(found)
            };
            if !has_col("crons", "run_once")? {
                db.execute_batch(
                    "ALTER TABLE crons ADD COLUMN run_once INTEGER NOT NULL DEFAULT 0;",
                )?;
            }
            if !has_col("locations", "timezone")? {
                db.execute_batch(
                    "ALTER TABLE locations ADD COLUMN timezone TEXT NOT NULL DEFAULT 'UTC';",
                )?;
            }
            if !has_col("crons", "last_run_at")? {
                db.execute_batch("ALTER TABLE crons ADD COLUMN last_run_at TEXT;")?;
            }
            if !has_col("crons", "last_status")? {
                db.execute_batch("ALTER TABLE crons ADD COLUMN last_status TEXT;")?;
            }
            if !has_col("crons", "last_error")? {
                db.execute_batch("ALTER TABLE crons ADD COLUMN last_error TEXT;")?;
            }
            if !has_col("memories", "category")? {
                db.execute_batch(
                    "ALTER TABLE memories ADD COLUMN category TEXT NOT NULL DEFAULT 'general';",
                )?;
            }
            if !has_col("memories", "updated_at")? {
                db.execute_batch(
                    "ALTER TABLE memories ADD COLUMN created_at TEXT;
                     ALTER TABLE memories ADD COLUMN updated_at TEXT;
                     ALTER TABLE memories ADD COLUMN source TEXT NOT NULL DEFAULT 'user';
                     ALTER TABLE memories ADD COLUMN embedding BLOB;
                     UPDATE memories SET created_at = datetime('now'), updated_at = datetime('now');",
                )?;
            }
            // Memories were keyed by a name like `dog_name`; they now get an id
            // and are told apart by meaning. A one-word value keeps its key as
            // the subject. Embeddings covered the key too, so they are redone.
            if has_col("memories", "key")? {
                db.execute_batch(
                    "BEGIN;
                     ALTER TABLE memories RENAME TO memories_by_key;
                     CREATE TABLE memories (
                         user_id TEXT NOT NULL, id TEXT NOT NULL, content TEXT NOT NULL,
                         category TEXT NOT NULL DEFAULT 'general',
                         source TEXT NOT NULL DEFAULT 'user',
                         created_at TEXT NOT NULL DEFAULT (datetime('now')),
                         updated_at TEXT NOT NULL DEFAULT (datetime('now')),
                         valid_from TEXT, valid_until TEXT, embedding BLOB,
                         PRIMARY KEY(user_id, id));
                     INSERT INTO memories (user_id, id, content, category, source, created_at, updated_at)
                         SELECT user_id, lower(hex(randomblob(6))),
                             CASE WHEN instr(trim(content), ' ') = 0
                                 THEN replace(key, '_', ' ') || ': ' || content ELSE content END,
                             category, CASE WHEN source = 'user' THEN 'user' ELSE 'learned' END,
                             coalesce(created_at, datetime('now')), coalesce(updated_at, datetime('now'))
                         FROM memories_by_key;
                     DROP TABLE memories_by_key;
                     COMMIT;",
                )?;
            }
            if !has_col("sessions", "pinned")? {
                db.execute_batch(
                    "ALTER TABLE sessions ADD COLUMN pinned INTEGER NOT NULL DEFAULT 0;",
                )?;
            }
            // `conversation` is the full transcript the phone shows; `context` is
            // what the model sees, compacted once the session is long and idle.
            if !has_col("sessions", "context")? {
                db.execute_batch(
                    "ALTER TABLE sessions ADD COLUMN context TEXT;
                     ALTER TABLE sessions ADD COLUMN context_tokens INTEGER NOT NULL DEFAULT 0;
                     ALTER TABLE sessions ADD COLUMN updated_at TEXT;",
                )?;
            }
            Ok(())
        })
        .await?;
        Ok(Self(db))
    }

    // Called once before accepting requests. A server restart never silently repeats tools.
    pub async fn recover(&self) -> Result<()> {
        self.0
            .call::<_, _, rusqlite::Error>(|db| {
                db.execute(
                    "UPDATE runs SET status='interrupted' WHERE status='running'",
                    [],
                )?;
                Ok(())
            })
            .await?;
        Ok(())
    }

    pub async fn create_run(
        &self,
        user: &str,
        session: &str,
        request: &str,
        message: &str,
    ) -> Result<CreateRun> {
        let (user, session, request, message) = (
            user.to_owned(),
            session.to_owned(),
            request.to_owned(),
            message.to_owned(),
        );
        Ok(self.0.call::<_, _, rusqlite::Error>(move |db| {
            let tx = db.transaction()?;
            let existing = tx.query_row(
                "SELECT id, session_id, status, message FROM runs WHERE user_id=? AND request_id=?",
                params![user, request], |r| Ok((Run { id:r.get(0)?, session_id:r.get(1)?, status:r.get(2)? }, r.get::<_,String>(3)?))
            ).optional()?;
            if let Some((run, original)) = existing {
                return Ok(if run.session_id == session && original == message { CreateRun::Existing(run) } else { CreateRun::Conflict });
            }
            let busy: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM runs WHERE user_id=? AND session_id=? AND status='running')", params![user, session], |r| r.get(0))?;
            if busy { return Ok(CreateRun::Conflict); }
            let active: i64 = tx.query_row("SELECT COUNT(*) FROM runs WHERE status='running'", [], |r| r.get(0))?;
            if active >= 16 { return Ok(CreateRun::Conflict); }
            let run = Run { id: Uuid::new_v4().to_string(), session_id: session, status: "running".into() };
            tx.execute("INSERT INTO runs VALUES (?, ?, ?, ?, ?, 'running')", params![run.id, user, run.session_id, request, message])?;
            tx.commit()?;
            Ok(CreateRun::Created(run))
        }).await?)
    }

    pub async fn run(&self, user: &str, id: &str) -> Result<Option<Run>> {
        let (user, id) = (user.to_owned(), id.to_owned());
        Ok(self
            .0
            .call::<_, _, rusqlite::Error>(move |db| {
                db.query_row(
                    "SELECT id, session_id, status FROM runs WHERE user_id=? AND id=?",
                    params![user, id],
                    |r| {
                        Ok(Run {
                            id: r.get(0)?,
                            session_id: r.get(1)?,
                            status: r.get(2)?,
                        })
                    },
                )
                .optional()
            })
            .await?)
    }

    pub async fn sessions(&self, user: &str) -> Result<Vec<Session>> {
        let user = user.to_owned();
        Ok(self.0.call::<_, _, rusqlite::Error>(move |db| {
            let mut statement = db.prepare(
                "SELECT s.id, COALESCE(t.title, (SELECT substr(message,1,80) FROM runs r WHERE r.user_id=s.user_id AND r.session_id=s.id ORDER BY r.rowid LIMIT 1), 'Conversation'), s.pinned
                 FROM sessions s LEFT JOIN session_titles t ON t.user_id=s.user_id AND t.session_id=s.id
                 WHERE s.user_id=? ORDER BY s.pinned DESC, s.rowid DESC LIMIT 200"
            )?;
            let rows = statement.query_map([user], |row| Ok(Session { id: row.get(0)?, title: row.get(1)?, pinned: row.get(2)? }))?.collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        }).await?)
    }

    pub async fn rename_session(&self, user: &str, session_id: &str, title: &str) -> Result<bool> {
        if title.is_empty() || title.len() > 200 {
            bail!("session title must be 1-200 characters");
        }
        let (user, session_id) = (user.to_owned(), session_id.to_owned());
        let title = title.to_owned();
        Ok(self
            .0
            .call::<_, _, rusqlite::Error>(move |db| {
                let exists: bool = db.query_row(
                    "SELECT EXISTS(SELECT 1 FROM runs WHERE user_id=? AND session_id=?)",
                    params![user, session_id],
                    |r| r.get(0),
                )?;
                if exists {
                    // A title the user chose is never replaced by a generated one.
                    db.execute(
                        "INSERT INTO session_titles (user_id, session_id, title, manual, titled_at)
                     VALUES (?, ?, ?, 1, strftime('%Y-%m-%dT%H:%M:%SZ','now'))
                     ON CONFLICT(user_id, session_id) DO UPDATE SET title=excluded.title,
                         manual=1, titled_at=excluded.titled_at",
                        params![user, session_id, title],
                    )?;
                }
                Ok(exists)
            })
            .await?)
    }

    /// Stores a generated title, unless the user named the session. Returns
    /// whether it was stored.
    pub async fn set_generated_title(&self, session: &SessionRef, title: &str) -> Result<bool> {
        let (user, id, title) = (session.user.clone(), session.id.clone(), title.to_owned());
        Ok(self
            .0
            .call::<_, _, rusqlite::Error>(move |db| {
                db.execute(
                    "INSERT INTO session_titles (user_id, session_id, title, titled_at)
                     VALUES (?, ?, ?, strftime('%Y-%m-%dT%H:%M:%SZ','now'))
                     ON CONFLICT(user_id, session_id) DO UPDATE SET title=excluded.title,
                         titled_at=excluded.titled_at WHERE manual=0",
                    params![user, id, title],
                )
            })
            .await?
            == 1)
    }

    /// Whether `session` has only its first run and no title yet.
    pub async fn needs_first_title(&self, session: &SessionRef) -> Result<bool> {
        let (user, id) = (session.user.clone(), session.id.clone());
        Ok(self
            .0
            .call::<_, _, rusqlite::Error>(move |db| {
                db.query_row(
                    "SELECT (SELECT COUNT(*) FROM runs WHERE user_id=?1 AND session_id=?2) = 1
                        AND NOT EXISTS(SELECT 1 FROM session_titles WHERE user_id=?1 AND session_id=?2)",
                    params![user, id],
                    |r| r.get(0),
                )
            })
            .await?)
    }

    /// Sessions that changed since their title was written, have had no
    /// activity for `idle`, and have no run in progress. Sessions untouched
    /// for a day keep the title they have, so old chats are not all retitled
    /// at once.
    pub async fn sessions_to_title(&self, idle: Duration) -> Result<Vec<SessionRef>> {
        let idle = format!("-{} seconds", idle.as_secs());
        Ok(self
            .0
            .call::<_, _, rusqlite::Error>(move |db| {
                let mut stmt = db.prepare(
                    "SELECT s.user_id, s.id FROM sessions s
                     LEFT JOIN session_titles t ON t.user_id = s.user_id AND t.session_id = s.id
                     WHERE s.updated_at <= strftime('%Y-%m-%dT%H:%M:%SZ', 'now', ?)
                       AND s.updated_at >= strftime('%Y-%m-%dT%H:%M:%SZ', 'now', '-1 day')
                       AND (t.titled_at IS NULL OR t.titled_at < s.updated_at)
                       AND COALESCE(t.manual, 0) = 0
                       AND NOT EXISTS (SELECT 1 FROM runs r WHERE r.user_id = s.user_id
                           AND r.session_id = s.id AND r.status = 'running')
                     LIMIT 20",
                )?;
                stmt.query_map(params![idle], |r| {
                    Ok(SessionRef {
                        user: r.get(0)?,
                        id: r.get(1)?,
                    })
                })?
                .collect()
            })
            .await?)
    }

    pub async fn delete_session(&self, user: &str, session_id: &str) -> Result<bool> {
        Ok(self
            .delete_sessions(user, vec![session_id.to_owned()])
            .await?
            > 0)
    }

    /// Deletes sessions with their runs and returns how many existed.
    pub async fn delete_sessions(&self, user: &str, ids: Vec<String>) -> Result<u64> {
        let user = user.to_owned();
        Ok(self
            .0
            .call::<_, _, rusqlite::Error>(move |db| {
                let tx = db.transaction()?;
                let mut deleted = 0;
                for id in &ids {
                    for table in ["runs", "tool_failures", "session_titles"] {
                        tx.execute(
                            &format!("DELETE FROM {table} WHERE user_id=? AND session_id=?"),
                            params![user, id],
                        )?;
                    }
                    deleted += tx.execute(
                        "DELETE FROM sessions WHERE user_id=? AND id=?",
                        params![user, id],
                    )? as u64;
                }
                tx.commit()?;
                Ok(deleted)
            })
            .await?)
    }

    pub async fn pin_session(&self, user: &str, session_id: &str, pinned: bool) -> Result<bool> {
        let (user, session_id) = (user.to_owned(), session_id.to_owned());
        Ok(self
            .0
            .call::<_, _, rusqlite::Error>(move |db| {
                let changed = db.execute(
                    "UPDATE sessions SET pinned=? WHERE user_id=? AND id=?",
                    params![pinned, user, session_id],
                )?;
                Ok(changed > 0)
            })
            .await?)
    }

    pub async fn conversation(&self, user: &str, session: &str) -> Result<Option<Conversation>> {
        let (user, session) = (user.to_owned(), session.to_owned());
        let raw: Option<String> = self
            .0
            .call::<_, _, rusqlite::Error>(move |db| {
                db.query_row(
                    "SELECT conversation FROM sessions WHERE user_id=? AND id=?",
                    params![user, session],
                    |r| r.get(0),
                )
                .optional()
            })
            .await?;
        raw.map(|s| serde_json::from_str(&s).map_err(Into::into))
            .transpose()
    }

    /// What the model sees for a session: the compacted context, or the
    /// transcript when the session was never compacted.
    pub async fn context(&self, user: &str, session: &str) -> Result<Option<StoredContext>> {
        let (user, session) = (user.to_owned(), session.to_owned());
        let raw: Option<String> = self
            .0
            .call::<_, _, rusqlite::Error>(move |db| {
                db.query_row(
                    "SELECT COALESCE(context, conversation) FROM sessions WHERE user_id=? AND id=?",
                    params![user, session],
                    |r| r.get(0),
                )
                .optional()
            })
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
        let (user, id) = (session.user.clone(), session.id.clone());
        let before = before.raw.clone();
        let compacted = serde_json::to_string(compacted)?;
        let changed = self
            .0
            .call::<_, _, rusqlite::Error>(move |db| {
                db.execute(
                    "UPDATE sessions SET context=?, context_tokens=0
                     WHERE user_id=? AND id=? AND COALESCE(context, conversation)=?",
                    params![compacted, user, id, before],
                )
            })
            .await?;
        Ok(changed == 1)
    }

    /// Sessions whose last prompt reached `min_tokens` and that have had no
    /// activity for `idle`, with no run in progress.
    pub async fn sessions_to_compact(
        &self,
        min_tokens: u32,
        idle: Duration,
    ) -> Result<Vec<SessionRef>> {
        let idle = format!("-{} seconds", idle.as_secs());
        Ok(self
            .0
            .call::<_, _, rusqlite::Error>(move |db| {
                let mut stmt = db.prepare(
                    "SELECT user_id, id FROM sessions s
                     WHERE context_tokens >= ?
                       AND updated_at <= strftime('%Y-%m-%dT%H:%M:%SZ', 'now', ?)
                       AND NOT EXISTS (SELECT 1 FROM runs r WHERE r.user_id = s.user_id
                           AND r.session_id = s.id AND r.status = 'running')",
                )?;
                stmt.query_map(params![min_tokens, idle], |r| {
                    Ok(SessionRef {
                        user: r.get(0)?,
                        id: r.get(1)?,
                    })
                })?
                .collect()
            })
            .await?)
    }

    pub async fn finish(
        &self,
        user: &str,
        run: &Run,
        turn: Option<FinishedTurn<'_>>,
        status: &str,
    ) -> Result<()> {
        let (user, run, status) = (user.to_owned(), run.clone(), status.to_owned());
        let turn = turn
            .map(|turn| {
                anyhow::Ok((
                    serde_json::to_string(turn.transcript)?,
                    serde_json::to_string(turn.context)?,
                    turn.prompt_tokens,
                ))
            })
            .transpose()?;
        self.0.call::<_, _, rusqlite::Error>(move |db| {
            let tx = db.transaction()?;
            if let Some((transcript, context, tokens)) = turn {
                tx.execute(
                    "INSERT INTO sessions (user_id, id, conversation, context, context_tokens, updated_at)
                     VALUES (?, ?, ?, ?, ?, strftime('%Y-%m-%dT%H:%M:%SZ','now'))
                     ON CONFLICT(user_id,id) DO UPDATE SET conversation=excluded.conversation,
                         context=excluded.context, context_tokens=excluded.context_tokens,
                         updated_at=excluded.updated_at",
                    params![user, run.session_id, transcript, context, tokens],
                )?;
            }
            tx.execute("UPDATE runs SET status=? WHERE id=? AND user_id=?", params![status,run.id,user])?;
            tx.commit()?;
            Ok(())
        }).await?;
        Ok(())
    }

    /// Records tool calls that failed, since the conversation keeps only their output.
    pub async fn record_tool_failures(
        &self,
        user: &str,
        session: &str,
        call_ids: Vec<String>,
    ) -> Result<()> {
        if call_ids.is_empty() {
            return Ok(());
        }
        let (user, session) = (user.to_owned(), session.to_owned());
        self.0
            .call::<_, _, rusqlite::Error>(move |db| {
                let tx = db.transaction()?;
                for id in call_ids {
                    tx.execute(
                        "INSERT OR IGNORE INTO tool_failures VALUES (?, ?, ?)",
                        params![user, session, id],
                    )?;
                }
                tx.commit()
            })
            .await?;
        Ok(())
    }

    /// Ids of the session's tool calls that failed.
    pub async fn tool_failures(&self, user: &str, session: &str) -> Result<HashSet<String>> {
        let (user, session) = (user.to_owned(), session.to_owned());
        Ok(self
            .0
            .call::<_, _, rusqlite::Error>(move |db| {
                let mut stmt = db.prepare(
                    "SELECT call_id FROM tool_failures WHERE user_id=? AND session_id=?",
                )?;
                stmt.query_map(params![user, session], |r| r.get(0))?
                    .collect()
            })
            .await?)
    }

    /// Stores a new memory and returns its id.
    pub async fn add_memory(&self, user: &str, fact: Fact<'_>) -> Result<String> {
        check_memory(fact.content)?;
        let category = validate_category(fact.category)?;
        let (user, id, content) = (user.to_owned(), memory_id(), fact.content.to_owned());
        let source = fact.source.as_str();
        let valid = fact.valid.unwrap_or_default();
        let (from, until) = (format_date(valid.from), format_date(valid.until));
        let embedding = fact.embedding.as_deref().map(encode_embedding);
        let stored = id.clone();
        self.0
            .call::<_, _, rusqlite::Error>(move |db| {
                db.execute(
                    "INSERT INTO memories (user_id, id, content, category, source, valid_from, \
                     valid_until, embedding) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
                    params![
                        user, stored, content, category, source, from, until, embedding
                    ],
                )
            })
            .await?;
        Ok(id)
    }

    /// Replaces a memory's text, and its days when given. A memory the user
    /// stated stays stated. Returns whether it exists.
    pub async fn update_memory(&self, user: &str, id: &str, fact: Fact<'_>) -> Result<bool> {
        check_memory(fact.content)?;
        let category = fact
            .category
            .map(|c| validate_category(Some(c)))
            .transpose()?;
        let (user, id, content) = (user.to_owned(), id.to_owned(), fact.content.to_owned());
        let source = fact.source.as_str();
        let keep_valid = fact.valid.is_none();
        let valid = fact.valid.unwrap_or_default();
        let (from, until) = (format_date(valid.from), format_date(valid.until));
        let embedding = fact.embedding.as_deref().map(encode_embedding);
        Ok(self
            .0
            .call::<_, _, rusqlite::Error>(move |db| {
                db.execute(
                    "UPDATE memories SET content=?1, category=coalesce(?2, category), \
                     source=CASE WHEN source='user' THEN 'user' ELSE ?3 END, \
                     valid_from=CASE WHEN ?9 THEN valid_from ELSE ?4 END, \
                     valid_until=CASE WHEN ?9 THEN valid_until ELSE ?5 END, \
                     updated_at=datetime('now'), \
                     embedding=CASE WHEN ?6 IS NOT NULL THEN ?6 \
                         WHEN content=?1 THEN embedding END \
                     WHERE user_id=?7 AND id=?8",
                    params![
                        content, category, source, from, until, embedding, user, id, keep_valid
                    ],
                )
                .map(|changed| changed > 0)
            })
            .await?)
    }

    /// Replaces only a memory's text, as the user does from the Memories
    /// screen; it then counts as stated by the user.
    pub async fn edit_memory(&self, user: &str, id: &str, content: &str) -> Result<bool> {
        check_memory(content)?;
        let (user, id, content) = (user.to_owned(), id.to_owned(), content.to_owned());
        Ok(self
            .0
            .call::<_, _, rusqlite::Error>(move |db| {
                db.execute(
                    "UPDATE memories SET content=?1, source='user', updated_at=datetime('now'), \
                     embedding=CASE WHEN content=?1 THEN embedding END \
                     WHERE user_id=?2 AND id=?3",
                    params![content, user, id],
                )
                .map(|changed| changed > 0)
            })
            .await?)
    }

    /// Every memory of a user with its embedding, newest first.
    pub async fn memory_rows(&self, user: &str) -> Result<Vec<MemoryRow>> {
        let user = user.to_owned();
        Ok(self
            .0
            .call::<_, _, rusqlite::Error>(move |db| {
                let mut stmt = db.prepare(&format!(
                    "SELECT {MEMORY_COLUMNS} FROM memories WHERE user_id=? \
                     ORDER BY updated_at DESC LIMIT 2000"
                ))?;
                stmt.query_map([user], memory_row)?.collect()
            })
            .await?)
    }

    /// Saves embeddings, skipping any memory whose text changed since it was embedded.
    pub async fn set_embeddings(&self, user: &str, embedded: Vec<Embedded>) -> Result<()> {
        let user = user.to_owned();
        self.0
            .call::<_, _, rusqlite::Error>(move |db| {
                let tx = db.transaction()?;
                for Embedded {
                    id,
                    content,
                    embedding,
                } in embedded
                {
                    tx.execute(
                        "UPDATE memories SET embedding=? WHERE user_id=? AND id=? AND content=?",
                        params![encode_embedding(&embedding), user, id, content],
                    )?;
                }
                tx.commit()
            })
            .await?;
        Ok(())
    }

    pub async fn forget(&self, user: &str, id: &str) -> Result<bool> {
        let (user, id) = (user.to_owned(), id.to_owned());
        Ok(self
            .0
            .call::<_, _, rusqlite::Error>(move |db| {
                Ok(db.execute(
                    "DELETE FROM memories WHERE user_id=? AND id=?",
                    params![user, id],
                )? > 0)
            })
            .await?)
    }

    /// Deletes every memory of a user and returns how many there were.
    pub async fn forget_all(&self, user: &str) -> Result<u64> {
        let user = user.to_owned();
        Ok(self
            .0
            .call::<_, _, rusqlite::Error>(move |db| {
                db.execute("DELETE FROM memories WHERE user_id=?", [user])
                    .map(|n| n as u64)
            })
            .await?)
    }

    /// Every memory for the Memories screen, newest first.
    pub async fn list_memories(&self, user: &str) -> Result<Vec<Memory>> {
        Ok(self
            .memory_rows(user)
            .await?
            .into_iter()
            .map(|row| Memory {
                updated: row.updated().to_owned(),
                id: row.id,
                content: row.content,
                category: row.category,
                source: row.source,
                valid_from: format_date(row.valid.from),
                valid_until: format_date(row.valid.until),
            })
            .collect())
    }

    /// Deletes memories whose time ended more than `keep_days` ago and returns
    /// how many.
    pub async fn purge_ended_memories(&self, keep_days: u32) -> Result<u64> {
        let cutoff = format!("-{keep_days} days");
        Ok(self
            .0
            .call::<_, _, rusqlite::Error>(move |db| {
                db.execute(
                    "DELETE FROM memories WHERE valid_until < date('now', ?)",
                    [cutoff],
                )
                .map(|n| n as u64)
            })
            .await?)
    }

    /// Users with memories changed since their last curation, which was at
    /// least `every` ago.
    pub async fn users_to_curate(&self, every: Duration) -> Result<Vec<String>> {
        let every = format!("-{} seconds", every.as_secs());
        Ok(self
            .0
            .call::<_, _, rusqlite::Error>(move |db| {
                let mut stmt = db.prepare(
                    "SELECT m.user_id FROM memories m
                     LEFT JOIN memory_curation c ON c.user_id = m.user_id
                     GROUP BY m.user_id, c.curated_at
                     HAVING c.curated_at IS NULL
                         OR (c.curated_at <= datetime('now', ?) AND max(m.updated_at) > c.curated_at)",
                )?;
                stmt.query_map([every], |r| r.get(0))?.collect()
            })
            .await?)
    }

    pub async fn mark_curated(&self, user: &str) -> Result<()> {
        let user = user.to_owned();
        self.0
            .call::<_, _, rusqlite::Error>(move |db| {
                db.execute(
                    "INSERT INTO memory_curation (user_id, curated_at) VALUES (?, datetime('now'))
                     ON CONFLICT(user_id) DO UPDATE SET curated_at=excluded.curated_at",
                    [user],
                )
            })
            .await?;
        Ok(())
    }

    pub async fn list_crons(&self, user: &str) -> Result<Vec<Cron>> {
        let user = user.to_owned();
        Ok(self
            .0
            .call::<_, _, rusqlite::Error>(move |db| {
                let mut stmt = db.prepare(
                    "SELECT id, label, prompt, schedule, enabled, created_at, run_once, last_run_at, last_status, last_error FROM crons WHERE user_id=? ORDER BY created_at DESC LIMIT 100",
                )?;
                let rows = stmt
                    .query_map([user], |r| {
                        Ok(Cron {
                            id: r.get(0)?,
                            label: r.get(1)?,
                            prompt: r.get(2)?,
                            schedule: r.get(3)?,
                            enabled: r.get(4)?,
                            created_at: r.get(5)?,
                            run_once: r.get(6)?,
                            last_run_at: r.get(7)?,
                            last_status: r.get(8)?,
                            last_error: r.get(9)?,
                            next_run_at: None,
                        })
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows)
            })
            .await?)
    }

    pub async fn create_cron(
        &self,
        user: &str,
        label: &str,
        schedule: &str,
        prompt: &str,
    ) -> Result<String> {
        self.create_cron_ex(user, label, schedule, prompt, false)
            .await
    }

    pub async fn create_cron_ex(
        &self,
        user: &str,
        label: &str,
        schedule: &str,
        prompt: &str,
        run_once: bool,
    ) -> Result<String> {
        if label.is_empty() || label.len() > 200 {
            bail!("cron label must be 1–200 characters");
        }
        if prompt.is_empty() || prompt.len() > 4000 {
            bail!("cron prompt must be 1–4000 characters");
        }
        if schedule.is_empty() || schedule.len() > 100 {
            bail!("cron schedule must be 1–100 characters");
        }
        let id = Uuid::new_v4().to_string();
        let (user, id_clone, label, prompt, schedule) = (
            user.to_owned(),
            id.clone(),
            label.to_owned(),
            prompt.to_owned(),
            schedule.to_owned(),
        );
        self.0
            .call::<_, _, rusqlite::Error>(move |db| {
                db.execute(
                    "INSERT INTO crons (user_id, id, label, prompt, schedule, run_once) VALUES (?, ?, ?, ?, ?, ?)",
                    params![user, id_clone, label, prompt, schedule, run_once],
                )?;
                Ok(())
            })
            .await?;
        Ok(id)
    }

    pub async fn update_cron(
        &self,
        user: &str,
        id: &str,
        enabled: Option<bool>,
        label: Option<&str>,
        schedule: Option<&str>,
    ) -> Result<bool> {
        let (user, id) = (user.to_owned(), id.to_owned());
        let set_enabled = enabled;
        let label = label.map(|s| s.to_owned());
        let schedule = schedule.map(|s| s.to_owned());
        Ok(self
            .0
            .call::<_, _, rusqlite::Error>(move |db| {
                let tx = db.transaction()?;
                let mut changed = false;
                if let Some(enabled) = set_enabled {
                    changed |= tx.execute(
                        "UPDATE crons SET enabled=? WHERE user_id=? AND id=?",
                        params![enabled, user, id],
                    )? > 0;
                }
                if let Some(label) = &label {
                    changed |= tx.execute(
                        "UPDATE crons SET label=? WHERE user_id=? AND id=?",
                        params![label, user, id],
                    )? > 0;
                }
                if let Some(schedule) = &schedule {
                    changed |= tx.execute(
                        "UPDATE crons SET schedule=? WHERE user_id=? AND id=?",
                        params![schedule, user, id],
                    )? > 0;
                }
                tx.commit()?;
                Ok(changed)
            })
            .await?)
    }

    pub async fn toggle_cron(&self, user: &str, id: &str) -> Result<Option<bool>> {
        let (user, id) = (user.to_owned(), id.to_owned());
        Ok(self
            .0
            .call::<_, _, rusqlite::Error>(move |db| {
                let new_enabled: Option<bool> = db
                    .query_row(
                        "UPDATE crons SET enabled = 1 - enabled WHERE user_id=? AND id=? RETURNING enabled",
                        params![user, id],
                        |r| r.get(0),
                    )
                    .optional()?;
                Ok(new_enabled)
            })
            .await?)
    }

    pub async fn delete_cron(&self, user: &str, id: &str) -> Result<bool> {
        let (user, id) = (user.to_owned(), id.to_owned());
        Ok(self
            .0
            .call::<_, _, rusqlite::Error>(move |db| {
                Ok(db.execute(
                    "DELETE FROM crons WHERE user_id=? AND id=?",
                    params![user, id],
                )? > 0)
            })
            .await?)
    }

    pub async fn register_device(
        &self,
        user: &str,
        device_id: &str,
        fcm_token: &str,
    ) -> Result<()> {
        let (user, device_id, fcm_token) =
            (user.to_owned(), device_id.to_owned(), fcm_token.to_owned());
        self.0
            .call::<_, _, rusqlite::Error>(move |db| {
                db.execute(
                    "INSERT INTO devices (user_id, device_id, fcm_token) VALUES (?, ?, ?) ON CONFLICT(user_id, device_id) DO UPDATE SET fcm_token=excluded.fcm_token, created_at=strftime('%Y-%m-%dT%H:%M:%SZ','now')",
                    params![user, device_id, fcm_token],
                )?;
                Ok(())
            })
            .await?;
        Ok(())
    }

    pub async fn unregister_device(&self, user: &str, device_id: &str) -> Result<bool> {
        let (user, device_id) = (user.to_owned(), device_id.to_owned());
        Ok(self
            .0
            .call::<_, _, rusqlite::Error>(move |db| {
                Ok(db.execute(
                    "DELETE FROM devices WHERE user_id=? AND device_id=?",
                    params![user, device_id],
                )? > 0)
            })
            .await?)
    }

    pub async fn due_crons(&self) -> Result<Vec<(String, Value)>> {
        Ok(self
            .0
            .call::<_, _, rusqlite::Error>(|db| {
                let mut stmt = db.prepare(
                    "SELECT user_id, id, label, prompt, schedule, run_once FROM crons WHERE enabled=1",
                )?;
                let rows = stmt
                    .query_map([], |r| {
                        Ok((
                            r.get::<_, String>(0)?,
                            json!({
                                "id": r.get::<_, String>(1)?,
                                "label": r.get::<_, String>(2)?,
                                "prompt": r.get::<_, String>(3)?,
                                "schedule": r.get::<_, String>(4)?,
                                "run_once": r.get::<_, bool>(5)?,
                            }),
                        ))
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows)
            })
            .await?)
    }

    pub async fn log_cron_start(&self, user: &str, cron_id: &str, run_id: &str) -> Result<()> {
        let (user, cron_id, run_id) = (user.to_owned(), cron_id.to_owned(), run_id.to_owned());
        self.0
            .call::<_, _, rusqlite::Error>(move |db| {
                db.execute(
                    "INSERT INTO cron_logs (user_id, cron_id, run_id, started_at, status) VALUES (?, ?, ?, datetime('now'), 'running')",
                    params![user, cron_id, run_id],
                )?;
                Ok(())
            })
            .await?;
        Ok(())
    }

    pub async fn log_cron_finish(&self, run_id: &str, status: &str, error: &str) -> Result<()> {
        let (run_id, status, error) = (run_id.to_owned(), status.to_owned(), error.to_owned());
        self.0
            .call::<_, _, rusqlite::Error>(move |db| {
                let tx = db.transaction()?;
                tx.execute(
                    "UPDATE cron_logs SET finished_at=datetime('now'), status=?, error=? WHERE run_id=?",
                    params![status, error, run_id],
                )?;
                // Denormalize last run info onto the crons row.
                tx.execute(
                    "UPDATE crons SET last_run_at=strftime('%Y-%m-%dT%H:%M:%SZ','now'), last_status=?, last_error=NULLIF(?,'') \
                     WHERE (user_id, id) IN (SELECT user_id, cron_id FROM cron_logs WHERE run_id=?)",
                    params![status, error, run_id],
                )?;
                tx.commit()?;
                Ok(())
            })
            .await?;
        Ok(())
    }

    pub async fn cron_last_run(&self, user: &str, cron_id: &str) -> Result<Option<String>> {
        let (user, cron_id) = (user.to_owned(), cron_id.to_owned());
        Ok(self
            .0
            .call::<_, _, rusqlite::Error>(move |db| {
                db.query_row(
                    "SELECT started_at FROM cron_logs WHERE user_id=? AND cron_id=? ORDER BY id DESC LIMIT 1",
                    params![user, cron_id],
                    |r| r.get(0),
                )
                .optional()
            })
            .await?)
    }

    pub async fn device_tokens(&self, user: &str) -> Result<Vec<String>> {
        let user = user.to_owned();
        Ok(self
            .0
            .call::<_, _, rusqlite::Error>(move |db| {
                let mut stmt = db.prepare("SELECT fcm_token FROM devices WHERE user_id=?")?;
                let rows = stmt
                    .query_map([user], |r| r.get::<_, String>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows)
            })
            .await?)
    }

    pub async fn list_mcp_connections(&self, user: &str) -> Result<Vec<Integration>> {
        let user = user.to_owned();
        Ok(self
            .0
            .call::<_, _, rusqlite::Error>(move |db| {
                let mut stmt = db.prepare(
                    "SELECT id, provider, label, status, created_at FROM mcp_connections WHERE user_id=? ORDER BY created_at DESC LIMIT 100",
                )?;
                let rows = stmt
                    .query_map([user], |r| {
                        Ok(Integration {
                            id: r.get(0)?,
                            provider: r.get(1)?,
                            label: r.get(2)?,
                            status: r.get(3)?,
                            created_at: r.get(4)?,
                        })
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows)
            })
            .await?)
    }

    #[allow(dead_code)]
    pub async fn get_mcp_connection(&self, user: &str, id: &str) -> Result<Option<Value>> {
        let (user, id) = (user.to_owned(), id.to_owned());
        Ok(self
            .0
            .call::<_, _, rusqlite::Error>(move |db| {
                db.query_row(
                    "SELECT id, provider, label, config, credentials, status FROM mcp_connections WHERE user_id=? AND id=?",
                    params![user, id],
                    |r| {
                        Ok(json!({
                            "id": r.get::<_,String>(0)?,
                            "provider": r.get::<_,String>(1)?,
                            "label": r.get::<_,String>(2)?,
                            "config": r.get::<_,String>(3)?,
                            "credentials": r.get::<_,String>(4)?,
                            "status": r.get::<_,String>(5)?,
                        }))
                    },
                )
                .optional()
            })
            .await?)
    }

    pub async fn create_mcp_connection(
        &self,
        user: &str,
        provider: &str,
        label: &str,
        config: &str,
    ) -> Result<String> {
        if provider.is_empty() || provider.len() > 100 {
            bail!("provider must be 1–100 characters");
        }
        if label.is_empty() || label.len() > 200 {
            bail!("label must be 1–200 characters");
        }
        if serde_json::from_str::<Value>(config).is_err() {
            bail!("config must be valid JSON");
        }
        let id = Uuid::new_v4().to_string();
        let (user, id_clone, provider, label, config) = (
            user.to_owned(),
            id.clone(),
            provider.to_owned(),
            label.to_owned(),
            config.to_owned(),
        );
        self.0
            .call::<_, _, rusqlite::Error>(move |db| {
                db.execute(
                    "INSERT INTO mcp_connections (user_id, id, provider, label, config) VALUES (?, ?, ?, ?, ?)",
                    params![user, id_clone, provider, label, config],
                )?;
                Ok(())
            })
            .await?;
        Ok(id)
    }

    pub async fn update_mcp_credentials(
        &self,
        user: &str,
        id: &str,
        credentials: &str,
        status: &str,
    ) -> Result<bool> {
        let (user, id, credentials, status) = (
            user.to_owned(),
            id.to_owned(),
            credentials.to_owned(),
            status.to_owned(),
        );
        Ok(self
            .0
            .call::<_, _, rusqlite::Error>(move |db| {
                Ok(db.execute(
                    "UPDATE mcp_connections SET credentials=?, status=? WHERE user_id=? AND id=?",
                    params![credentials, status, user, id],
                )? > 0)
            })
            .await?)
    }

    pub async fn delete_mcp_connection(&self, user: &str, id: &str) -> Result<bool> {
        let (user, id) = (user.to_owned(), id.to_owned());
        Ok(self
            .0
            .call::<_, _, rusqlite::Error>(move |db| {
                Ok(db.execute(
                    "DELETE FROM mcp_connections WHERE user_id=? AND id=?",
                    params![user, id],
                )? > 0)
            })
            .await?)
    }

    /// The user's server-managed MCP servers.
    pub async fn mcp_servers(&self, user: &str) -> Result<Vec<McpServerRow>> {
        let user = user.to_owned();
        Ok(self
            .0
            .call::<_, _, rusqlite::Error>(move |db| {
                let mut stmt = db.prepare(
                    "SELECT name, config, credentials FROM mcp_servers WHERE user_id=? ORDER BY name",
                )?;
                let rows = stmt
                    .query_map([user], |r| {
                        Ok(McpServerRow {
                            name: r.get(0)?,
                            config: r.get(1)?,
                            credentials: r.get(2)?,
                        })
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows)
            })
            .await?)
    }

    /// Whether `mailbox` finished a sync within `every`.
    pub async fn mail_synced_recently(
        &self,
        user: &str,
        mailbox: Mailbox,
        every: Duration,
    ) -> Result<bool> {
        let (user, every) = (user.to_owned(), format!("-{} seconds", every.as_secs()));
        Ok(self
            .0
            .call::<_, _, rusqlite::Error>(move |db| {
                db.query_row(
                    "SELECT EXISTS(SELECT 1 FROM mail_sync WHERE user_id=? AND mailbox=?
                     AND synced_at > datetime('now', ?))",
                    params![user, mailbox.slug(), every],
                    |r| r.get(0),
                )
            })
            .await?)
    }

    pub async fn mark_mail_synced(&self, user: &str, mailbox: Mailbox) -> Result<()> {
        let user = user.to_owned();
        self.0
            .call::<_, _, rusqlite::Error>(move |db| {
                db.execute(
                    "INSERT INTO mail_sync (user_id, mailbox, synced_at)
                     VALUES (?, ?, datetime('now'))
                     ON CONFLICT(user_id, mailbox) DO UPDATE SET synced_at=excluded.synced_at",
                    params![user, mailbox.slug()],
                )?;
                Ok(())
            })
            .await?;
        Ok(())
    }

    /// When the newest processed email of `mailbox` arrived.
    pub async fn mail_cursor(&self, user: &str, mailbox: Mailbox) -> Result<Option<DateTime<Utc>>> {
        let user = user.to_owned();
        let newest: Option<String> = self
            .0
            .call::<_, _, rusqlite::Error>(move |db| {
                db.query_row(
                    "SELECT max(received_at) FROM mail_seen WHERE user_id=? AND mailbox=?",
                    params![user, mailbox.slug()],
                    |r| r.get(0),
                )
            })
            .await?;
        Ok(newest
            .and_then(|text| DateTime::parse_from_rfc3339(&text).ok())
            .map(|date| date.with_timezone(&Utc)))
    }

    /// Ids of the emails of `mailbox` already processed.
    pub async fn mail_seen(&self, user: &str, mailbox: Mailbox) -> Result<HashSet<String>> {
        let user = user.to_owned();
        Ok(self
            .0
            .call::<_, _, rusqlite::Error>(move |db| {
                let mut stmt =
                    db.prepare("SELECT message_id FROM mail_seen WHERE user_id=? AND mailbox=?")?;
                stmt.query_map(params![user, mailbox.slug()], |r| r.get(0))?
                    .collect()
            })
            .await?)
    }

    pub async fn mark_mail_seen(
        &self,
        user: &str,
        mailbox: Mailbox,
        id: &str,
        received: DateTime<Utc>,
    ) -> Result<()> {
        let (user, id) = (user.to_owned(), id.to_owned());
        let received = received.to_rfc3339_opts(SecondsFormat::Secs, true);
        self.0
            .call::<_, _, rusqlite::Error>(move |db| {
                db.execute(
                    "INSERT OR IGNORE INTO mail_seen (user_id, mailbox, message_id, received_at)
                     VALUES (?, ?, ?, ?)",
                    params![user, mailbox.slug(), id, received],
                )?;
                Ok(())
            })
            .await?;
        Ok(())
    }

    /// Forgets processed ids older than `before`, which no run reads again.
    pub async fn prune_mail_seen(
        &self,
        user: &str,
        mailbox: Mailbox,
        before: DateTime<Utc>,
    ) -> Result<usize> {
        let user = user.to_owned();
        let before = before.to_rfc3339_opts(SecondsFormat::Secs, true);
        Ok(self
            .0
            .call::<_, _, rusqlite::Error>(move |db| {
                // The newest row is the cursor and always stays.
                db.execute(
                    "DELETE FROM mail_seen WHERE user_id=?1 AND mailbox=?2 AND received_at < ?3
                     AND received_at < (SELECT max(received_at) FROM mail_seen
                                        WHERE user_id=?1 AND mailbox=?2)",
                    params![user, mailbox.slug(), before],
                )
            })
            .await?)
    }

    /// Users with at least one MCP server that holds credentials.
    pub async fn mcp_users(&self) -> Result<Vec<String>> {
        Ok(self
            .0
            .call::<_, _, rusqlite::Error>(|db| {
                let mut stmt = db.prepare(
                    "SELECT DISTINCT user_id FROM mcp_servers WHERE credentials IS NOT NULL",
                )?;
                let rows = stmt
                    .query_map([], |r| r.get(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows)
            })
            .await?)
    }

    /// Saves a server's config. Credentials are untouched: they change only
    /// through `save_mcp_credentials`, so a stale config cannot roll back a
    /// rotated refresh token.
    pub async fn save_mcp_config(&self, user: &str, name: &str, config: Value) -> Result<()> {
        let (user, name) = (user.to_owned(), name.to_owned());
        self.0
            .call::<_, _, rusqlite::Error>(move |db| {
                db.execute(
                    "INSERT INTO mcp_servers(user_id, name, config) VALUES(?,?,?)
                     ON CONFLICT(user_id, name) DO UPDATE SET config=excluded.config",
                    params![user, name, config],
                )?;
                Ok(())
            })
            .await?;
        Ok(())
    }

    /// Saves a server's OAuth credentials; `Value::Null` clears them.
    pub async fn save_mcp_credentials(
        &self,
        user: &str,
        name: &str,
        credentials: Value,
    ) -> Result<()> {
        let (user, name) = (user.to_owned(), name.to_owned());
        let credentials = (!credentials.is_null()).then_some(credentials);
        self.0
            .call::<_, _, rusqlite::Error>(move |db| {
                db.execute(
                    "INSERT INTO mcp_servers(user_id, name, credentials) VALUES(?,?,?)
                     ON CONFLICT(user_id, name) DO UPDATE SET credentials=excluded.credentials",
                    params![user, name, credentials],
                )?;
                Ok(())
            })
            .await?;
        Ok(())
    }

    pub async fn delete_mcp_server(&self, user: &str, name: &str) -> Result<bool> {
        let (user, name) = (user.to_owned(), name.to_owned());
        Ok(self
            .0
            .call::<_, _, rusqlite::Error>(move |db| {
                Ok(db.execute(
                    "DELETE FROM mcp_servers WHERE user_id=? AND name=?",
                    params![user, name],
                )? > 0)
            })
            .await?)
    }

    pub async fn update_location(
        &self,
        user: &str,
        latitude: f64,
        longitude: f64,
        accuracy: f64,
    ) -> Result<()> {
        self.update_location_tz(user, latitude, longitude, accuracy, "UTC")
            .await
    }

    pub async fn update_location_tz(
        &self,
        user: &str,
        latitude: f64,
        longitude: f64,
        accuracy: f64,
        timezone: &str,
    ) -> Result<()> {
        let user = user.to_owned();
        let timezone = timezone.to_owned();
        self.0
            .call::<_, _, rusqlite::Error>(move |db| {
                db.execute(
                    "INSERT INTO locations (user_id, latitude, longitude, accuracy, timezone, updated_at) VALUES (?, ?, ?, ?, ?, strftime('%Y-%m-%dT%H:%M:%SZ','now')) ON CONFLICT(user_id) DO UPDATE SET latitude=excluded.latitude, longitude=excluded.longitude, accuracy=excluded.accuracy, timezone=excluded.timezone, updated_at=excluded.updated_at",
                    params![user, latitude, longitude, accuracy, timezone],
                )?;
                Ok(())
            })
            .await?;
        Ok(())
    }

    pub async fn get_location(&self, user: &str) -> Result<Option<Value>> {
        let user = user.to_owned();
        Ok(self
            .0
            .call::<_, _, rusqlite::Error>(move |db| {
                db.query_row(
                    "SELECT latitude, longitude, accuracy, timezone, updated_at FROM locations WHERE user_id=?",
                    params![user],
                    |r| {
                        Ok(json!({
                            "latitude": r.get::<_, f64>(0)?,
                            "longitude": r.get::<_, f64>(1)?,
                            "accuracy": r.get::<_, f64>(2)?,
                            "timezone": r.get::<_, String>(3)?,
                            "updated_at": r.get::<_, String>(4)?,
                        }))
                    },
                )
                .optional()
            })
            .await?)
    }

    pub async fn get_timezone(&self, user: &str) -> Result<String> {
        let user = user.to_owned();
        Ok(self
            .0
            .call::<_, _, rusqlite::Error>(move |db| {
                db.query_row(
                    "SELECT timezone FROM locations WHERE user_id=?",
                    params![user],
                    |r| r.get(0),
                )
                .optional()
                .map(|v| v.unwrap_or_else(|| "UTC".into()))
            })
            .await?)
    }

    pub async fn save_notifications(
        &self,
        user: &str,
        notifications: Vec<PhoneNotification>,
    ) -> Result<usize> {
        let user = user.to_owned();
        Ok(self
            .0
            .call::<_, _, rusqlite::Error>(move |db| {
                let tx = db.transaction()?;
                let mut count = 0usize;
                for n in &notifications {
                    if n.app.is_empty() || n.timestamp == 0 {
                        continue;
                    }
                    let id = Uuid::new_v4().to_string();
                    let inserted = tx.execute(
                        "INSERT OR IGNORE INTO notifications (id, user_id, app, title, text, timestamp) VALUES (?, ?, ?, ?, ?, ?)",
                        params![id, user, n.app, n.title, n.text, n.timestamp],
                    )?;
                    count += inserted;
                }
                tx.commit()?;
                Ok(count)
            })
            .await?)
    }

    pub async fn query_notifications(
        &self,
        user: &str,
        app: Option<&str>,
        since_minutes: Option<i64>,
        keyword: Option<&str>,
        limit: Option<i64>,
    ) -> Result<Vec<Value>> {
        let user = user.to_owned();
        let app = app.map(|s| s.to_owned());
        let keyword = keyword.map(|s| s.to_owned());
        let since_minutes = since_minutes.unwrap_or(60);
        let limit = limit.unwrap_or(100).min(500);
        Ok(self
            .0
            .call::<_, _, rusqlite::Error>(move |db| {
                let cutoff_ms =
                    chrono::Utc::now().timestamp_millis() - since_minutes * 60 * 1000;
                let mut sql = String::from(
                    "SELECT app, title, text, timestamp FROM notifications WHERE user_id=? AND timestamp>=?",
                );
                let mut params_vec: Vec<Box<dyn rusqlite::types::ToSql>> =
                    vec![Box::new(user), Box::new(cutoff_ms)];
                if let Some(ref app) = app {
                    sql.push_str(" AND (app=? OR instr(lower(app),lower(?))>0)");
                    params_vec.push(Box::new(app.clone()));
                    params_vec.push(Box::new(app.clone()));
                }
                if let Some(ref kw) = keyword {
                    sql.push_str(
                        " AND (instr(lower(title),lower(?))>0 OR instr(lower(text),lower(?))>0)",
                    );
                    params_vec.push(Box::new(kw.clone()));
                    params_vec.push(Box::new(kw.clone()));
                }
                sql.push_str(" ORDER BY timestamp DESC LIMIT ?");
                params_vec.push(Box::new(limit));
                let mut stmt = db.prepare(&sql)?;
                let rows = stmt
                    .query_map(rusqlite::params_from_iter(params_vec.iter()), |r| {
                        Ok(json!({
                            "app": r.get::<_, String>(0)?,
                            "title": r.get::<_, Option<String>>(1)?,
                            "text": r.get::<_, Option<String>>(2)?,
                            "timestamp": r.get::<_, i64>(3)?,
                        }))
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows)
            })
            .await?)
    }

    pub async fn purge_old_notifications(&self) -> Result<u64> {
        Ok(self
            .0
            .call::<_, _, rusqlite::Error>(|db| {
                let cutoff_ms = chrono::Utc::now().timestamp_millis() - 7 * 24 * 60 * 60 * 1000;
                let deleted = db.execute(
                    "DELETE FROM notifications WHERE timestamp < ?",
                    params![cutoff_ms],
                )?;
                Ok(deleted as u64)
            })
            .await?)
    }

    pub async fn create_habit(&self, user: &str, name: &str, frequency: &str) -> Result<String> {
        if name.is_empty() || name.len() > 200 {
            bail!("habit name must be 1-200 characters");
        }
        let valid_freq = ["daily", "weekly", "weekdays"];
        if !valid_freq.contains(&frequency) {
            bail!(
                "invalid frequency '{}', must be one of: {}",
                frequency,
                valid_freq.join(", ")
            );
        }
        let id = Uuid::new_v4().to_string();
        let (user, id_clone, name, frequency) = (
            user.to_owned(),
            id.clone(),
            name.to_owned(),
            frequency.to_owned(),
        );
        self.0
            .call::<_, _, rusqlite::Error>(move |db| {
                db.execute(
                    "INSERT INTO habits (id, user_id, name, frequency) VALUES (?, ?, ?, ?)",
                    params![id_clone, user, name, frequency],
                )?;
                Ok(())
            })
            .await?;
        Ok(id)
    }

    pub async fn list_habits(&self, user: &str) -> Result<Vec<Value>> {
        let user = user.to_owned();
        Ok(self
            .0
            .call::<_, _, rusqlite::Error>(move |db| {
                let mut stmt = db.prepare(
                    "SELECT h.id, h.name, h.frequency, h.created_at,
                            COALESCE((SELECT COUNT(*) FROM habit_logs l WHERE l.habit_id=h.id), 0)
                     FROM habits h
                     WHERE h.user_id=? AND h.archived=0
                     ORDER BY h.created_at",
                )?;
                let rows = stmt
                    .query_map([&user], |r| {
                        Ok((
                            r.get::<_, String>(0)?,
                            r.get::<_, String>(1)?,
                            r.get::<_, String>(2)?,
                            r.get::<_, String>(3)?,
                            r.get::<_, i64>(4)?,
                        ))
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?;

                let mut result = Vec::with_capacity(rows.len());
                for (habit_id, name, frequency, created_at, total_logs) in rows {
                    let streak = calculate_streak(db, &habit_id)?;
                    result.push(json!({
                        "name": name,
                        "frequency": frequency,
                        "created_at": created_at,
                        "total_logs": total_logs,
                        "current_streak": streak,
                    }));
                }
                Ok(result)
            })
            .await?)
    }

    pub async fn log_habit(
        &self,
        user: &str,
        habit_name: &str,
        value: f64,
        note: Option<&str>,
    ) -> Result<()> {
        let (user, habit_name) = (user.to_owned(), habit_name.to_owned());
        let note = note.map(|s| s.to_owned());
        let id = Uuid::new_v4().to_string();
        let found = self
            .0
            .call::<_, _, rusqlite::Error>(move |db| {
                let habit_id: Option<String> = db
                    .query_row(
                        "SELECT id FROM habits WHERE user_id=? AND name=? AND archived=0",
                        params![user, habit_name],
                        |r| r.get(0),
                    )
                    .optional()?;
                let habit_id = match habit_id {
                    Some(hid) => hid,
                    None => return Ok(false),
                };
                db.execute(
                    "INSERT INTO habit_logs (id, habit_id, user_id, value, note) VALUES (?, ?, ?, ?, ?)",
                    params![id, habit_id, user, value, note],
                )?;
                Ok(true)
            })
            .await?;
        if !found {
            bail!("habit not found: check the name and make sure it is not archived");
        }
        Ok(())
    }

    pub async fn habit_status(&self, user: &str, habit_name: &str, days: i64) -> Result<Value> {
        let (user, habit_name) = (user.to_owned(), habit_name.to_owned());
        Ok(self
            .0
            .call::<_, _, rusqlite::Error>(move |db| {
                let row: Option<(String, String, String, String)> = db
                    .query_row(
                        "SELECT id, name, frequency, created_at FROM habits WHERE user_id=? AND name=? AND archived=0",
                        params![user, habit_name],
                        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
                    )
                    .optional()?;
                let (habit_id, name, frequency, created_at) = match row {
                    Some(r) => r,
                    None => return Ok(json!({"error": "habit not found"})),
                };

                let mut stmt = db.prepare(
                    "SELECT date(logged_at) as day, SUM(value), GROUP_CONCAT(note, '; ')
                     FROM habit_logs
                     WHERE habit_id=? AND logged_at >= date('now', ?)
                     GROUP BY day ORDER BY day",
                )?;
                let offset = format!("-{} days", days);
                let logs = stmt
                    .query_map(params![habit_id, offset], |r| {
                        Ok(json!({
                            "date": r.get::<_, String>(0)?,
                            "total": r.get::<_, f64>(1)?,
                            "notes": r.get::<_, Option<String>>(2)?,
                        }))
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?;

                let logged_days = logs.len() as f64;
                let rate = if days > 0 {
                    logged_days / days as f64
                } else {
                    0.0
                };
                let streak = calculate_streak(db, &habit_id)?;

                Ok(json!({
                    "name": name,
                    "frequency": frequency,
                    "created_at": created_at,
                    "days_queried": days,
                    "logged_days": logged_days as i64,
                    "completion_rate": (rate * 100.0).round() / 100.0,
                    "current_streak": streak,
                    "logs": logs,
                }))
            })
            .await?)
    }

    pub async fn archive_habit(&self, user: &str, habit_name: &str) -> Result<bool> {
        let (user, habit_name) = (user.to_owned(), habit_name.to_owned());
        Ok(self
            .0
            .call::<_, _, rusqlite::Error>(move |db| {
                Ok(db.execute(
                    "UPDATE habits SET archived=1 WHERE user_id=? AND name=? AND archived=0",
                    params![user, habit_name],
                )? > 0)
            })
            .await?)
    }

    /// Create default cron jobs for a user if they have none yet.
    pub async fn ensure_default_crons(&self, user: &str) -> Result<()> {
        let existing = self.list_crons(user).await?;
        if !existing.is_empty() {
            return Ok(());
        }
        self.create_cron(
            user,
            "Morning briefing",
            "0 8 * * *",
            "Give me a brief morning update: current time, any scheduled crons coming up today, and a motivational note. Keep it to 2-3 sentences.",
        )
        .await?;
        Ok(())
    }

    /// Return user IDs who have had a session or registered a device.
    pub async fn active_users(&self) -> Result<Vec<String>> {
        Ok(self
            .0
            .call::<_, _, rusqlite::Error>(|db| {
                let mut stmt = db.prepare(
                    "SELECT DISTINCT user_id FROM sessions
                     UNION
                     SELECT DISTINCT user_id FROM devices",
                )?;
                let rows = stmt
                    .query_map([], |r| r.get::<_, String>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows)
            })
            .await?)
    }

    /// Count notifications for a user received in the last `minutes` minutes.
    pub async fn notification_count_since(&self, user: &str, minutes: i64) -> Result<i64> {
        let user = user.to_owned();
        Ok(self
            .0
            .call::<_, _, rusqlite::Error>(move |db| {
                let cutoff_ms = chrono::Utc::now().timestamp_millis() - minutes * 60 * 1000;
                db.query_row(
                    "SELECT COUNT(*) FROM notifications WHERE user_id=? AND timestamp >= ?",
                    params![user, cutoff_ms],
                    |r| r.get(0),
                )
            })
            .await?)
    }
}

/// Count consecutive days with at least one log, going backward from today.
fn calculate_streak(db: &rusqlite::Connection, habit_id: &str) -> rusqlite::Result<i64> {
    let mut stmt = db.prepare(
        "SELECT DISTINCT date(logged_at) as day FROM habit_logs WHERE habit_id=? ORDER BY day DESC",
    )?;
    let days: Vec<String> = stmt
        .query_map(params![habit_id], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    if days.is_empty() {
        return Ok(0);
    }

    let today: String = db.query_row("SELECT date('now')", [], |r| r.get(0))?;
    let mut streak = 0i64;
    let mut expected = today.clone();

    for day in &days {
        if *day == expected {
            streak += 1;
            expected = db.query_row("SELECT date(?, '-1 day')", params![expected], |r| {
                r.get::<_, String>(0)
            })?;
        } else if streak == 0 {
            // Allow starting from yesterday if nothing logged today yet
            let yesterday: String =
                db.query_row("SELECT date('now', '-1 day')", [], |r| r.get(0))?;
            if *day == yesterday {
                streak = 1;
                expected = db.query_row("SELECT date(?, '-1 day')", params![day], |r| {
                    r.get::<_, String>(0)
                })?;
            } else {
                break;
            }
        } else {
            break;
        }
    }

    Ok(streak)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn keyed_memories_get_ids_and_keep_their_subject() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("old.db");
        let old = rusqlite::Connection::open(&path).unwrap();
        old.execute_batch(
            "CREATE TABLE memories (
                 user_id TEXT NOT NULL, key TEXT NOT NULL, content TEXT NOT NULL,
                 PRIMARY KEY(user_id, key));
             INSERT INTO memories VALUES ('alice', 'dog_name', 'Rex');
             INSERT INTO memories VALUES ('alice', 'home', 'Lives in Lisbon');",
        )
        .unwrap();
        drop(old);

        let store = Store::open(&path).await.unwrap();
        let mut rows = store.memory_rows("alice").await.unwrap();
        rows.sort_by(|a, b| a.content.cmp(&b.content));
        let contents: Vec<_> = rows.iter().map(|r| r.content.as_str()).collect();
        assert_eq!(contents, ["Lives in Lisbon", "dog name: Rex"]);
        assert!(rows.iter().all(|r| r.id.len() == 12));
        assert!(rows.iter().all(|r| r.source == MemorySource::User {}));
        assert!(store.forget("alice", &rows[1].id).await.unwrap());
    }

    #[tokio::test]
    async fn mail_cursor_follows_seen_mail_and_survives_pruning() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("db.sqlite")).await.unwrap();
        let day = |d: u32| {
            NaiveDate::from_ymd_opt(2026, 3, d)
                .unwrap()
                .and_hms_opt(12, 0, 0)
                .unwrap()
                .and_utc()
        };
        let gmail = Mailbox::Gmail;
        assert!(store.mail_cursor("alice", gmail).await.unwrap().is_none());
        for (id, d) in [("a", 1), ("b", 5), ("c", 3)] {
            store
                .mark_mail_seen("alice", gmail, id, day(d))
                .await
                .unwrap();
        }
        store
            .mark_mail_seen("alice", gmail, "a", day(9))
            .await
            .unwrap();
        assert_eq!(
            store.mail_cursor("alice", gmail).await.unwrap(),
            Some(day(5))
        );
        assert!(
            store
                .mail_cursor("alice", Mailbox::Fastmail)
                .await
                .unwrap()
                .is_none()
        );

        // Everything is older than the cutoff, but the newest row stays.
        let pruned = store
            .prune_mail_seen("alice", gmail, day(20))
            .await
            .unwrap();
        assert_eq!(pruned, 2);
        assert_eq!(
            store.mail_cursor("alice", gmail).await.unwrap(),
            Some(day(5))
        );
        let seen = store.mail_seen("alice", gmail).await.unwrap();
        assert_eq!(seen, HashSet::from(["b".to_owned()]));

        let every = Duration::from_secs(60);
        assert!(
            !store
                .mail_synced_recently("alice", gmail, every)
                .await
                .unwrap()
        );
        store.mark_mail_synced("alice", gmail).await.unwrap();
        assert!(
            store
                .mail_synced_recently("alice", gmail, every)
                .await
                .unwrap()
        );
        assert!(
            !store
                .mail_synced_recently("bob", gmail, every)
                .await
                .unwrap()
        );
    }
}
