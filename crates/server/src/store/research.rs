//! Researches, their agents' entries and their sources.

use super::{
    Store,
    chats::{SessionChat, ensure_session},
    projects::end,
};
use crate::{
    account::user::UserKey,
    research::{Entry, Member, Plan, Status},
};
use anyhow::{Result, bail};
use chrono::{DateTime, Utc};
use sqlx::{PgConnection, Row, postgres::PgRow};
use uuid::Uuid;

/// A research as the store keeps it.
#[derive(Debug, Clone)]
pub struct ResearchRow {
    pub id: Uuid,
    /// The session the lead talks to the user in.
    pub chat: Uuid,
    pub user_id: String,
    pub topic: String,
    pub status: Status,
    /// Set once the user approves one.
    pub plan: Option<Plan>,
    pub draft: String,
    pub created_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
}

/// An entry with where it sits.
#[derive(Debug, Clone)]
pub struct EntryRow {
    pub seq: i64,
    pub member: Member,
    pub entry: Entry,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SourceRow {
    pub number: u32,
    pub url: String,
    pub title: String,
    pub quote: String,
    pub member: Member,
}

/// A research with how many sources it kept, for a list.
#[derive(Debug, Clone)]
pub struct ListedResearch {
    pub research: ResearchRow,
    pub sources: u32,
}

/// The most researches a list returns.
const MAX_LISTED: i64 = 200;

/// A source a member found.
pub struct NewSource<'a> {
    pub url: &'a str,
    pub title: &'a str,
    pub quote: &'a str,
}

const COLUMNS: &str = "id, chat_id, user_id, topic, status, plan, draft, created_at, finished_at";

fn research_from(row: &PgRow) -> Result<ResearchRow> {
    let status: String = row.try_get(4)?;
    let plan: Option<serde_json::Value> = row.try_get(5)?;
    Ok(ResearchRow {
        id: row.try_get(0)?,
        chat: row.try_get(1)?,
        user_id: row.try_get(2)?,
        topic: row.try_get(3)?,
        status: status.parse()?,
        plan: plan.map(serde_json::from_value).transpose()?,
        draft: row.try_get(6)?,
        created_at: row.try_get(7)?,
        finished_at: row.try_get(8)?,
    })
}

fn entry_from(row: &PgRow) -> Result<EntryRow> {
    let member: String = row.try_get(1)?;
    let kind: String = row.try_get(2)?;
    let recipient: Option<String> = row.try_get(3)?;
    let content: String = row.try_get(4)?;
    let entry = match kind.as_str() {
        "said" => Entry::Said {
            to: recipient.map(|to| to.parse()).transpose()?,
            content,
        },
        "step" => Entry::Step {
            label: content,
            success: row.try_get(5)?,
        },
        "note" => {
            let sources: Vec<i32> = row.try_get(6)?;
            Entry::Note {
                content,
                sources: sources
                    .into_iter()
                    .map(u32::try_from)
                    .collect::<Result<_, _>>()?,
            }
        }
        other => bail!("unknown research entry kind {other}"),
    };
    Ok(EntryRow {
        seq: row.try_get(0)?,
        member: member.parse()?,
        entry,
        created_at: row.try_get(7)?,
    })
}

/// An entry as its row's columns.
struct EntryColumns<'a> {
    kind: &'static str,
    recipient: Option<String>,
    content: &'a str,
    success: bool,
    sources: Vec<i32>,
}

impl<'a> EntryColumns<'a> {
    fn of(entry: &'a Entry) -> Result<Self> {
        Ok(match entry {
            Entry::Said { to, content } => Self {
                kind: "said",
                recipient: to.map(|to| to.to_string()),
                content,
                success: true,
                sources: Vec::new(),
            },
            Entry::Step { label, success } => Self {
                kind: "step",
                recipient: None,
                content: label,
                success: *success,
                sources: Vec::new(),
            },
            Entry::Note { content, sources } => Self {
                kind: "note",
                recipient: None,
                content,
                success: true,
                sources: sources
                    .iter()
                    .map(|&n| i32::try_from(n))
                    .collect::<Result<_, _>>()?,
            },
        })
    }
}

impl Store {
    /// Makes session `chat` a research on `topic`, creating it if new; an existing research is kept.
    pub(crate) async fn start_research(
        &self,
        user: &UserKey,
        chat: Uuid,
        topic: &str,
    ) -> Result<ResearchRow> {
        let mut tx = self.pool.begin().await?;
        let done = start_research_in(&mut tx, user, chat, topic).await;
        let keep = done.is_ok();
        end(tx, keep, done).await
    }

