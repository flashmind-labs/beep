//! Agent runs: creating them once per request and saving how they ended.

use super::{
    Store,
    alerts::clear_alert,
    chats::{SessionChat, ensure_session, touch},
    messages::{Step, TurnRecord, drop_approvals, insert_steps, latest_turn, patch_turn},
    updates::{Bumped, Readers, UpdateOp, log_chat, log_messages},
};
use crate::{account::user::UserKey, protocol::Run};
use anyhow::Result;
use chrono::{DateTime, Utc};
use flashmind::core::Conversation;
use sqlx::{PgConnection, Row, postgres::PgRow};
use uuid::Uuid;

pub enum CreateRun {
    Created(Run),
    Existing(Run),
    Conflict,
}

/// What a finished run saves for its session.
pub struct FinishedTurn<'a> {
    /// What the model sees next turn.
    pub context: &'a Conversation,
    /// Prompt size of the run's last model call; `None` keeps the chat's count.
    pub prompt_tokens: Option<u32>,
    /// The turn's steps as apps show them; steps before the first opener carry on the latest turn.
    pub steps: &'a [Step],
    /// Whether non-opening steps stand alone (a routine notice) instead of carrying on the latest turn.
    pub stands_alone: bool,
    /// Added to the carried-on turn: its reasoning and a resumed turn's memory score.
    pub carried: TurnRecord,
}

/// What `Store::finish` saved of a turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SavedTurn {
    /// The session's new `updated_at`.
    pub updated_at: DateTime<Utc>,
    /// The message the saved steps opened their turn with, if they did.
    pub opened: Option<i64>,
    /// The message that opened the steps' turn: `opened`, or the carried-on turn.
    pub turn: Option<i64>,
}

/// How an agent run ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
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
        self.into()
    }
}

fn run(row: &PgRow) -> Result<Run> {
    Ok(Run {
        id: row.try_get(0)?,
        session_id: row.try_get::<Uuid, _>(1)?.to_string(),
        status: row.try_get(2)?,
    })
}

