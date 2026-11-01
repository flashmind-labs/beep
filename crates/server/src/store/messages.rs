//! The messages the apps show, one row per step of a chat: a user message,
//! a tool call, a reply, an approval card, a group message. A session turn's
//! steps point at the message that opened it, which keeps what the turn ran
//! with in `turns`.

use super::{
    Store,
    updates::{Bumped, Readers, UpdateOp, log_messages},
};
use crate::protocol::{
    Approval, ApprovalField, AutoPick, ChatMessage, ImageRef, MessageBody, ToolLabel, TurnInfo,
};
use anyhow::{Result, bail};
use chrono::{DateTime, Utc};
use flashmind::types::ReasoningLevel;
use sqlx::{PgConnection, Row, postgres::PgRow};
use std::collections::HashMap;
use uuid::Uuid;

/// What a message is, as `messages.role` names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    User,
    Assistant,
    Tool,
    Scheduled,
    Approval,
    Member,
    Beep,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Assistant => "assistant",
            Self::Tool => "tool",
            Self::Scheduled => "scheduled",
            Self::Approval => "approval",
            Self::Member => "member",
            Self::Beep => "beep",
        }
    }

    fn parse(role: &str) -> Result<Self> {
        Ok(match role {
            "user" => Self::User,
            "assistant" => Self::Assistant,
            "tool" => Self::Tool,
            "scheduled" => Self::Scheduled,
            "approval" => Self::Approval,
            "member" => Self::Member,
            "beep" => Self::Beep,
            _ => bail!("unknown message role {role}"),
        })
    }
}

/// What a turn ran with.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TurnRecord {
    /// Empty when not recorded.
    pub model: String,
    /// What Jev picked the thinking at, when it picked the model.
    pub auto_thinking: Option<ReasoningLevel>,
    /// Every step's reasoning, oldest first.
    pub reasoning: String,
    pub memory_score: Option<f64>,
}

impl TurnRecord {
    fn to_wire(&self) -> TurnInfo {
        TurnInfo {
            model: self.model.clone(),
            auto: self.auto_thinking.map(|thinking| AutoPick {
                model: self.model.clone(),
                thinking,
            }),
            reasoning: self.reasoning.clone(),
            memory_score: self.memory_score,
        }
    }
}

/// One step of a session turn to save.
#[derive(Debug, Clone, PartialEq)]
pub struct Step {
    pub sent_at: DateTime<Utc>,
    pub body: StepBody,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StepBody {
    /// Opens a turn.
    User {
        content: String,
        turn: TurnRecord,
    },
    /// Opens a turn a routine or an event started.
    Scheduled {
        label: String,
        turn: TurnRecord,
    },
    Assistant {
        content: String,
    },
    Tool {
        call_id: String,
        name: String,
        output: String,
        success: bool,
        /// What it showed while it ran, such as "Recalling memories".
        /// Empty when not recorded.
        description: String,
    },
    /// The turn stopped to ask before a tool call.
    Approval {
        query_id: String,
        approval: Approval,
    },
}

impl StepBody {
    fn role(&self) -> Role {
        match self {
            Self::User { .. } => Role::User,
            Self::Scheduled { .. } => Role::Scheduled,
            Self::Assistant { .. } => Role::Assistant,
            Self::Tool { .. } => Role::Tool,
            Self::Approval { .. } => Role::Approval,
        }
    }

    /// Whether this step opens a turn.
    pub fn opens_turn(&self) -> bool {
        self.turn().is_some()
    }

    fn turn(&self) -> Option<&TurnRecord> {
        match self {
            Self::User { turn, .. } | Self::Scheduled { turn, .. } => Some(turn),
            _ => None,
        }
    }