    /// The research `chat` runs, if it is one.
    pub(crate) async fn research_in(
        &self,
        user: &UserKey,
        chat: Uuid,
    ) -> Result<Option<ResearchRow>> {
        let row = sqlx::query(&format!(
            "SELECT {COLUMNS} FROM researches WHERE chat_id=$1 AND user_id=$2"
        ))
        .bind(chat)
        .bind(user.as_ref())
        .fetch_optional(&self.pool)
        .await?;
        row.as_ref().map(research_from).transpose()
    }

    /// The user's researches, newest first.
    pub(crate) async fn list_researches(&self, user: &UserKey) -> Result<Vec<ListedResearch>> {
        let rows = sqlx::query(&format!(
            "SELECT {COLUMNS},
                    (SELECT COUNT(*) FROM research_sources s WHERE s.research_id = r.id)
             FROM researches r WHERE user_id=$1 ORDER BY created_at DESC LIMIT $2"
        ))
        .bind(user.as_ref())
        .bind(MAX_LISTED)
        .fetch_all(&self.pool)
        .await?;
        rows.iter()
            .map(|row| {
                let sources: i64 = row.try_get(9)?;
                Ok(ListedResearch {
                    research: research_from(row)?,
                    sources: u32::try_from(sources)?,
                })
            })
            .collect()
    }

    pub(crate) async fn research(&self, id: Uuid) -> Result<Option<ResearchRow>> {
        let row = sqlx::query(&format!("SELECT {COLUMNS} FROM researches WHERE id=$1"))
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        row.as_ref().map(research_from).transpose()
    }

