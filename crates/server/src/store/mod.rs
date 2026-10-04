use crate::{
    feeds::{mail::Mailbox, slack::SLACK},
    protocol::{Cron, Integration, MemorySource, PhoneNotification, Run, Session},
    user::UserKey,
};
use anyhow::{Result, bail};
use chrono::{DateTime, NaiveDate, SecondsFormat, Utc};
use flashmind::{
    core::Conversation,
    memory::{EmbeddingProvider, MemoryStore},
};
use rusqlite::{OptionalExtension, params};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    path::Path,
    sync::Arc,
    time::Duration,
};
use tokio::sync::Notify;
use tokio_rusqlite::Connection;
use uuid::Uuid;

mod memories;

pub use memories::{Fact, Scored, WordEmbedder};

#[derive(Clone)]
pub struct Store {
    db: Connection,
    /// Every user's memories, each tagged with its owner.
    memories: MemoryStore,
    /// Woken after any write to `crons`, so the scheduler picks the change up.
    crons_changed: Arc<Notify>,
}

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

/// Something synced once a day for memories: a mailbox or Slack. Its slug
/// keys the sync state tables, which predate Slack and keep their mail names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Feed {
    Mail(Mailbox),
    Slack,
}

impl Feed {
    pub const ALL: [Feed; 3] = [
        Feed::Mail(Mailbox::Gmail),
        Feed::Mail(Mailbox::Fastmail),
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

/// How an agent run ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunStatus {
    Completed,
    Cancelled,
    TimedOut,
    Stalled,
    Looped,
    ToolLimit,
    Failed,
}

impl RunStatus {
    /// The name stored in the database and sent to clients.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Cancelled => "cancelled",
            Self::TimedOut => "timed_out",
            Self::Stalled => "stalled",
            Self::Looped => "looped",
            Self::ToolLimit => "tool_limit",
            Self::Failed => "failed",
        }
    }
}

/// The phone's last reported position.
#[derive(Debug, Clone, PartialEq)]
pub struct Location {
    pub latitude: f64,
    pub longitude: f64,
    /// Radius in meters.
    pub accuracy: f64,
    pub timezone: String,
    /// Reverse-geocoded name, such as "Shinjuku, Tokyo, Japan".
    pub place: Option<String>,
    pub updated_at: DateTime<Utc>,
}

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

/// A stored memory.
#[derive(Debug, Clone)]
pub struct MemoryRow {
    pub id: String,
    pub content: String,
    pub category: String,
    pub source: MemorySource,
    /// SQLite `datetime('now')` text.
    pub updated_at: String,
    pub valid: Validity,
}

impl MemoryRow {
    /// `YYYY-MM-DD` of the last change.
    pub fn updated(&self) -> &str {
        self.updated_at.get(..10).unwrap_or_default()
    }
}

fn parse_date(text: Option<String>) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(text.as_deref()?, "%Y-%m-%d").ok()
}

fn format_date(date: Option<NaiveDate>) -> Option<String> {
    date.map(|d| d.format("%Y-%m-%d").to_string())
}

impl Store {
    pub fn raw(&self) -> &Connection {
        &self.db
    }

    /// Woken after any write to `crons`.
    pub fn crons_changed(&self) -> Arc<Notify> {
        self.crons_changed.clone()
    }

    /// Tells the scheduler the `crons` table changed. `notify_one` keeps a permit
    /// when nobody is waiting, so a write between two waits is not lost.
    pub(crate) fn notify_crons_changed(&self) {
        self.crons_changed.notify_one();
    }

