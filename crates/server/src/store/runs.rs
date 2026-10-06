//! Agent runs: creating them once per request and saving how they ended.

use super::Store;
use crate::{protocol::Run, user::UserKey};
use anyhow::Result;
use chrono::{DateTime, Utc};
use flashmind::core::Conversation;
use sqlx::Row;
use uuid::Uuid;

pub enum CreateRun {
    Created(Run),
    Existing(Run),
    Conflict,
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

/// How an agent run ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunStatus {
    Completed,
    Cancelled,
    /// Cut short by the server shutting down, as on a deploy.
    Interrupted,
    TimedOut,
    Stalled,
    Looped,
    Failed,
    /// Ended on a tool call that waits for the user's OK.
    Waiting,
}

impl RunStatus {
    /// The name stored in the database and sent to clients.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Cancelled => "cancelled",
            Self::Interrupted => "interrupted",
            Self::TimedOut => "timed_out",
            Self::Stalled => "stalled",
            Self::Looped => "looped",
            Self::Failed => "failed",
            Self::Waiting => "waiting",
        }
    }
}

impl Store {
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
    pub(crate) async fn run_for_request(
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

    /// Ends `run` with `status`, saving `turn` when there is one. Returns the
    /// session's new `updated_at`, `None` when no turn was saved.
    pub(crate) async fn finish(
        &self,
        user: &UserKey,
        run: &Run,
        turn: Option<FinishedTurn<'_>>,
        status: RunStatus,
    ) -> Result<Option<DateTime<Utc>>> {
        let mut tx = self.pool.begin().await?;
        let mut updated_at = None;
        if let Some(turn) = turn {
            let saved: DateTime<Utc> = sqlx::query_scalar(
                "INSERT INTO sessions (user_id, id, conversation, context, context_tokens, updated_at)
                 VALUES ($1, $2, $3, $4, $5, now())
                 ON CONFLICT (user_id, id) DO UPDATE SET conversation=excluded.conversation,
                     context=excluded.context, context_tokens=excluded.context_tokens,
                     updated_at=excluded.updated_at
                 RETURNING updated_at",
            )
            .bind(user.as_str())
            .bind(&run.session_id)
            .bind(serde_json::to_string(turn.transcript)?)
            .bind(serde_json::to_string(turn.context)?)
            .bind(i64::from(turn.prompt_tokens))
            .fetch_one(&mut *tx)
            .await?;
            updated_at = Some(saved);
        }
        sqlx::query("UPDATE runs SET status=$1 WHERE id=$2 AND user_id=$3")
            .bind(status.as_str())
            .bind(&run.id)
            .bind(user.as_str())
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(updated_at)
    }
}