impl Store {
    // Called once before accepting requests. A server restart never silently repeats tools.
    pub async fn recover(&self) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        let rows = sqlx::query(
            "UPDATE runs SET status='interrupted' WHERE status='running' RETURNING chat_id, prompt_id",
        )
        .fetch_all(&mut *tx)
        .await?;
        // Their turns were never saved, so the prompts shown meanwhile go too.
        let mut bumped = Vec::new();
        for row in rows {
            if let Some(prompt) = row.try_get::<Option<i64>, _>(1)? {
                bumped.extend(drop_prompt(&mut tx, row.try_get(0)?, prompt).await?);
            }
        }
        tx.commit().await?;
        self.announce(&bumped);
        // Their teams died with the last server too.
        self.interrupt_researches().await
    }

    /// Saves the prompt of just-started `run` so every device shows it until the turn replaces it.
    pub(crate) async fn save_prompt(&self, user: &UserKey, run: &Run, prompt: &str) -> Result<()> {
        let chat = Uuid::parse_str(&run.session_id)?;
        let mut tx = self.pool.begin().await?;
        let id: i64 = sqlx::query_scalar(
            "WITH next AS (SELECT nextval(pg_get_serial_sequence('messages', 'id')) AS id)
             INSERT INTO messages (id, chat_id, role, turn_id, author_id, content, sent_at)
             SELECT id, $1, 'user', id, $2, $3, now() FROM next RETURNING id",
        )
        .bind(chat)
        .bind(user.as_str())
        .bind(prompt)
        .fetch_one(&mut *tx)
        .await?;
        let running = sqlx::query(
            "UPDATE runs SET prompt_id = $1 WHERE id = $2 AND user_id = $3 AND status = 'running'",
        )
        .bind(id)
        .bind(&run.id)
        .bind(user.as_str())
        .execute(&mut *tx)
        .await?
        .rows_affected();
        // Finished already: its turn is saved, or nothing is.
        if running == 0 {
            tx.rollback().await?;
            return Ok(());
        }
        touch(&mut tx, chat).await?;
        let bumped =
            log_messages(&mut tx, chat, &[id], UpdateOp::Message, Readers::Members).await?;
        tx.commit().await?;
        self.announce(&bumped);
        Ok(())
    }

    /// The run going on in session `chat`, if any.
    pub(crate) async fn active_run(&self, user: &UserKey, chat: Uuid) -> Result<Option<Run>> {
        let row = sqlx::query(
            "SELECT id, chat_id, status FROM runs
             WHERE user_id = $1 AND chat_id = $2 AND status = 'running'",
        )
        .bind(user.as_str())
        .bind(chat)
        .fetch_optional(&self.pool)
        .await?;
        row.as_ref().map(run).transpose()
    }

    /// The runs going on in any of `user`'s sessions, oldest first.
    pub(crate) async fn active_runs(&self, user: &UserKey) -> Result<Vec<Run>> {
        let rows = sqlx::query(
            "SELECT id, chat_id, status FROM runs
             WHERE user_id = $1 AND status = 'running' ORDER BY seq",
        )
        .bind(user.as_str())
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(run).collect()
    }

    /// Starts a run in session `chat`, creating it on first run; conflict if not the user's or busy.
    pub(crate) async fn create_run(
        &self,
        user: &UserKey,
        chat: Uuid,
        request: &str,
        message: &str,
    ) -> Result<CreateRun> {
        let user = user.as_str();
        if let Some(found) = self
            .run_for_request(user, chat, request, Some(message))
            .await?
        {
            return Ok(found);
        }
        let mut tx = self.pool.begin().await?;
        let session = ensure_session(&mut tx, user, chat).await?;
        if session == SessionChat::Taken {
            tx.rollback().await?;
            return Ok(CreateRun::Conflict);
        }
        let busy: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM runs WHERE chat_id=$1 AND status='running')",
        )
        .bind(chat)
        .fetch_one(&mut *tx)
        .await?;
        if busy {
            tx.rollback().await?;
            return Ok(CreateRun::Conflict);
        }
        let active: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM runs WHERE status='running'")
            .fetch_one(&mut *tx)
            .await?;
        if active >= 16 {
            tx.rollback().await?;
            return Ok(CreateRun::Conflict);
        }
        let run = Run {
            id: Uuid::new_v4().to_string(),
            session_id: chat.to_string(),
            status: "running".into(),
        };
        // A concurrent run for the same request or session wins the race
        // through the unique indexes; this one then reports what it lost to.
        let inserted = sqlx::query(
            "INSERT INTO runs (id, user_id, chat_id, request_id, message, status)
             VALUES ($1, $2, $3, $4, $5, 'running') ON CONFLICT DO NOTHING",
        )
        .bind(&run.id)
        .bind(user)
        .bind(chat)
        .bind(request)
        .bind(message)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        // A new session shows up in the user's list right away, titled by
        // this first message.
        let bumped = if inserted == 1 && session == SessionChat::New {
            log_chat(&mut tx, chat, UpdateOp::Chat, Readers::User(user)).await?
        } else {
            Vec::new()
        };
        // A reply in an alert makes it a normal chat.
        if inserted == 1 && session == SessionChat::Owned {
            clear_alert(&mut tx, chat).await?;
        }
        tx.commit().await?;
        self.announce(&bumped);
        if inserted == 1 {
            return Ok(CreateRun::Created(run));
        }
        Ok(self
            .run_for_request(user, chat, request, Some(message))
            .await?
            .unwrap_or(CreateRun::Conflict))
    }

    /// The run already made for `request`, or a conflict if the id was reused; `message: None` checks only the chat.
    pub(crate) async fn run_for_request(
        &self,
        user: &str,
        chat: Uuid,
        request: &str,
        message: Option<&str>,
    ) -> Result<Option<CreateRun>> {
        let row = sqlx::query(
            "SELECT id, chat_id, status, message FROM runs WHERE user_id=$1 AND request_id=$2",
        )
        .bind(user)
        .bind(request)
        .fetch_optional(&self.pool)
        .await?;
        let Some(row) = row else {
            return Ok(None);
        };
        let original: String = row.try_get(3)?;
        let same = row.try_get::<Uuid, _>(1)? == chat && message.is_none_or(|m| original == m);
        let run = run(&row)?;
        Ok(Some(if same {
            CreateRun::Existing(run)
        } else {
            CreateRun::Conflict
        }))
    }

    pub(crate) async fn run(&self, user: &UserKey, id: &str) -> Result<Option<Run>> {
        let row = sqlx::query("SELECT id, chat_id, status FROM runs WHERE user_id=$1 AND id=$2")
            .bind(user.as_str())
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        row.as_ref().map(run).transpose()
    }

    /// Ends `run` with `status`, saving `turn` if any; `None` when nothing was saved.
    pub(crate) async fn finish(
        &self,
        user: &UserKey,
        run: &Run,
        turn: Option<FinishedTurn<'_>>,
        status: RunStatus,
    ) -> Result<Option<SavedTurn>> {
        let chat = Uuid::parse_str(&run.session_id)?;
        let mut tx = self.pool.begin().await?;
        let mut saved = None;
        let mut bumped = Vec::new();
        // The prompt shown while it ran gives way to the turn, or to nothing.
        let prompt: Option<i64> =
            sqlx::query_scalar("SELECT prompt_id FROM runs WHERE id = $1 AND user_id = $2")
                .bind(&run.id)
                .bind(user.as_str())
                .fetch_optional(&mut *tx)
                .await?
                .flatten();
        if let Some(prompt) = prompt {
            bumped.extend(drop_prompt(&mut tx, chat, prompt).await?);
            if turn.is_none() {
                touch(&mut tx, chat).await?;
            }
        }
        if let Some(turn) = turn {
            sqlx::query(
                "INSERT INTO sessions (chat_id, context, context_tokens) VALUES ($1, $2, COALESCE($3, 0))
                 ON CONFLICT (chat_id) DO UPDATE SET context=excluded.context,
                     context_tokens=COALESCE($3, sessions.context_tokens)",
            )
            .bind(chat)
            .bind(serde_json::to_string(turn.context)?)
            .bind(turn.prompt_tokens.map(i64::from))
            .execute(&mut *tx)
            .await?;
            // Whatever the turn did answered or skipped the cards still
            // asking; a turn that shows nothing did neither.
            if !turn.steps.is_empty() {
                bumped.extend(drop_approvals(&mut tx, chat).await?);
            }
            let opens = turn
                .steps
                .first()
                .is_some_and(|step| step.body.opens_turn());
            let carried = if opens || turn.stands_alone {
                None
            } else {
                latest_turn(&mut tx, chat).await?
            };
            let ids = insert_steps(&mut tx, chat, user.as_str(), turn.steps, carried).await?;
            let opened = turn
                .steps
                .iter()
                .zip(&ids)
                .find(|(step, _)| step.body.opens_turn())
                .map(|(_, id)| *id);
            bumped.extend(
                log_messages(&mut tx, chat, &ids, UpdateOp::Message, Readers::Members).await?,
            );
            if let Some(carried) = carried
                && turn.carried != TurnRecord::default()
            {
                bumped.extend(patch_turn(&mut tx, chat, carried, &turn.carried).await?);
            }
            // A turn that shows nothing, like a quiet routine's, leaves the
            // chat where it was in the list.
            if !turn.steps.is_empty() || prompt.is_some() {
                touch(&mut tx, chat).await?;
            }
            let updated_at: DateTime<Utc> =
                sqlx::query_scalar("SELECT updated_at FROM chats WHERE id = $1")
                    .bind(chat)
                    .fetch_one(&mut *tx)
                    .await?;
            saved = Some(SavedTurn {
                updated_at,
                opened,
                turn: opened.or(carried),
            });
        }
        sqlx::query("UPDATE runs SET status=$1 WHERE id=$2 AND user_id=$3")
            .bind(status.as_str())
            .bind(&run.id)
            .bind(user.as_str())
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        self.announce(&bumped);
        Ok(saved)
    }
}

/// Deletes the prompt message `prompt` of `chat` and logs it gone.
async fn drop_prompt(tx: &mut PgConnection, chat: Uuid, prompt: i64) -> Result<Vec<Bumped>> {
    let gone: Vec<i64> = sqlx::query_scalar("DELETE FROM messages WHERE id = $1 RETURNING id")
        .bind(prompt)
        .fetch_all(&mut *tx)
        .await?;
    log_messages(tx, chat, &gone, UpdateOp::Deleted, Readers::Members).await
}