    /// Starts the team on `plan` unless running; says whether it did.
    pub(crate) async fn approve_research(&self, id: Uuid, plan: &Plan) -> Result<bool> {
        let can_start: Vec<&str> = [
            Status::Planning,
            Status::Done,
            Status::Failed,
            Status::Stopped,
            Status::Interrupted,
        ]
        .into_iter()
        .map(<&'static str>::from)
        .collect();
        let moved = sqlx::query(
            "UPDATE researches SET status=$2, plan=$3, finished_at=NULL
             WHERE id=$1 AND status = ANY($4)",
        )
        .bind(id)
        .bind(Status::Researching.as_ref())
        .bind(serde_json::to_value(plan)?)
        .bind(&can_start)
        .execute(&self.pool)
        .await?
        .rows_affected();
        Ok(moved == 1)
    }

    /// Keeps the plan the running team grew, with the scouts the lead added.
    pub(crate) async fn set_research_plan(&self, id: Uuid, plan: &Plan) -> Result<()> {
        sqlx::query("UPDATE researches SET plan=$2 WHERE id=$1")
            .bind(id)
            .bind(serde_json::to_value(plan)?)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Moves the research from `from` to `to`; says whether it was at `from`, so a stop wins.
    pub(crate) async fn move_research(&self, id: Uuid, from: Status, to: Status) -> Result<bool> {
        let moved = sqlx::query(
            "UPDATE researches SET status=$3,
                 finished_at = CASE WHEN $4 THEN now() ELSE finished_at END
             WHERE id=$1 AND status=$2",
        )
        .bind(id)
        .bind(from.as_ref())
        .bind(to.as_ref())
        .bind(!to.is_running())
        .execute(&self.pool)
        .await?
        .rows_affected();
        Ok(moved == 1)
    }

    /// Stops the team, wherever it is. Says whether it was running.
    pub(crate) async fn stop_research(&self, id: Uuid) -> Result<bool> {
        let running: Vec<&str> = [Status::Researching, Status::Reviewing, Status::Writing]
            .into_iter()
            .map(<&'static str>::from)
            .collect();
        let moved = sqlx::query(
            "UPDATE researches SET status=$2, finished_at=now() WHERE id=$1 AND status = ANY($3)",
        )
        .bind(id)
        .bind(Status::Stopped.as_ref())
        .bind(&running)
        .execute(&self.pool)
        .await?
        .rows_affected();
        Ok(moved == 1)
    }

    /// Marks running researches interrupted at startup.
    pub(crate) async fn interrupt_researches(&self) -> Result<()> {
        sqlx::query(
            "UPDATE researches SET status='interrupted', finished_at=now()
             WHERE status IN ('researching', 'reviewing', 'writing')",
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub(crate) async fn set_research_draft(&self, id: Uuid, draft: &str) -> Result<()> {
        sqlx::query("UPDATE researches SET draft=$2 WHERE id=$1")
            .bind(id)
            .bind(draft)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub(crate) async fn add_research_entry(
        &self,
        id: Uuid,
        member: Member,
        entry: &Entry,
    ) -> Result<i64> {
        let columns = EntryColumns::of(entry)?;
        let seq = sqlx::query_scalar(
            "INSERT INTO research_entries (research_id, agent, kind, recipient, content, success, sources)
             VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING seq",
        )
        .bind(id)
        .bind(member.to_string())
        .bind(columns.kind)
        .bind(columns.recipient)
        .bind(columns.content)
        .bind(columns.success)
        .bind(columns.sources)
        .fetch_one(&self.pool)
        .await?;
        Ok(seq)
    }

    /// The research's entries after `after`, oldest first.
    pub(crate) async fn research_entries(&self, id: Uuid, after: i64) -> Result<Vec<EntryRow>> {
        let rows = sqlx::query(
            "SELECT seq, agent, kind, recipient, content, success, sources, created_at
             FROM research_entries WHERE research_id=$1 AND seq > $2 ORDER BY seq LIMIT 2000",
        )
        .bind(id)
        .bind(after)
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(entry_from).collect()
    }

    /// Keeps a source and returns its number; a known URL keeps its number.
    pub(crate) async fn add_research_source(
        &self,
        id: Uuid,
        member: Member,
        source: &NewSource<'_>,
    ) -> Result<u32> {
        let mut tx = self.pool.begin().await?;
        // Numbers are given out one at a time per research.
        sqlx::query("SELECT 1 FROM researches WHERE id=$1 FOR UPDATE")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        let number: i32 = sqlx::query_scalar(
            "INSERT INTO research_sources (research_id, number, url, title, quote, agent)
             SELECT $1, COALESCE(MAX(number), 0) + 1, $2, $3, $4, $5
             FROM research_sources WHERE research_id=$1
             ON CONFLICT (research_id, url) DO UPDATE SET url=excluded.url
             RETURNING number",
        )
        .bind(id)
        .bind(source.url.trim())
        .bind(source.title.trim())
        .bind(source.quote.trim())
        .bind(member.to_string())
        .fetch_one(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(u32::try_from(number)?)
    }

    pub(crate) async fn research_sources(&self, id: Uuid) -> Result<Vec<SourceRow>> {
        let rows = sqlx::query(
            "SELECT number, url, title, quote, agent FROM research_sources
             WHERE research_id=$1 ORDER BY number",
        )
        .bind(id)
        .fetch_all(&self.pool)
        .await?;
        rows.iter()
            .map(|row| {
                let number: i32 = row.try_get(0)?;
                let member: String = row.try_get(4)?;
                Ok(SourceRow {
                    number: u32::try_from(number)?,
                    url: row.try_get(1)?,
                    title: row.try_get(2)?,
                    quote: row.try_get(3)?,
                    member: member.parse()?,
                })
            })
            .collect()
    }
}

/// The body of [`Store::start_research`], in its transaction.
async fn start_research_in(
    conn: &mut PgConnection,
    user: &UserKey,
    chat: Uuid,
    topic: &str,
) -> Result<ResearchRow> {
    if ensure_session(conn, user.as_ref(), chat).await? == SessionChat::Taken {
        bail!("the chat is not the user's");
    }

    sqlx::query(
        "INSERT INTO researches (id, chat_id, user_id, topic) VALUES ($1, $2, $3, $4)
         ON CONFLICT (chat_id) DO NOTHING",
    )
    .bind(Uuid::new_v4())
    .bind(chat)
    .bind(user.as_ref())
    .bind(topic.trim())
    .execute(&mut *conn)
    .await?;
    let row = sqlx::query(&format!(
        "SELECT {COLUMNS} FROM researches WHERE chat_id=$1 AND user_id=$2"
    ))
    .bind(chat)
    .bind(user.as_ref())
    .fetch_one(&mut *conn)
    .await?;
    research_from(&row)
}

#[cfg(test)]
#[path = "../tests/store_research.rs"]
mod tests;