    fn content(&self) -> &str {
        match self {
            Self::User { content, .. } | Self::Assistant { content } => content,
            Self::Scheduled { label, .. } => label,
            Self::Tool { .. } | Self::Approval { .. } => "",
        }
    }
}

/// A stored message with what its role adds.
#[derive(Debug, Clone, PartialEq)]
pub struct StoredMessage {
    pub id: i64,
    pub chat_id: Uuid,
    pub turn_id: Option<i64>,
    pub sent_at: DateTime<Utc>,
    /// Who wrote it; `None` for Beep and its steps.
    pub author_id: Option<String>,
    pub body: StoredBody,
    /// The pictures sent with it, in order.
    pub images: Vec<ImageRef>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StoredBody {
    User {
        content: String,
        turn: TurnRecord,
    },
    Scheduled {
        label: String,
        turn: TurnRecord,
    },
    Assistant {
        content: String,
    },
    Tool {
        call_id: String,
        name: String,
        output: String,
        success: bool,
        /// What it showed while it ran, such as "Recalling memories".
        /// Empty when not recorded.
        description: String,
    },
    Approval {
        query_id: String,
        approval: Approval,
    },
    /// A group member's message.
    Member {
        name: String,
        content: String,
    },
    /// Beep's reply in a group.
    Beep {
        content: String,
    },
}

impl From<StepBody> for StoredBody {
    fn from(step: StepBody) -> Self {
        match step {
            StepBody::User { content, turn } => Self::User { content, turn },
            StepBody::Scheduled { label, turn } => Self::Scheduled { label, turn },
            StepBody::Assistant { content } => Self::Assistant { content },
            StepBody::Tool {
                call_id,
                name,
                output,
                success,
                description,
            } => Self::Tool {
                call_id,
                name,
                output,
                success,
                description,
            },
            StepBody::Approval { query_id, approval } => Self::Approval { query_id, approval },
        }
    }
}

impl StoredMessage {
    /// In milliseconds since the epoch.
    pub fn timestamp(&self) -> i64 {
        self.sent_at.timestamp_millis()
    }

    /// The message as `viewer` reads it.
    pub fn to_wire(&self, viewer: &str) -> ChatMessage {
        let body = match &self.body {
            StoredBody::User { content, turn } => MessageBody::User {
                content: content.clone(),
                images: self.images.clone(),
                turn: turn.to_wire(),
            },
            StoredBody::Scheduled { label, turn } => MessageBody::Scheduled {
                label: label.clone(),
                turn: turn.to_wire(),
            },
            StoredBody::Assistant { content } => MessageBody::Assistant {
                content: content.clone(),
            },
            StoredBody::Tool {
                call_id,
                name,
                output,
                success,
                ..
            } => MessageBody::Tool {
                call_id: call_id.clone(),
                name: name.clone(),
                output: output.clone(),
                success: *success,
            },
            StoredBody::Approval { query_id, approval } => MessageBody::Approval {
                query_id: query_id.clone(),
                approval: approval.clone(),
            },
            StoredBody::Member { name, content } => MessageBody::Member {
                name: name.clone(),
                you: self.author_id.as_deref() == Some(viewer),
                content: content.clone(),
                images: self.images.clone(),
            },
            StoredBody::Beep { content } => MessageBody::Beep {
                content: content.clone(),
            },
        };
        ChatMessage {
            id: self.id,
            turn_id: self.turn_id,
            timestamp: self.timestamp(),
            body,
        }
    }

