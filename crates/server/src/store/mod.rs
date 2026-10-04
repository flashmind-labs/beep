use crate::{
    feeds::{mail::Mailbox, slack::SLACK},
    protocol::{Cron, Integration, MemorySource, PhoneNotification, Run, Session},
    user::UserKey,
};
use anyhow::{Result, bail};
use chrono::{DateTime, Days, NaiveDate, SecondsFormat, Utc};
use flashmind::{
    core::Conversation,
    memory::{EmbeddingProvider, MemoryStore},
};
use secrecy::{ExposeSecret, SecretString};
use serde_json::{Value, json};
use sqlx::{
    PgPool, Row,
    postgres::{PgPoolOptions, PgRow},
};
use sqlx_core::migrate::{Migration, MigrationType, Migrator};
use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
    sync::Arc,
    time::Duration,
};
use uuid::Uuid;

mod cron_jobs;
mod memories;
mod test_db;

#[cfg(test)]
pub(crate) use cron_jobs::decode_schedule;
pub use memories::{Fact, Scored, WordEmbedder};
pub use test_db::TestDb;

/// The schema, oldest first. Embedded by hand because `sqlx::migrate!`
/// needs sqlx's `migrate` feature, which clashes with rusqlite.
const MIGRATIONS: &[(i64, &str, &str)] =
    &[(1, "init", include_str!("../../migrations/0001_init.sql"))];

fn migrator() -> Migrator {
    let migrations = MIGRATIONS
        .iter()
        .map(|&(version, description, sql)| {
            Migration::new(
                version,
                Cow::Borrowed(description),
                MigrationType::Simple,
                Cow::Borrowed(sql),
                false,
            )
        })
        .collect();
    Migrator {
        migrations: Cow::Owned(migrations),
        ..Migrator::DEFAULT
    }
}

/// Most connections one server process opens.
const MAX_CONNECTIONS: u32 = 10;

#[derive(Clone)]
pub struct Store {
    pool: PgPool,
    /// Every user's memories, each tagged with its owner.
    memories: MemoryStore,
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
    /// `YYYY-MM-DD HH:MM:SS` in UTC.
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

/// RFC 3339 in UTC to the second, as clients get times.
fn format_time(time: DateTime<Utc>) -> String {
    time.to_rfc3339_opts(SecondsFormat::Secs, true)
}

/// `duration` in seconds, for `make_interval(secs => ...)`.
fn seconds(duration: Duration) -> f64 {
    duration.as_secs_f64()
}

impl Store {
    /// Connects to the database at `url`, brings its schema up to date and
    /// opens the memory store in the same database.
    pub async fn open(url: &SecretString, embedder: Arc<dyn EmbeddingProvider>) -> Result<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(MAX_CONNECTIONS)
            .connect(url.expose_secret())
            .await?;
        Self::with_pool(pool, embedder).await
    }

    /// Runs the migrations on `pool` and opens the store on it.
    pub async fn with_pool(pool: PgPool, embedder: Arc<dyn EmbeddingProvider>) -> Result<Self> {
        migrator().run(&pool).await?;
        let memories = MemoryStore::connect_postgres(pool.clone(), embedder).await?;
        Ok(Self { pool, memories })
    }

    #[cfg(test)]
    pub(crate) fn pool(&self) -> &PgPool {
        &self.pool
    }