    /// Opens the database at `path` and the memory store beside it, and
    /// moves any memories still in the old table into the memory store.
    pub async fn open(
        path: impl AsRef<Path>,
        embedder: Arc<dyn EmbeddingProvider>,
    ) -> Result<Self> {
        let path = path.as_ref();
        let memories = Self::open_memories(path, embedder).await?;
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
                -- Memories now live in the memory store; rows left here are
                -- moved over on open.
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
                    accuracy REAL NOT NULL, timezone TEXT NOT NULL DEFAULT 'UTC', place TEXT,
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
                CREATE TABLE IF NOT EXISTS memory_scores (
                    user_id TEXT NOT NULL, session_id TEXT NOT NULL, turn_at INTEGER NOT NULL,
                    probability REAL NOT NULL,
                    PRIMARY KEY(user_id, session_id, turn_at));
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
            if !has_col("locations", "place")? {
                db.execute_batch("ALTER TABLE locations ADD COLUMN place TEXT;")?;
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
        let store = Self {
            db,
            memories,
            crons_changed: Arc::new(Notify::new()),
        };
        store.move_legacy_memories().await?;
        Ok(store)
    }

    /// Opens a store whose memories are embedded by `WordEmbedder`.
    #[doc(hidden)]
    pub async fn open_for_test(path: impl AsRef<Path>) -> Result<Self> {
        Self::open(path, Arc::new(WordEmbedder)).await
    }

    // Called once before accepting requests. A server restart never silently repeats tools.
    pub async fn recover(&self) -> Result<()> {
        self.db
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

    pub(crate) async fn create_run(
        &self,
        user: &UserKey,
        session: &str,
        request: &str,
        message: &str,
    ) -> Result<CreateRun> {
        let user = user.as_str();
        let (user, session, request, message) = (
            user.to_owned(),
            session.to_owned(),
            request.to_owned(),
            message.to_owned(),
        );
        Ok(self.db.call::<_, _, rusqlite::Error>(move |db| {
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

    pub(crate) async fn run(&self, user: &UserKey, id: &str) -> Result<Option<Run>> {
        let user = user.as_str();
        let (user, id) = (user.to_owned(), id.to_owned());
        Ok(self
            .db
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

    pub(crate) async fn sessions(&self, user: &UserKey) -> Result<Vec<Session>> {
        let user = user.as_str().to_owned();
        Ok(self.db.call::<_, _, rusqlite::Error>(move |db| {
            let mut statement = db.prepare(
                "SELECT s.id, COALESCE(t.title, (SELECT substr(message,1,80) FROM runs r WHERE r.user_id=s.user_id AND r.session_id=s.id ORDER BY r.rowid LIMIT 1), 'Conversation'), s.pinned
                 FROM sessions s LEFT JOIN session_titles t ON t.user_id=s.user_id AND t.session_id=s.id
                 WHERE s.user_id=? ORDER BY s.pinned DESC, s.rowid DESC LIMIT 200"
            )?;
            let rows = statement.query_map([user], |row| Ok(Session { id: row.get(0)?, title: row.get(1)?, pinned: row.get(2)? }))?.collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        }).await?)
    }

    pub(crate) async fn rename_session(
        &self,
        user: &UserKey,
        session_id: &str,
        title: &str,
    ) -> Result<bool> {
        let user = user.as_str();
        if title.is_empty() || title.len() > 200 {
            bail!("session title must be 1-200 characters");
        }
        let (user, session_id) = (user.to_owned(), session_id.to_owned());
        let title = title.to_owned();
        Ok(self
            .db
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
            .db
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
            .db
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
            .db
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

    pub(crate) async fn delete_session(&self, user: &UserKey, session_id: &str) -> Result<bool> {
        Ok(self
            .delete_sessions(user, vec![session_id.to_owned()])
            .await?
            > 0)
    }

    /// Deletes sessions with their runs and returns how many existed.
    pub(crate) async fn delete_sessions(&self, user: &UserKey, ids: Vec<String>) -> Result<u64> {
        let user = user.as_str().to_owned();
        Ok(self
            .db
            .call::<_, _, rusqlite::Error>(move |db| {
                let tx = db.transaction()?;
                let mut deleted = 0;
                for id in &ids {
                    for table in ["runs", "tool_failures", "memory_scores", "session_titles"] {
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

    pub(crate) async fn pin_session(
        &self,
        user: &UserKey,
        session_id: &str,
        pinned: bool,
    ) -> Result<bool> {
        let user = user.as_str();
        let (user, session_id) = (user.to_owned(), session_id.to_owned());
        Ok(self
            .db
            .call::<_, _, rusqlite::Error>(move |db| {
                let changed = db.execute(
                    "UPDATE sessions SET pinned=? WHERE user_id=? AND id=?",
                    params![pinned, user, session_id],
                )?;
                Ok(changed > 0)
            })
            .await?)
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
        let (user, session) = (user.to_owned(), session.to_owned());
        let raw: Option<String> = self
            .db
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
        let (user, session) = (user.to_owned(), session.to_owned());
        let raw: Option<String> = self
            .db
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
            .db
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
            .db
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

    pub(crate) async fn finish(
        &self,
        user: &UserKey,
        run: &Run,
        turn: Option<FinishedTurn<'_>>,
        status: RunStatus,
    ) -> Result<()> {
        let user = user.as_str();
        let (user, run, status) = (user.to_owned(), run.clone(), status.as_str());
        let turn = turn
            .map(|turn| {
                anyhow::Ok((
                    serde_json::to_string(turn.transcript)?,
                    serde_json::to_string(turn.context)?,
                    turn.prompt_tokens,
                ))
            })
            .transpose()?;
        self.db.call::<_, _, rusqlite::Error>(move |db| {
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
    pub(crate) async fn record_tool_failures(
        &self,
        user: &UserKey,
        session: &str,
        call_ids: Vec<String>,
    ) -> Result<()> {
        let user = user.as_str();
        if call_ids.is_empty() {
            return Ok(());
        }
        let (user, session) = (user.to_owned(), session.to_owned());
        self.db
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
    pub(crate) async fn tool_failures(
        &self,
        user: &UserKey,
        session: &str,
    ) -> Result<HashSet<String>> {
        let user = user.as_str();
        let (user, session) = (user.to_owned(), session.to_owned());
        Ok(self
            .db
            .call::<_, _, rusqlite::Error>(move |db| {
                let mut stmt = db.prepare(
                    "SELECT call_id FROM tool_failures WHERE user_id=? AND session_id=?",
                )?;
                stmt.query_map(params![user, session], |r| r.get(0))?
                    .collect()
            })
            .await?)
    }

    /// Records Jev's memory score for the turn whose user message was sent at
    /// `turn_at`, in milliseconds since the epoch.
    pub(crate) async fn record_memory_score(
        &self,
        user: &UserKey,
        session: &str,
        turn_at: i64,
        probability: f64,
    ) -> Result<()> {
        let user = user.as_str();
        let (user, session) = (user.to_owned(), session.to_owned());
        self.db
            .call::<_, _, rusqlite::Error>(move |db| {
                db.execute(
                    "INSERT OR REPLACE INTO memory_scores VALUES (?, ?, ?, ?)",
                    params![user, session, turn_at, probability],
                )
            })
            .await?;
        Ok(())
    }

    /// The session's memory scores by the time of each turn's user message.
    pub(crate) async fn memory_scores(
        &self,
        user: &UserKey,
        session: &str,
    ) -> Result<HashMap<i64, f64>> {
        let user = user.as_str();
        let (user, session) = (user.to_owned(), session.to_owned());
        Ok(self
            .db
            .call::<_, _, rusqlite::Error>(move |db| {
                let mut stmt = db.prepare(
                    "SELECT turn_at, probability FROM memory_scores
                     WHERE user_id=? AND session_id=?",
                )?;
                stmt.query_map(params![user, session], |r| Ok((r.get(0)?, r.get(1)?)))?
                    .collect()
            })
            .await?)
    }

    pub(crate) async fn list_crons(&self, user: &UserKey) -> Result<Vec<Cron>> {
        let user = user.as_str().to_owned();
        Ok(self
            .db
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

    pub(crate) async fn create_cron(
        &self,
        user: &UserKey,
        label: &str,
        schedule: &str,
        prompt: &str,
    ) -> Result<String> {
        self.create_cron_ex(user, label, schedule, prompt, false)
            .await
    }

    pub(crate) async fn create_cron_ex(
        &self,
        user: &UserKey,
        label: &str,
        schedule: &str,
        prompt: &str,
        run_once: bool,
    ) -> Result<String> {
        let user = user.as_str();
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
        self.db
            .call::<_, _, rusqlite::Error>(move |db| {
                db.execute(
                    "INSERT INTO crons (user_id, id, label, prompt, schedule, run_once) VALUES (?, ?, ?, ?, ?, ?)",
                    params![user, id_clone, label, prompt, schedule, run_once],
                )?;
                Ok(())
            })
            .await?;
        self.notify_crons_changed();
        Ok(id)
    }

    pub(crate) async fn update_cron(
        &self,
        user: &UserKey,
        id: &str,
        enabled: Option<bool>,
        label: Option<&str>,
        schedule: Option<&str>,
    ) -> Result<bool> {
        let user = user.as_str();
        let (user, id) = (user.to_owned(), id.to_owned());
        let set_enabled = enabled;
        let label = label.map(|s| s.to_owned());
        let schedule = schedule.map(|s| s.to_owned());
        let changed = self
            .db
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
            .await?;
        self.notify_crons_changed();
        Ok(changed)
    }

    pub(crate) async fn toggle_cron(&self, user: &UserKey, id: &str) -> Result<Option<bool>> {
        let user = user.as_str();
        let (user, id) = (user.to_owned(), id.to_owned());
        let new_enabled = self
            .db
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
            .await?;
        self.notify_crons_changed();
        Ok(new_enabled)
    }

    pub(crate) async fn delete_cron(&self, user: &UserKey, id: &str) -> Result<bool> {
        let user = user.as_str();
        let (user, id) = (user.to_owned(), id.to_owned());
        let deleted = self
            .db
            .call::<_, _, rusqlite::Error>(move |db| {
                Ok(db.execute(
                    "DELETE FROM crons WHERE user_id=? AND id=?",
                    params![user, id],
                )? > 0)
            })
            .await?;
        self.notify_crons_changed();
        Ok(deleted)
    }

    pub(crate) async fn register_device(
        &self,
        user: &UserKey,
        device_id: &str,
        fcm_token: &str,
    ) -> Result<()> {
        let user = user.as_str();
        let (user, device_id, fcm_token) =
            (user.to_owned(), device_id.to_owned(), fcm_token.to_owned());
        self.db
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

    pub(crate) async fn unregister_device(&self, user: &UserKey, device_id: &str) -> Result<bool> {
        let user = user.as_str();
        let (user, device_id) = (user.to_owned(), device_id.to_owned());
        Ok(self
            .db
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
            .db
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

    pub(crate) async fn log_cron_start(
        &self,
        user: &UserKey,
        cron_id: &str,
        run_id: &str,
    ) -> Result<()> {
        let user = user.as_str();
        let (user, cron_id, run_id) = (user.to_owned(), cron_id.to_owned(), run_id.to_owned());
        self.db
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

    pub async fn log_cron_finish(
        &self,
        run_id: &str,
        status: RunStatus,
        error: &str,
    ) -> Result<()> {
        let (run_id, status, error) = (run_id.to_owned(), status.as_str(), error.to_owned());
        self.db
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

    pub(crate) async fn cron_last_run(
        &self,
        user: &UserKey,
        cron_id: &str,
    ) -> Result<Option<String>> {
        let user = user.as_str();
        let (user, cron_id) = (user.to_owned(), cron_id.to_owned());
        Ok(self
            .db
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

    pub(crate) async fn device_tokens(&self, user: &UserKey) -> Result<Vec<String>> {
        let user = user.as_str().to_owned();
        Ok(self
            .db
            .call::<_, _, rusqlite::Error>(move |db| {
                let mut stmt = db.prepare("SELECT fcm_token FROM devices WHERE user_id=?")?;
                let rows = stmt
                    .query_map([user], |r| r.get::<_, String>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows)
            })
            .await?)
    }

    pub(crate) async fn list_mcp_connections(&self, user: &UserKey) -> Result<Vec<Integration>> {
        let user = user.as_str().to_owned();
        Ok(self
            .db
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
    pub(crate) async fn get_mcp_connection(
        &self,
        user: &UserKey,
        id: &str,
    ) -> Result<Option<Value>> {
        let user = user.as_str();
        let (user, id) = (user.to_owned(), id.to_owned());
        Ok(self
            .db
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

    pub(crate) async fn create_mcp_connection(
        &self,
        user: &UserKey,
        provider: &str,
        label: &str,
        config: &str,
    ) -> Result<String> {
        let user = user.as_str();
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
        self.db
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

    pub(crate) async fn update_mcp_credentials(
        &self,
        user: &UserKey,
        id: &str,
        credentials: &str,
        status: &str,
    ) -> Result<bool> {
        let user = user.as_str();
        let (user, id, credentials, status) = (
            user.to_owned(),
            id.to_owned(),
            credentials.to_owned(),
            status.to_owned(),
        );
        Ok(self
            .db
            .call::<_, _, rusqlite::Error>(move |db| {
                Ok(db.execute(
                    "UPDATE mcp_connections SET credentials=?, status=? WHERE user_id=? AND id=?",
                    params![credentials, status, user, id],
                )? > 0)
            })
            .await?)
    }

    pub(crate) async fn delete_mcp_connection(&self, user: &UserKey, id: &str) -> Result<bool> {
        let user = user.as_str();
        let (user, id) = (user.to_owned(), id.to_owned());
        Ok(self
            .db
            .call::<_, _, rusqlite::Error>(move |db| {
                Ok(db.execute(
                    "DELETE FROM mcp_connections WHERE user_id=? AND id=?",
                    params![user, id],
                )? > 0)
            })
            .await?)
    }

    /// The user's server-managed MCP servers.
    pub(crate) async fn mcp_servers(&self, user: &UserKey) -> Result<Vec<McpServerRow>> {
        let user = user.as_str().to_owned();
        Ok(self
            .db
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

    /// Whether `feed` finished a sync within `every`.
    pub(crate) async fn feed_synced_recently(
        &self,
        user: &UserKey,
        feed: Feed,
        every: Duration,
    ) -> Result<bool> {
        let user = user.as_str();
        let (user, every) = (user.to_owned(), format!("-{} seconds", every.as_secs()));
        Ok(self
            .db
            .call::<_, _, rusqlite::Error>(move |db| {
                db.query_row(
                    "SELECT EXISTS(SELECT 1 FROM mail_sync WHERE user_id=? AND mailbox=?
                     AND synced_at > datetime('now', ?))",
                    params![user, feed.slug(), every],
                    |r| r.get(0),
                )
            })
            .await?)
    }

    pub(crate) async fn mark_feed_synced(&self, user: &UserKey, feed: Feed) -> Result<()> {
        let user = user.as_str().to_owned();
        self.db
            .call::<_, _, rusqlite::Error>(move |db| {
                db.execute(
                    "INSERT INTO mail_sync (user_id, mailbox, synced_at)
                     VALUES (?, ?, datetime('now'))
                     ON CONFLICT(user_id, mailbox) DO UPDATE SET synced_at=excluded.synced_at",
                    params![user, feed.slug()],
                )?;
                Ok(())
            })
            .await?;
        Ok(())
    }

    /// When the newest processed item of `feed` arrived.
    pub(crate) async fn feed_cursor(
        &self,
        user: &UserKey,
        feed: Feed,
    ) -> Result<Option<DateTime<Utc>>> {
        let user = user.as_str().to_owned();
        let newest: Option<String> = self
            .db
            .call::<_, _, rusqlite::Error>(move |db| {
                db.query_row(
                    "SELECT max(received_at) FROM mail_seen WHERE user_id=? AND mailbox=?",
                    params![user, feed.slug()],
                    |r| r.get(0),
                )
            })
            .await?;
        Ok(newest
            .and_then(|text| DateTime::parse_from_rfc3339(&text).ok())
            .map(|date| date.with_timezone(&Utc)))
    }

    /// Ids of the items of `feed` already processed.
    pub(crate) async fn feed_seen(&self, user: &UserKey, feed: Feed) -> Result<HashSet<String>> {
        let user = user.as_str().to_owned();
        Ok(self
            .db
            .call::<_, _, rusqlite::Error>(move |db| {
                let mut stmt =
                    db.prepare("SELECT message_id FROM mail_seen WHERE user_id=? AND mailbox=?")?;
                stmt.query_map(params![user, feed.slug()], |r| r.get(0))?
                    .collect()
            })
            .await?)
    }

    pub(crate) async fn mark_feed_seen(
        &self,
        user: &UserKey,
        feed: Feed,
        id: &str,
        received: DateTime<Utc>,
    ) -> Result<()> {
        let user = user.as_str();
        let (user, id) = (user.to_owned(), id.to_owned());
        let received = received.to_rfc3339_opts(SecondsFormat::Secs, true);
        self.db
            .call::<_, _, rusqlite::Error>(move |db| {
                db.execute(
                    "INSERT OR IGNORE INTO mail_seen (user_id, mailbox, message_id, received_at)
                     VALUES (?, ?, ?, ?)",
                    params![user, feed.slug(), id, received],
                )?;
                Ok(())
            })
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
        let user = user.as_str().to_owned();
        let before = before.to_rfc3339_opts(SecondsFormat::Secs, true);
        Ok(self
            .db
            .call::<_, _, rusqlite::Error>(move |db| {
                // The newest row is the cursor and always stays.
                db.execute(
                    "DELETE FROM mail_seen WHERE user_id=?1 AND mailbox=?2 AND received_at < ?3
                     AND received_at < (SELECT max(received_at) FROM mail_seen
                                        WHERE user_id=?1 AND mailbox=?2)",
                    params![user, feed.slug(), before],
                )
            })
            .await?)
    }

    /// Saves a server's config. Credentials are untouched: they change only
    /// through `save_mcp_credentials`, so a stale config cannot roll back a
    /// rotated refresh token.
    pub(crate) async fn save_mcp_config(
        &self,
        user: &UserKey,
        name: &str,
        config: Value,
    ) -> Result<()> {
        let user = user.as_str();
        let (user, name) = (user.to_owned(), name.to_owned());
        self.db
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
    pub(crate) async fn save_mcp_credentials(
        &self,
        user: &UserKey,
        name: &str,
        credentials: Value,
    ) -> Result<()> {
        let user = user.as_str();
        let (user, name) = (user.to_owned(), name.to_owned());
        let credentials = (!credentials.is_null()).then_some(credentials);
        self.db
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

    pub(crate) async fn delete_mcp_server(&self, user: &UserKey, name: &str) -> Result<bool> {
        let user = user.as_str();
        let (user, name) = (user.to_owned(), name.to_owned());
        Ok(self
            .db
            .call::<_, _, rusqlite::Error>(move |db| {
                Ok(db.execute(
                    "DELETE FROM mcp_servers WHERE user_id=? AND name=?",
                    params![user, name],
                )? > 0)
            })
            .await?)
    }

    pub(crate) async fn update_location(&self, user: &UserKey, fix: &Location) -> Result<()> {
        let user = user.as_str().to_owned();
        let fix = fix.clone();
        self.db
            .call::<_, _, rusqlite::Error>(move |db| {
                db.execute(
                    "INSERT INTO locations (user_id, latitude, longitude, accuracy, timezone, place, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?) ON CONFLICT(user_id) DO UPDATE SET latitude=excluded.latitude, longitude=excluded.longitude, accuracy=excluded.accuracy, timezone=excluded.timezone, place=excluded.place, updated_at=excluded.updated_at",
                    params![
                        user,
                        fix.latitude,
                        fix.longitude,
                        fix.accuracy,
                        fix.timezone,
                        fix.place,
                        fix.updated_at.to_rfc3339_opts(SecondsFormat::Secs, true)
                    ],
                )?;
                Ok(())
            })
            .await?;
        Ok(())
    }

    pub(crate) async fn get_location(&self, user: &UserKey) -> Result<Option<Location>> {
        let user = user.as_str().to_owned();
        Ok(self
            .db
            .call::<_, _, rusqlite::Error>(move |db| {
                db.query_row(
                    "SELECT latitude, longitude, accuracy, timezone, place, updated_at FROM locations WHERE user_id=?",
                    params![user],
                    |r| {
                        let updated_at: String = r.get(5)?;
                        Ok(Location {
                            latitude: r.get(0)?,
                            longitude: r.get(1)?,
                            accuracy: r.get(2)?,
                            timezone: r.get(3)?,
                            place: r.get(4)?,
                            updated_at: DateTime::parse_from_rfc3339(&updated_at)
                                .map(|t| t.with_timezone(&Utc))
                                .unwrap_or_default(),
                        })
                    },
                )
                .optional()
            })
            .await?)
    }

    pub(crate) async fn get_timezone(&self, user: &UserKey) -> Result<String> {
        let user = user.as_str().to_owned();
        Ok(self
            .db
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

    pub(crate) async fn save_notifications(
        &self,
        user: &UserKey,
        notifications: Vec<PhoneNotification>,
    ) -> Result<usize> {
        let user = user.as_str().to_owned();
        Ok(self
            .db
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

    pub(crate) async fn query_notifications(
        &self,
        user: &UserKey,
        app: Option<&str>,
        since_minutes: Option<i64>,
        keyword: Option<&str>,
        limit: Option<i64>,
    ) -> Result<Vec<Value>> {
        let user = user.as_str().to_owned();
        let app = app.map(|s| s.to_owned());
        let keyword = keyword.map(|s| s.to_owned());
        let since_minutes = since_minutes.unwrap_or(60);
        let limit = limit.unwrap_or(100).min(500);
        Ok(self
            .db
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
            .db
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

    pub(crate) async fn create_habit(
        &self,
        user: &UserKey,
        name: &str,
        frequency: &str,
    ) -> Result<String> {
        let user = user.as_str();
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
        self.db
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

    pub(crate) async fn list_habits(&self, user: &UserKey) -> Result<Vec<Value>> {
        let user = user.as_str().to_owned();
        Ok(self
            .db
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

    pub(crate) async fn log_habit(
        &self,
        user: &UserKey,
        habit_name: &str,
        value: f64,
        note: Option<&str>,
    ) -> Result<()> {
        let user = user.as_str();
        let (user, habit_name) = (user.to_owned(), habit_name.to_owned());
        let note = note.map(|s| s.to_owned());
        let id = Uuid::new_v4().to_string();
        let found = self
            .db
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

    pub(crate) async fn habit_status(
        &self,
        user: &UserKey,
        habit_name: &str,
        days: i64,
    ) -> Result<Value> {
        let user = user.as_str();
        let (user, habit_name) = (user.to_owned(), habit_name.to_owned());
        Ok(self
            .db
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

    pub(crate) async fn archive_habit(&self, user: &UserKey, habit_name: &str) -> Result<bool> {
        let user = user.as_str();
        let (user, habit_name) = (user.to_owned(), habit_name.to_owned());
        Ok(self
            .db
            .call::<_, _, rusqlite::Error>(move |db| {
                Ok(db.execute(
                    "UPDATE habits SET archived=1 WHERE user_id=? AND name=? AND archived=0",
                    params![user, habit_name],
                )? > 0)
            })
            .await?)
    }

    /// Create default cron jobs for a user if they have none yet.
    pub(crate) async fn ensure_default_crons(&self, user: &UserKey) -> Result<()> {
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
            .db
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
    pub(crate) async fn notification_count_since(
        &self,
        user: &UserKey,
        minutes: i64,
    ) -> Result<i64> {
        let user = user.as_str().to_owned();
        Ok(self
            .db
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
    async fn old_memories_move_to_the_memory_store() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("old.db");
        let old = rusqlite::Connection::open(&path).unwrap();
        old.execute_batch(
            "CREATE TABLE memories (
                 user_id TEXT NOT NULL, key TEXT NOT NULL, content TEXT NOT NULL,
                 PRIMARY KEY(user_id, key));
             INSERT INTO memories VALUES ('alice', 'dog_name', 'Rex');
             INSERT INTO memories VALUES ('alice', 'home', 'Lives in Lisbon');
             INSERT INTO memories VALUES ('bob', 'home', 'Lives in Porto');",
        )
        .unwrap();
        drop(old);

        let store = Store::open_for_test(&path).await.unwrap();
        let alice = UserKey::for_test("alice");
        let mut rows = store.memory_rows(&alice).await.unwrap();
        rows.sort_by(|a, b| a.content.cmp(&b.content));
        let contents: Vec<_> = rows.iter().map(|r| r.content.as_str()).collect();
        assert_eq!(contents, ["Lives in Lisbon", "dog name: Rex"]);
        assert!(rows.iter().all(|r| r.source == MemorySource::User {}));
        assert!(
            !store
                .forget(&UserKey::for_test("bob"), &rows[1].id)
                .await
                .unwrap()
        );
        assert!(store.forget(&alice, &rows[1].id).await.unwrap());

        // Opening again moves nothing twice.
        drop(store);
        let store = Store::open_for_test(&path).await.unwrap();
        assert_eq!(store.memory_rows(&alice).await.unwrap().len(), 1);
        assert_eq!(
            store
                .memory_rows(&UserKey::for_test("bob"))
                .await
                .unwrap()
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn feed_cursor_follows_seen_items_and_survives_pruning() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open_for_test(dir.path().join("db.sqlite"))
            .await
            .unwrap();
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