    /// The text a search matches, or a share keeps.
    pub fn text(&self) -> &str {
        match &self.body {
            StoredBody::User { content, .. }
            | StoredBody::Assistant { content }
            | StoredBody::Member { content, .. }
            | StoredBody::Beep { content } => content,
            StoredBody::Scheduled { label, .. } => label,
            StoredBody::Tool { .. } | StoredBody::Approval { .. } => "",
        }
    }
}

/// The columns `stored` reads, from `messages m` and its side tables.
const COLUMNS: &str = "m.id, m.chat_id, m.turn_id, m.sent_at, m.author_id, m.author_name,
    m.role::text, m.content, t.model, t.auto_thinking::text, t.reasoning, t.memory_score,
    c.call_id, c.name, c.output, c.success, a.query_id, a.app, a.action,
    c.description";

const JOINS: &str = "messages m
    LEFT JOIN turns t ON t.message_id = m.id
    LEFT JOIN tool_calls c ON c.message_id = m.id
    LEFT JOIN approvals a ON a.message_id = m.id";

fn turn_record(row: &PgRow) -> Result<TurnRecord> {
    let thinking: Option<String> = row.try_get(9)?;
    Ok(TurnRecord {
        model: row.try_get::<Option<String>, _>(8)?.unwrap_or_default(),
        auto_thinking: thinking.and_then(|level| level.parse().ok()),
        reasoning: row.try_get::<Option<String>, _>(10)?.unwrap_or_default(),
        memory_score: row.try_get(11)?,
    })
}

fn stored(row: &PgRow) -> Result<StoredMessage> {
    let role = Role::parse(row.try_get(6)?)?;
    let content: String = row.try_get(7)?;
    let body = match role {
        Role::User => StoredBody::User {
            content,
            turn: turn_record(row)?,
        },
        Role::Scheduled => StoredBody::Scheduled {
            label: content,
            turn: turn_record(row)?,
        },
        Role::Assistant => StoredBody::Assistant { content },
        Role::Tool => StoredBody::Tool {
            call_id: row.try_get(12)?,
            name: row.try_get(13)?,
            output: row.try_get(14)?,
            success: row.try_get(15)?,
            description: row.try_get(19)?,
        },
        Role::Approval => StoredBody::Approval {
            query_id: row.try_get(16)?,
            approval: Approval {
                app: row.try_get(17)?,
                action: row.try_get(18)?,
                fields: Vec::new(),
            },
        },
        Role::Member => StoredBody::Member {
            name: row.try_get::<Option<String>, _>(5)?.unwrap_or_default(),
            content,
        },
        Role::Beep => StoredBody::Beep { content },
    };
    Ok(StoredMessage {
        id: row.try_get(0)?,
        chat_id: row.try_get(1)?,
        turn_id: row.try_get(2)?,
        sent_at: row.try_get(3)?,
        author_id: row.try_get(4)?,
        body,
        images: Vec::new(),
    })
}

/// Saves `steps` in `chat` in order and returns their ids. A step before
/// the first one that opens a turn belongs to `turn`, the turn they carry on.
pub(super) async fn insert_steps(
    tx: &mut PgConnection,
    chat: Uuid,
    author: &str,
    steps: &[Step],
    mut turn: Option<i64>,
) -> Result<Vec<i64>> {
    let mut ids = Vec::with_capacity(steps.len());
    for step in steps {
        let opens = step.body.opens_turn();
        let id: i64 = sqlx::query_scalar(
            "WITH next AS (SELECT nextval(pg_get_serial_sequence('messages', 'id')) AS id)
             INSERT INTO messages (id, chat_id, role, turn_id, author_id, content, sent_at)
             SELECT id, $1, $2::message_role, CASE WHEN $3 THEN id ELSE $4 END, $5, $6, $7
             FROM next RETURNING id",
        )
        .bind(chat)
        .bind(step.body.role().as_str())
        .bind(opens)
        .bind(turn)
        .bind(matches!(step.body, StepBody::User { .. }).then_some(author))
        .bind(step.body.content())
        .bind(step.sent_at)
        .fetch_one(&mut *tx)
        .await?;
        if opens {
            turn = Some(id);
        }
        match &step.body {
            StepBody::User { turn, .. } | StepBody::Scheduled { turn, .. } => {
                sqlx::query(
                    "INSERT INTO turns (message_id, model, auto_thinking, reasoning, memory_score)
                     VALUES ($1, $2, $3::reasoning_level, $4, $5)",
                )
                .bind(id)
                .bind(&turn.model)
                .bind(turn.auto_thinking.map(|level| level.to_string()))
                .bind(&turn.reasoning)
                .bind(turn.memory_score)
                .execute(&mut *tx)
                .await?;
            }
            StepBody::Tool {
                call_id,
                name,
                output,
                success,
                description,
            } => {
                sqlx::query(
                    "INSERT INTO tool_calls (message_id, call_id, name, output, success)
                     VALUES ($1, $2, $3, $4, $5)",
                )
                .bind(id)
                .bind(call_id)
                .bind(name)
                .bind(output)
                .bind(success)
                .execute(&mut *tx)
                .await?;
                // Set apart: the backfill writes tool
                // calls before the migration that adds the column, and
                // never with a description.
                if !description.is_empty() {
                    sqlx::query("UPDATE tool_calls SET description = $2 WHERE message_id = $1")
                        .bind(id)
                        .bind(description)
                        .execute(&mut *tx)
                        .await?;
                }
            }
            StepBody::Approval { query_id, approval } => {
                sqlx::query(
                    "INSERT INTO approvals (message_id, query_id, app, action)
                     VALUES ($1, $2, $3, $4)",
                )
                .bind(id)
                .bind(query_id)
                .bind(&approval.app)
                .bind(&approval.action)
                .execute(&mut *tx)
                .await?;
                let (labels, values): (Vec<&str>, Vec<&str>) = approval
                    .fields
                    .iter()
                    .map(|field| (field.label.as_str(), field.value.as_str()))
                    .unzip();
                sqlx::query(
                    "INSERT INTO approval_fields (message_id, position, label, value)
                     SELECT $1, n - 1, label, value
                     FROM unnest($2::text[], $3::text[]) WITH ORDINALITY f (label, value, n)",
                )
                .bind(id)
                .bind(&labels)
                .bind(&values)
                .execute(&mut *tx)
                .await?;
            }
            StepBody::Assistant { .. } => {}
        }
        ids.push(id);
    }
    Ok(ids)
}

/// The newest turn of `chat`, the one an answered approval carries on.
pub(super) async fn latest_turn(tx: &mut PgConnection, chat: Uuid) -> Result<Option<i64>> {
    Ok(
        sqlx::query_scalar("SELECT max(id) FROM messages WHERE chat_id = $1 AND turn_id = id")
            .bind(chat)
            .fetch_one(&mut *tx)
            .await?,
    )
}

/// Deletes the approval cards still showing in `chat` and logs them gone.
/// A new turn answers or skips them.
pub(super) async fn drop_approvals(tx: &mut PgConnection, chat: Uuid) -> Result<Vec<Bumped>> {
    let gone: Vec<i64> = sqlx::query_scalar(
        "DELETE FROM messages WHERE chat_id = $1 AND role = 'approval' RETURNING id",
    )
    .bind(chat)
    .fetch_all(&mut *tx)
    .await?;
    log_messages(tx, chat, &gone, UpdateOp::Deleted, Readers::Members).await
}

/// Changes what turn `turn` ran with: adds `reasoning` to its reasoning and
/// sets the rest, then logs its opening message again.
pub(super) async fn patch_turn(
    tx: &mut PgConnection,
    chat: Uuid,
    turn: i64,
    patch: &TurnRecord,
) -> Result<Vec<Bumped>> {
    sqlx::query(
        "UPDATE turns SET model = CASE WHEN $2 = '' THEN model ELSE $2 END,
             auto_thinking = COALESCE($3::reasoning_level, auto_thinking),
             reasoning = CASE WHEN reasoning = '' THEN $4
                 WHEN $4 = '' THEN reasoning ELSE reasoning || E'\\n\\n' || $4 END,
             memory_score = COALESCE($5, memory_score)
         WHERE message_id = $1",
    )
    .bind(turn)
    .bind(&patch.model)
    .bind(patch.auto_thinking.map(|level| level.to_string()))
    .bind(&patch.reasoning)
    .bind(patch.memory_score)
    .execute(&mut *tx)
    .await?;
    log_messages(tx, chat, &[turn], UpdateOp::Message, Readers::Members).await
}

impl Store {
    /// What the newest turn of `chat` ran with.
    pub(crate) async fn last_turn(&self, chat: Uuid) -> Result<Option<TurnRecord>> {
        let row = sqlx::query(
            "SELECT t.model, t.auto_thinking::text, t.reasoning, t.memory_score
             FROM turns t JOIN messages m ON m.id = t.message_id
             WHERE m.chat_id = $1 ORDER BY m.id DESC LIMIT 1",
        )
        .bind(chat)
        .fetch_optional(&self.pool)
        .await?;
        row.map(|row| {
            let thinking: Option<String> = row.try_get(1)?;
            Ok(TurnRecord {
                model: row.try_get(0)?,
                auto_thinking: thinking.and_then(|level| level.parse().ok()),
                reasoning: row.try_get(2)?,
                memory_score: row.try_get(3)?,
            })
        })
        .transpose()
    }