    // Called once before accepting requests. A server restart never silently repeats tools.
    pub async fn recover(&self) -> Result<()> {
        sqlx::query("UPDATE runs SET status='interrupted' WHERE status='running'")
            .execute(&self.pool)
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
        if let Some(found) = self
            .run_for_request(user, session, request, message)
            .await?
        {
            return Ok(found);
        }
        let mut tx = self.pool.begin().await?;
        let busy: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM runs WHERE user_id=$1 AND session_id=$2 AND status='running')",
        )
        .bind(user)
        .bind(session)
        .fetch_one(&mut *tx)
        .await?;
        if busy {
            return Ok(CreateRun::Conflict);
        }
        let active: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM runs WHERE status='running'")
            .fetch_one(&mut *tx)
            .await?;
        if active >= 16 {
            return Ok(CreateRun::Conflict);
        }
        let run = Run {
            id: Uuid::new_v4().to_string(),
            session_id: session.to_owned(),
            status: "running".into(),
        };
        // A concurrent run for the same request or session wins the race
        // through the unique indexes; this one then reports what it lost to.
        let inserted = sqlx::query(
            "INSERT INTO runs (id, user_id, session_id, request_id, message, status)
             VALUES ($1, $2, $3, $4, $5, 'running') ON CONFLICT DO NOTHING",
        )
        .bind(&run.id)
        .bind(user)
        .bind(session)
        .bind(request)
        .bind(message)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        tx.commit().await?;
        if inserted == 1 {
            return Ok(CreateRun::Created(run));
        }
        Ok(self
            .run_for_request(user, session, request, message)
            .await?
            .unwrap_or(CreateRun::Conflict))
    }

    /// The run already made for `request`: the same one again, or a
    /// conflict when the request id was used for something else.
    async fn run_for_request(
        &self,
        user: &str,
        session: &str,
        request: &str,
        message: &str,
    ) -> Result<Option<CreateRun>> {
        let row = sqlx::query(
            "SELECT id, session_id, status, message FROM runs WHERE user_id=$1 AND request_id=$2",
        )
        .bind(user)
        .bind(request)
        .fetch_optional(&self.pool)
        .await?;
        let Some(row) = row else {
            return Ok(None);
        };
        let run = Run {
            id: row.try_get(0)?,
            session_id: row.try_get(1)?,
            status: row.try_get(2)?,
        };
        let original: String = row.try_get(3)?;
        Ok(Some(if run.session_id == session && original == message {
            CreateRun::Existing(run)
        } else {
            CreateRun::Conflict
        }))
    }

    pub(crate) async fn run(&self, user: &UserKey, id: &str) -> Result<Option<Run>> {
        let row = sqlx::query("SELECT id, session_id, status FROM runs WHERE user_id=$1 AND id=$2")
            .bind(user.as_str())
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        row.map(|r| {
            Ok(Run {
                id: r.try_get(0)?,
                session_id: r.try_get(1)?,
                status: r.try_get(2)?,
            })
        })
        .transpose()
    }

    pub(crate) async fn sessions(&self, user: &UserKey) -> Result<Vec<Session>> {
        let rows = sqlx::query(
            "SELECT s.id, COALESCE(t.title, (SELECT substr(message, 1, 80) FROM runs r
                 WHERE r.user_id=s.user_id AND r.session_id=s.id ORDER BY r.seq LIMIT 1), 'Conversation'),
                 s.pinned
             FROM sessions s LEFT JOIN session_titles t ON t.user_id=s.user_id AND t.session_id=s.id
             WHERE s.user_id=$1 ORDER BY s.pinned DESC, s.seq DESC LIMIT 200",
        )
        .bind(user.as_str())
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|r| {
                Ok(Session {
                    id: r.try_get(0)?,
                    title: r.try_get(1)?,
                    pinned: r.try_get(2)?,
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
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM runs WHERE user_id=$1 AND session_id=$2)",
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

    pub(crate) async fn delete_session(&self, user: &UserKey, session_id: &str) -> Result<bool> {
        Ok(self
            .delete_sessions(user, vec![session_id.to_owned()])
            .await?
            > 0)
    }

    /// Deletes sessions with their runs and returns how many existed.
    pub(crate) async fn delete_sessions(&self, user: &UserKey, ids: Vec<String>) -> Result<u64> {
        let mut tx = self.pool.begin().await?;
        for table in ["runs", "tool_failures", "memory_scores", "session_titles"] {
            sqlx::query(&format!(
                "DELETE FROM {table} WHERE user_id=$1 AND session_id = ANY($2)"
            ))
            .bind(user.as_str())
            .bind(&ids)
            .execute(&mut *tx)
            .await?;
        }
        let deleted = sqlx::query("DELETE FROM sessions WHERE user_id=$1 AND id = ANY($2)")
            .bind(user.as_str())
            .bind(&ids)
            .execute(&mut *tx)
            .await?
            .rows_affected();
        tx.commit().await?;
        Ok(deleted)
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

    pub(crate) async fn finish(
        &self,
        user: &UserKey,
        run: &Run,
        turn: Option<FinishedTurn<'_>>,
        status: RunStatus,
    ) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        if let Some(turn) = turn {
            sqlx::query(
                "INSERT INTO sessions (user_id, id, conversation, context, context_tokens, updated_at)
                 VALUES ($1, $2, $3, $4, $5, now())
                 ON CONFLICT (user_id, id) DO UPDATE SET conversation=excluded.conversation,
                     context=excluded.context, context_tokens=excluded.context_tokens,
                     updated_at=excluded.updated_at",
            )
            .bind(user.as_str())
            .bind(&run.session_id)
            .bind(serde_json::to_string(turn.transcript)?)
            .bind(serde_json::to_string(turn.context)?)
            .bind(i64::from(turn.prompt_tokens))
            .execute(&mut *tx)
            .await?;
        }
        sqlx::query("UPDATE runs SET status=$1 WHERE id=$2 AND user_id=$3")
            .bind(status.as_str())
            .bind(&run.id)
            .bind(user.as_str())
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    /// Records tool calls that failed, since the conversation keeps only their output.
    pub(crate) async fn record_tool_failures(
        &self,
        user: &UserKey,
        session: &str,
        call_ids: Vec<String>,
    ) -> Result<()> {
        if call_ids.is_empty() {
            return Ok(());
        }
        sqlx::query(
            "INSERT INTO tool_failures (user_id, session_id, call_id)
             SELECT $1, $2, unnest($3::text[]) ON CONFLICT DO NOTHING",
        )
        .bind(user.as_str())
        .bind(session)
        .bind(&call_ids)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Ids of the session's tool calls that failed.
    pub(crate) async fn tool_failures(
        &self,
        user: &UserKey,
        session: &str,
    ) -> Result<HashSet<String>> {
        let ids: Vec<String> = sqlx::query_scalar(
            "SELECT call_id FROM tool_failures WHERE user_id=$1 AND session_id=$2",
        )
        .bind(user.as_str())
        .bind(session)
        .fetch_all(&self.pool)
        .await?;
        Ok(ids.into_iter().collect())
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
        sqlx::query(
            "INSERT INTO memory_scores (user_id, session_id, turn_at, probability)
             VALUES ($1, $2, $3, $4)
             ON CONFLICT (user_id, session_id, turn_at) DO UPDATE SET probability=excluded.probability",
        )
        .bind(user.as_str())
        .bind(session)
        .bind(turn_at)
        .bind(probability)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// The session's memory scores by the time of each turn's user message.
    pub(crate) async fn memory_scores(
        &self,
        user: &UserKey,
        session: &str,
    ) -> Result<HashMap<i64, f64>> {
        let rows = sqlx::query(
            "SELECT turn_at, probability FROM memory_scores WHERE user_id=$1 AND session_id=$2",
        )
        .bind(user.as_str())
        .bind(session)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|r| Ok((r.try_get(0)?, r.try_get(1)?)))
            .collect()
    }

    pub(crate) async fn list_crons(&self, user: &UserKey) -> Result<Vec<Cron>> {
        let rows = sqlx::query(
            "SELECT id, label, prompt, schedule, enabled, created_at, run_once, last_run_at,
                 last_status, last_error
             FROM crons WHERE user_id=$1 ORDER BY created_at DESC LIMIT 100",
        )
        .bind(user.as_str())
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|r| {
                Ok(Cron {
                    id: r.try_get(0)?,
                    label: r.try_get(1)?,
                    prompt: r.try_get(2)?,
                    schedule: r.try_get(3)?,
                    enabled: r.try_get(4)?,
                    created_at: format_time(r.try_get(5)?),
                    run_once: r.try_get(6)?,
                    last_run_at: r.try_get::<Option<DateTime<Utc>>, _>(7)?.map(format_time),
                    last_status: r.try_get(8)?,
                    last_error: r.try_get(9)?,
                    next_run_at: None,
                })
            })
            .collect()
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
        sqlx::query(
            "INSERT INTO crons (user_id, id, label, prompt, schedule, run_once)
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(user.as_str())
        .bind(&id)
        .bind(label)
        .bind(prompt)
        .bind(schedule)
        .bind(run_once)
        .execute(&self.pool)
        .await?;
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
        let changed = sqlx::query(
            "UPDATE crons SET enabled=COALESCE($1, enabled), label=COALESCE($2, label),
                 schedule=COALESCE($3, schedule)
             WHERE user_id=$4 AND id=$5 AND ($1 IS NOT NULL OR $2 IS NOT NULL OR $3 IS NOT NULL)",
        )
        .bind(enabled)
        .bind(label)
        .bind(schedule)
        .bind(user.as_str())
        .bind(id)
        .execute(&self.pool)
        .await?
        .rows_affected();
        Ok(changed > 0)
    }

    pub(crate) async fn toggle_cron(&self, user: &UserKey, id: &str) -> Result<Option<bool>> {
        Ok(sqlx::query_scalar(
            "UPDATE crons SET enabled = NOT enabled WHERE user_id=$1 AND id=$2 RETURNING enabled",
        )
        .bind(user.as_str())
        .bind(id)
        .fetch_optional(&self.pool)
        .await?)
    }

    pub(crate) async fn delete_cron(&self, user: &UserKey, id: &str) -> Result<bool> {
        let deleted = sqlx::query("DELETE FROM crons WHERE user_id=$1 AND id=$2")
            .bind(user.as_str())
            .bind(id)
            .execute(&self.pool)
            .await?
            .rows_affected();
        Ok(deleted > 0)
    }

    pub(crate) async fn register_device(
        &self,
        user: &UserKey,
        device_id: &str,
        fcm_token: &str,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO devices (user_id, device_id, fcm_token) VALUES ($1, $2, $3)
             ON CONFLICT (user_id, device_id) DO UPDATE SET fcm_token=excluded.fcm_token,
                 created_at=now()",
        )
        .bind(user.as_str())
        .bind(device_id)
        .bind(fcm_token)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub(crate) async fn unregister_device(&self, user: &UserKey, device_id: &str) -> Result<bool> {
        let deleted = sqlx::query("DELETE FROM devices WHERE user_id=$1 AND device_id=$2")
            .bind(user.as_str())
            .bind(device_id)
            .execute(&self.pool)
            .await?
            .rows_affected();
        Ok(deleted > 0)
    }

    pub async fn due_crons(&self) -> Result<Vec<(String, Value)>> {
        let rows = sqlx::query(
            "SELECT user_id, id, label, prompt, schedule, run_once FROM crons WHERE enabled",
        )
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|r| {
                Ok((
                    r.try_get::<String, _>(0)?,
                    json!({
                        "id": r.try_get::<String, _>(1)?,
                        "label": r.try_get::<String, _>(2)?,
                        "prompt": r.try_get::<String, _>(3)?,
                        "schedule": r.try_get::<String, _>(4)?,
                        "run_once": r.try_get::<bool, _>(5)?,
                    }),
                ))
            })
            .collect()
    }

    pub(crate) async fn log_cron_start(
        &self,
        user: &UserKey,
        cron_id: &str,
        run_id: &str,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO cron_logs (user_id, cron_id, run_id, started_at, status)
             VALUES ($1, $2, $3, now(), 'running')",
        )
        .bind(user.as_str())
        .bind(cron_id)
        .bind(run_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn log_cron_finish(
        &self,
        run_id: &str,
        status: RunStatus,
        error: &str,
    ) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("UPDATE cron_logs SET finished_at=now(), status=$1, error=$2 WHERE run_id=$3")
            .bind(status.as_str())
            .bind(error)
            .bind(run_id)
            .execute(&mut *tx)
            .await?;
        // Denormalize last run info onto the crons row.
        sqlx::query(
            "UPDATE crons SET last_run_at=now(), last_status=$1, last_error=NULLIF($2, '')
             WHERE (user_id, id) IN (SELECT user_id, cron_id FROM cron_logs WHERE run_id=$3)",
        )
        .bind(status.as_str())
        .bind(error)
        .bind(run_id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    /// When the job last started, as `YYYY-MM-DD HH:MM:SS` in UTC.
    pub(crate) async fn cron_last_run(
        &self,
        user: &UserKey,
        cron_id: &str,
    ) -> Result<Option<String>> {
        let started: Option<DateTime<Utc>> = sqlx::query_scalar(
            "SELECT started_at FROM cron_logs WHERE user_id=$1 AND cron_id=$2
             ORDER BY id DESC LIMIT 1",
        )
        .bind(user.as_str())
        .bind(cron_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(started.map(|t| t.format("%Y-%m-%d %H:%M:%S").to_string()))
    }

    pub(crate) async fn device_tokens(&self, user: &UserKey) -> Result<Vec<String>> {
        Ok(
            sqlx::query_scalar("SELECT fcm_token FROM devices WHERE user_id=$1")
                .bind(user.as_str())
                .fetch_all(&self.pool)
                .await?,
        )
    }

    pub(crate) async fn list_mcp_connections(&self, user: &UserKey) -> Result<Vec<Integration>> {
        let rows = sqlx::query(
            "SELECT id, provider, label, status, created_at FROM mcp_connections
             WHERE user_id=$1 ORDER BY created_at DESC LIMIT 100",
        )
        .bind(user.as_str())
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|r| {
                Ok(Integration {
                    id: r.try_get(0)?,
                    provider: r.try_get(1)?,
                    label: r.try_get(2)?,
                    status: r.try_get(3)?,
                    created_at: format_time(r.try_get(4)?),
                })
            })
            .collect()
    }

    #[allow(dead_code)]
    pub(crate) async fn get_mcp_connection(
        &self,
        user: &UserKey,
        id: &str,
    ) -> Result<Option<Value>> {
        let row = sqlx::query(
            "SELECT id, provider, label, config, credentials, status FROM mcp_connections
             WHERE user_id=$1 AND id=$2",
        )
        .bind(user.as_str())
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        row.map(|r| {
            Ok(json!({
                "id": r.try_get::<String, _>(0)?,
                "provider": r.try_get::<String, _>(1)?,
                "label": r.try_get::<String, _>(2)?,
                "config": r.try_get::<String, _>(3)?,
                "credentials": r.try_get::<String, _>(4)?,
                "status": r.try_get::<String, _>(5)?,
            }))
        })
        .transpose()
    }

    pub(crate) async fn create_mcp_connection(
        &self,
        user: &UserKey,
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
        sqlx::query(
            "INSERT INTO mcp_connections (user_id, id, provider, label, config)
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(user.as_str())
        .bind(&id)
        .bind(provider)
        .bind(label)
        .bind(config)
        .execute(&self.pool)
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
        let changed = sqlx::query(
            "UPDATE mcp_connections SET credentials=$1, status=$2 WHERE user_id=$3 AND id=$4",
        )
        .bind(credentials)
        .bind(status)
        .bind(user.as_str())
        .bind(id)
        .execute(&self.pool)
        .await?
        .rows_affected();
        Ok(changed > 0)
    }

    pub(crate) async fn delete_mcp_connection(&self, user: &UserKey, id: &str) -> Result<bool> {
        let deleted = sqlx::query("DELETE FROM mcp_connections WHERE user_id=$1 AND id=$2")
            .bind(user.as_str())
            .bind(id)
            .execute(&self.pool)
            .await?
            .rows_affected();
        Ok(deleted > 0)
    }

    /// The user's server-managed MCP servers.
    pub(crate) async fn mcp_servers(&self, user: &UserKey) -> Result<Vec<McpServerRow>> {
        let rows = sqlx::query(
            "SELECT name, config, credentials FROM mcp_servers WHERE user_id=$1 ORDER BY name",
        )
        .bind(user.as_str())
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|r| {
                Ok(McpServerRow {
                    name: r.try_get(0)?,
                    config: r.try_get(1)?,
                    credentials: r.try_get(2)?,
                })
            })
            .collect()
    }

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

    /// Saves a server's config. Credentials are untouched: they change only
    /// through `save_mcp_credentials`, so a stale config cannot roll back a
    /// rotated refresh token.
    pub(crate) async fn save_mcp_config(
        &self,
        user: &UserKey,
        name: &str,
        config: Value,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO mcp_servers (user_id, name, config) VALUES ($1, $2, $3)
             ON CONFLICT (user_id, name) DO UPDATE SET config=excluded.config",
        )
        .bind(user.as_str())
        .bind(name)
        .bind(config)
        .execute(&self.pool)
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
        let credentials = (!credentials.is_null()).then_some(credentials);
        sqlx::query(
            "INSERT INTO mcp_servers (user_id, name, credentials) VALUES ($1, $2, $3)
             ON CONFLICT (user_id, name) DO UPDATE SET credentials=excluded.credentials",
        )
        .bind(user.as_str())
        .bind(name)
        .bind(credentials)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub(crate) async fn delete_mcp_server(&self, user: &UserKey, name: &str) -> Result<bool> {
        let deleted = sqlx::query("DELETE FROM mcp_servers WHERE user_id=$1 AND name=$2")
            .bind(user.as_str())
            .bind(name)
            .execute(&self.pool)
            .await?
            .rows_affected();
        Ok(deleted > 0)
    }

    pub(crate) async fn update_location(&self, user: &UserKey, fix: &Location) -> Result<()> {
        sqlx::query(
            "INSERT INTO locations (user_id, latitude, longitude, accuracy, timezone, place, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7)
             ON CONFLICT (user_id) DO UPDATE SET latitude=excluded.latitude,
                 longitude=excluded.longitude, accuracy=excluded.accuracy,
                 timezone=excluded.timezone, place=excluded.place, updated_at=excluded.updated_at",
        )
        .bind(user.as_str())
        .bind(fix.latitude)
        .bind(fix.longitude)
        .bind(fix.accuracy)
        .bind(&fix.timezone)
        .bind(&fix.place)
        .bind(fix.updated_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub(crate) async fn get_location(&self, user: &UserKey) -> Result<Option<Location>> {
        let row = sqlx::query(
            "SELECT latitude, longitude, accuracy, timezone, place, updated_at FROM locations
             WHERE user_id=$1",
        )
        .bind(user.as_str())
        .fetch_optional(&self.pool)
        .await?;
        row.map(|r| {
            Ok(Location {
                latitude: r.try_get(0)?,
                longitude: r.try_get(1)?,
                accuracy: r.try_get(2)?,
                timezone: r.try_get(3)?,
                place: r.try_get(4)?,
                updated_at: r.try_get(5)?,
            })
        })
        .transpose()
    }

    pub(crate) async fn get_timezone(&self, user: &UserKey) -> Result<String> {
        let timezone: Option<String> =
            sqlx::query_scalar("SELECT timezone FROM locations WHERE user_id=$1")
                .bind(user.as_str())
                .fetch_optional(&self.pool)
                .await?;
        Ok(timezone.unwrap_or_else(|| "UTC".into()))
    }

    pub(crate) async fn save_notifications(
        &self,
        user: &UserKey,
        notifications: Vec<PhoneNotification>,
    ) -> Result<usize> {
        let mut tx = self.pool.begin().await?;
        let mut count = 0;
        for n in &notifications {
            if n.app.is_empty() || n.timestamp == 0 {
                continue;
            }
            let inserted = sqlx::query(
                "INSERT INTO notifications (id, user_id, app, title, text, timestamp)
                 VALUES ($1, $2, $3, $4, $5, $6) ON CONFLICT DO NOTHING",
            )
            .bind(Uuid::new_v4().to_string())
            .bind(user.as_str())
            .bind(&n.app)
            .bind(&n.title)
            .bind(&n.text)
            .bind(n.timestamp)
            .execute(&mut *tx)
            .await?
            .rows_affected();
            count += usize::try_from(inserted)?;
        }
        tx.commit().await?;
        Ok(count)
    }

    pub(crate) async fn query_notifications(
        &self,
        user: &UserKey,
        app: Option<&str>,
        since_minutes: Option<i64>,
        keyword: Option<&str>,
        limit: Option<i64>,
    ) -> Result<Vec<Value>> {
        let since_minutes = since_minutes.unwrap_or(60);
        let limit = limit.unwrap_or(100).min(500);
        let cutoff_ms = Utc::now().timestamp_millis() - since_minutes * 60 * 1000;
        let rows = sqlx::query(
            "SELECT app, title, text, timestamp FROM notifications
             WHERE user_id=$1 AND timestamp>=$2
               AND ($3::text IS NULL OR app=$3 OR strpos(lower(app), lower($3)) > 0)
               AND ($4::text IS NULL OR strpos(lower(title), lower($4)) > 0
                    OR strpos(lower(text), lower($4)) > 0)
             ORDER BY timestamp DESC LIMIT $5",
        )
        .bind(user.as_str())
        .bind(cutoff_ms)
        .bind(app)
        .bind(keyword)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|r| {
                Ok(json!({
                    "app": r.try_get::<String, _>(0)?,
                    "title": r.try_get::<Option<String>, _>(1)?,
                    "text": r.try_get::<Option<String>, _>(2)?,
                    "timestamp": r.try_get::<i64, _>(3)?,
                }))
            })
            .collect()
    }

    pub async fn purge_old_notifications(&self) -> Result<u64> {
        let cutoff_ms = Utc::now().timestamp_millis() - 7 * 24 * 60 * 60 * 1000;
        Ok(
            sqlx::query("DELETE FROM notifications WHERE timestamp < $1")
                .bind(cutoff_ms)
                .execute(&self.pool)
                .await?
                .rows_affected(),
        )
    }

    pub(crate) async fn create_habit(
        &self,
        user: &UserKey,
        name: &str,
        frequency: &str,
    ) -> Result<String> {
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
        sqlx::query("INSERT INTO habits (id, user_id, name, frequency) VALUES ($1, $2, $3, $4)")
            .bind(&id)
            .bind(user.as_str())
            .bind(name)
            .bind(frequency)
            .execute(&self.pool)
            .await?;
        Ok(id)
    }

    pub(crate) async fn list_habits(&self, user: &UserKey) -> Result<Vec<Value>> {
        let rows = sqlx::query(
            "SELECT h.id, h.name, h.frequency, h.created_at,
                    (SELECT COUNT(*) FROM habit_logs l WHERE l.habit_id=h.id)
             FROM habits h
             WHERE h.user_id=$1 AND NOT h.archived
             ORDER BY h.created_at",
        )
        .bind(user.as_str())
        .fetch_all(&self.pool)
        .await?;
        let mut result = Vec::with_capacity(rows.len());
        for r in rows {
            let habit_id: String = r.try_get(0)?;
            result.push(json!({
                "name": r.try_get::<String, _>(1)?,
                "frequency": r.try_get::<String, _>(2)?,
                "created_at": format_time(r.try_get(3)?),
                "total_logs": r.try_get::<i64, _>(4)?,
                "current_streak": self.habit_streak(&habit_id).await?,
            }));
        }
        Ok(result)
    }

    pub(crate) async fn log_habit(
        &self,
        user: &UserKey,
        habit_name: &str,
        value: f64,
        note: Option<&str>,
    ) -> Result<()> {
        let inserted = sqlx::query(
            "INSERT INTO habit_logs (id, habit_id, user_id, value, note)
             SELECT $1, id, user_id, $4, $5 FROM habits
             WHERE user_id=$2 AND name=$3 AND NOT archived",
        )
        .bind(Uuid::new_v4().to_string())
        .bind(user.as_str())
        .bind(habit_name)
        .bind(value)
        .bind(note)
        .execute(&self.pool)
        .await?
        .rows_affected();
        if inserted == 0 {
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
        let row = sqlx::query(
            "SELECT id, name, frequency, created_at FROM habits
             WHERE user_id=$1 AND name=$2 AND NOT archived",
        )
        .bind(user.as_str())
        .bind(habit_name)
        .fetch_optional(&self.pool)
        .await?;
        let Some(row) = row else {
            return Ok(json!({"error": "habit not found"}));
        };
        let habit_id: String = row.try_get(0)?;
        let since = Utc::now()
            .date_naive()
            .checked_sub_days(Days::new(days.max(0).unsigned_abs()))
            .unwrap_or(NaiveDate::MIN);
        let logs = sqlx::query(
            "SELECT (logged_at AT TIME ZONE 'UTC')::date AS day, SUM(value),
                    string_agg(note, '; ')
             FROM habit_logs
             WHERE habit_id=$1 AND (logged_at AT TIME ZONE 'UTC')::date >= $2
             GROUP BY day ORDER BY day",
        )
        .bind(&habit_id)
        .bind(since)
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(|r| {
            Ok(json!({
                "date": r.try_get::<NaiveDate, _>(0)?.format("%Y-%m-%d").to_string(),
                "total": r.try_get::<f64, _>(1)?,
                "notes": r.try_get::<Option<String>, _>(2)?,
            }))
        })
        .collect::<Result<Vec<_>>>()?;

        let logged_days = logs.len() as f64;
        let rate = if days > 0 {
            logged_days / days as f64
        } else {
            0.0
        };
        Ok(json!({
            "name": row.try_get::<String, _>(1)?,
            "frequency": row.try_get::<String, _>(2)?,
            "created_at": format_time(row.try_get(3)?),
            "days_queried": days,
            "logged_days": logged_days as i64,
            "completion_rate": (rate * 100.0).round() / 100.0,
            "current_streak": self.habit_streak(&habit_id).await?,
            "logs": logs,
        }))
    }

    /// Consecutive days with at least one log, going back from today.
    async fn habit_streak(&self, habit_id: &str) -> Result<i64> {
        let days: Vec<NaiveDate> = sqlx::query_scalar(
            "SELECT DISTINCT (logged_at AT TIME ZONE 'UTC')::date AS day FROM habit_logs
             WHERE habit_id=$1 ORDER BY day DESC",
        )
        .bind(habit_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(streak(&days, Utc::now().date_naive()))
    }

    pub(crate) async fn archive_habit(&self, user: &UserKey, habit_name: &str) -> Result<bool> {
        let changed = sqlx::query(
            "UPDATE habits SET archived=TRUE WHERE user_id=$1 AND name=$2 AND NOT archived",
        )
        .bind(user.as_str())
        .bind(habit_name)
        .execute(&self.pool)
        .await?
        .rows_affected();
        Ok(changed > 0)
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
        Ok(sqlx::query_scalar(
            "SELECT DISTINCT user_id FROM sessions UNION SELECT DISTINCT user_id FROM devices",
        )
        .fetch_all(&self.pool)
        .await?)
    }

    /// Count notifications for a user received in the last `minutes` minutes.
    pub(crate) async fn notification_count_since(
        &self,
        user: &UserKey,
        minutes: i64,
    ) -> Result<i64> {
        let cutoff_ms = Utc::now().timestamp_millis() - minutes * 60 * 1000;
        Ok(sqlx::query_scalar(
            "SELECT COUNT(*) FROM notifications WHERE user_id=$1 AND timestamp >= $2",
        )
        .bind(user.as_str())
        .bind(cutoff_ms)
        .fetch_one(&self.pool)
        .await?)
    }
}

fn session_ref(row: &PgRow) -> Result<SessionRef> {
    Ok(SessionRef {
        user: row.try_get(0)?,
        id: row.try_get(1)?,
    })
}

/// Consecutive days in `days` (distinct, newest first) ending today, or
/// ending yesterday when nothing was logged today yet.
fn streak(days: &[NaiveDate], today: NaiveDate) -> i64 {
    let Some(&newest) = days.first() else {
        return 0;
    };
    if newest != today && Some(newest) != today.pred_opt() {
        return 0;
    }
    let mut expected = newest;
    let mut streak = 0;
    for &day in days {
        if day != expected {
            break;
        }
        streak += 1;
        let Some(previous) = day.pred_opt() else {
            break;
        };
        expected = previous;
    }
    streak
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn streak_counts_back_from_today_or_yesterday() {
        let day = |d: u32| NaiveDate::from_ymd_opt(2026, 3, d).unwrap();
        assert_eq!(streak(&[], day(10)), 0);
        assert_eq!(streak(&[day(10), day(9), day(7)], day(10)), 2);
        assert_eq!(streak(&[day(9), day(8)], day(10)), 2);
        assert_eq!(streak(&[day(8), day(7)], day(10)), 0);
    }

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