    /// What each tool step of `chat` showed while it ran, oldest first.
    /// Steps without a description are left out. The caller checks the
    /// user may read it.
    pub(crate) async fn tool_labels(&self, chat: Uuid) -> Result<Vec<ToolLabel>> {
        let rows = sqlx::query(
            "SELECT c.call_id, c.description
             FROM tool_calls c JOIN messages m ON m.id = c.message_id
             WHERE m.chat_id = $1 AND c.description <> ''
             ORDER BY m.id",
        )
        .bind(chat)
        .fetch_all(&self.pool)
        .await?;
        rows.iter()
            .map(|row| {
                Ok(ToolLabel {
                    call_id: row.try_get(0)?,
                    description: row.try_get(1)?,
                })
            })
            .collect()
    }

    /// Every message of `chat`, oldest first, with their pictures. The
    /// caller checks the user may read it.
    pub(crate) async fn chat_messages(&self, chat: Uuid) -> Result<Vec<StoredMessage>> {
        let rows = sqlx::query(&format!(
            "SELECT {COLUMNS} FROM {JOINS} WHERE m.chat_id = $1 ORDER BY m.id"
        ))
        .bind(chat)
        .fetch_all(&self.pool)
        .await?;
        self.complete(rows).await
    }

    /// At most `limit` messages of `chat` after the message `after`, oldest
    /// first, leaving out those sent before `since`.
    pub(crate) async fn chat_messages_after(
        &self,
        chat: Uuid,
        after: i64,
        since: DateTime<Utc>,
        limit: i64,
    ) -> Result<Vec<StoredMessage>> {
        let rows = sqlx::query(&format!(
            "SELECT {COLUMNS} FROM {JOINS}
             WHERE m.chat_id = $1 AND m.id > $2 AND m.sent_at >= $3
             ORDER BY m.id LIMIT $4"
        ))
        .bind(chat)
        .bind(after)
        .bind(since)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        self.complete(rows).await
    }

    /// The messages with `ids` that still exist, by id.
    pub(crate) async fn messages_by_id(&self, ids: &[i64]) -> Result<Vec<StoredMessage>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let rows = sqlx::query(&format!(
            "SELECT {COLUMNS} FROM {JOINS} WHERE m.id = ANY($1) ORDER BY m.id"
        ))
        .bind(ids)
        .fetch_all(&self.pool)
        .await?;
        self.complete(rows).await
    }

    /// Reads `rows` and fills in approval fields and pictures.
    async fn complete(&self, rows: Vec<PgRow>) -> Result<Vec<StoredMessage>> {
        let mut messages = rows.iter().map(stored).collect::<Result<Vec<_>>>()?;
        let ids: Vec<i64> = messages.iter().map(|m| m.id).collect();
        if ids.is_empty() {
            return Ok(messages);
        }
        let mut fields: HashMap<i64, Vec<ApprovalField>> = HashMap::new();
        if messages
            .iter()
            .any(|m| matches!(m.body, StoredBody::Approval { .. }))
        {
            let rows = sqlx::query(
                "SELECT message_id, label, value FROM approval_fields
                 WHERE message_id = ANY($1) ORDER BY message_id, position",
            )
            .bind(&ids)
            .fetch_all(&self.pool)
            .await?;
            for row in rows {
                fields
                    .entry(row.try_get(0)?)
                    .or_default()
                    .push(ApprovalField {
                        label: row.try_get(1)?,
                        value: row.try_get(2)?,
                    });
            }
        }
        let mut images: HashMap<i64, Vec<ImageRef>> = HashMap::new();
        let rows = sqlx::query(
            "SELECT message_id, id, mime, width, height FROM files
             WHERE message_id = ANY($1) AND NOT attached ORDER BY message_id, position",
        )
        .bind(&ids)
        .fetch_all(&self.pool)
        .await?;
        for row in rows {
            let width: i32 = row.try_get(3)?;
            let height: i32 = row.try_get(4)?;
            images.entry(row.try_get(0)?).or_default().push(ImageRef {
                id: row.try_get(1)?,
                mime: row.try_get(2)?,
                width: u32::try_from(width)?,
                height: u32::try_from(height)?,
            });
        }
        for message in &mut messages {
            if let StoredBody::Approval { approval, .. } = &mut message.body {
                approval.fields = fields.remove(&message.id).unwrap_or_default();
            }
            message.images = images.remove(&message.id).unwrap_or_default();
        }
        Ok(messages)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        store::{CreateRun, FinishedTurn, RunStatus, TestDb, test_chat},
        user::UserKey,
    };
    use flashmind::core::{Conversation, ConversationEntry};

    fn tool(call_id: &str, description: &str) -> Step {
        Step {
            sent_at: Utc::now(),
            body: StepBody::Tool {
                call_id: call_id.to_owned(),
                name: "recall".to_owned(),
                output: "tea".to_owned(),
                success: true,
                description: description.to_owned(),
            },
        }
    }

    #[tokio::test]
    async fn tool_steps_keep_what_they_showed() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let alice = UserKey::for_test("alice");
        let chat = test_chat("s1");
        let CreateRun::Created(run) = store
            .create_run(&alice, chat, "req-1", "What do I like?")
            .await
            .unwrap()
        else {
            panic!("the run was not created");
        };
        let steps = [
            Step {
                sent_at: Utc::now(),
                body: StepBody::User {
                    content: "What do I like?".to_owned(),
                    turn: TurnRecord::default(),
                },
            },
            tool("a", "Recalling memories"),
            // Saved without one, as every step was before descriptions.
            tool("b", ""),
        ];
        let mut context = Conversation::new();
        context.add(ConversationEntry::user("What do I like?"));
        store
            .finish(
                &alice,
                &run,
                Some(FinishedTurn {
                    context: &context,
                    prompt_tokens: Some(0),
                    steps: &steps,
                    stands_alone: false,
                    carried: TurnRecord::default(),
                }),
                RunStatus::Completed,
            )
            .await
            .unwrap();

        let labels = store.tool_labels(chat).await.unwrap();
        assert_eq!(
            labels,
            [ToolLabel {
                call_id: "a".to_owned(),
                description: "Recalling memories".to_owned(),
            }]
        );
        let described: Vec<String> = store
            .chat_messages(chat)
            .await
            .unwrap()
            .into_iter()
            .filter_map(|message| match message.body {
                StoredBody::Tool { description, .. } => Some(description),
                _ => None,
            })
            .collect();
        assert_eq!(described, ["Recalling memories", ""]);
    }
}
