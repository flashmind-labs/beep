//! Researches, their agents' entries and their sources.

use super::{
    Store,
    chats::{SessionChat, ensure_session},
};
use crate::{
    research::{Entry, Member, Plan, Status},
    user::UserKey,
};
use anyhow::{Result, bail};
use chrono::{DateTime, Utc};
use sqlx::{Row, postgres::PgRow};
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
    /// Makes session `chat` a research on `topic`, making the session if it
    /// is new. A chat that is already one keeps its research.
    pub(crate) async fn start_research(
        &self,
        user: &UserKey,
        chat: Uuid,
        topic: &str,
    ) -> Result<ResearchRow> {
        let mut tx = self.pool.begin().await?;
        if ensure_session(&mut tx, user.as_str(), chat).await? == SessionChat::Taken {
            bail!("the chat is not the user's");
        }
        sqlx::query(
            "INSERT INTO researches (id, chat_id, user_id, topic) VALUES ($1, $2, $3, $4)
             ON CONFLICT (chat_id) DO NOTHING",
        )
        .bind(Uuid::new_v4())
        .bind(chat)
        .bind(user.as_str())
        .bind(topic.trim())
        .execute(&mut *tx)
        .await?;
        let row = sqlx::query(&format!(
            "SELECT {COLUMNS} FROM researches WHERE chat_id=$1 AND user_id=$2"
        ))
        .bind(chat)
        .bind(user.as_str())
        .fetch_one(&mut *tx)
        .await?;
        tx.commit().await?;
        research_from(&row)
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
        .bind(user.as_str())
        .fetch_optional(&self.pool)
        .await?;
        row.as_ref().map(research_from).transpose()
    }

    pub(crate) async fn research(&self, id: Uuid) -> Result<Option<ResearchRow>> {
        let row = sqlx::query(&format!("SELECT {COLUMNS} FROM researches WHERE id=$1"))
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        row.as_ref().map(research_from).transpose()
    }

    /// Starts the team on `plan`, if the research can start: it is not
    /// running. Says whether it did.
    pub(crate) async fn approve_research(&self, id: Uuid, plan: &Plan) -> Result<bool> {
        let can_start: Vec<&str> = [
            Status::Planning,
            Status::Done,
            Status::Failed,
            Status::Stopped,
            Status::Interrupted,
        ]
        .into_iter()
        .map(Status::as_str)
        .collect();
        let moved = sqlx::query(
            "UPDATE researches SET status=$2, plan=$3, finished_at=NULL
             WHERE id=$1 AND status = ANY($4)",
        )
        .bind(id)
        .bind(Status::Researching.as_str())
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

    /// Moves the research from `from` to `to`. Says whether it was at
    /// `from`, so a stop in between wins over the team moving on.
    pub(crate) async fn move_research(&self, id: Uuid, from: Status, to: Status) -> Result<bool> {
        let moved = sqlx::query(
            "UPDATE researches SET status=$3,
                 finished_at = CASE WHEN $4 THEN now() ELSE finished_at END
             WHERE id=$1 AND status=$2",
        )
        .bind(id)
        .bind(from.as_str())
        .bind(to.as_str())
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
            .map(Status::as_str)
            .collect();
        let moved = sqlx::query(
            "UPDATE researches SET status=$2, finished_at=now() WHERE id=$1 AND status = ANY($3)",
        )
        .bind(id)
        .bind(Status::Stopped.as_str())
        .bind(&running)
        .execute(&self.pool)
        .await?
        .rows_affected();
        Ok(moved == 1)
    }

    /// Marks every running research interrupted, at startup: their teams
    /// died with the last server.
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

    /// Keeps a source and returns its number. A URL kept before keeps its
    /// number.
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        research::{RoleModel, Scout},
        store::TestDb,
    };
    use flashmind::types::ReasoningLevel;

    fn plan() -> Plan {
        let model = RoleModel {
            model: "openrouter:deepseek/deepseek-v4.1-flash"
                .parse()
                .expect("a model id"),
            thinking: ReasoningLevel::Low,
        };
        Plan {
            scouts: vec![
                Scout {
                    question: "Who ships first?".into(),
                    model: model.clone(),
                    round: 1,
                },
                Scout {
                    question: "What does it cost?".into(),
                    model: model.clone(),
                    round: 1,
                },
            ],
            critic: model.clone(),
            writer: model,
        }
    }

    #[tokio::test]
    async fn a_research_runs_through_its_statuses() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let user = UserKey::for_test("alice");
        let chat = Uuid::new_v4();
        let made = store
            .start_research(&user, chat, " EV batteries ")
            .await
            .unwrap();
        assert_eq!(made.topic, "EV batteries");
        assert_eq!(made.status, Status::Planning);
        // Starting again keeps the same research.
        let again = store.start_research(&user, chat, "other").await.unwrap();
        assert_eq!(again.id, made.id);

        assert!(store.approve_research(made.id, &plan()).await.unwrap());
        assert!(!store.approve_research(made.id, &plan()).await.unwrap());
        let row = store.research_in(&user, chat).await.unwrap().unwrap();
        assert_eq!(row.status, Status::Researching);
        assert_eq!(row.plan, Some(plan()));

        assert!(store.stop_research(made.id).await.unwrap());
        // The team moving on after a stop changes nothing.
        assert!(
            !store
                .move_research(made.id, Status::Researching, Status::Reviewing)
                .await
                .unwrap()
        );
        let row = store.research(made.id).await.unwrap().unwrap();
        assert_eq!(row.status, Status::Stopped);
        assert!(row.finished_at.is_some());
        // A stopped research can start again.
        assert!(store.approve_research(made.id, &plan()).await.unwrap());
        // And a finished one starts a follow-up round.
        assert!(
            store
                .move_research(made.id, Status::Researching, Status::Done)
                .await
                .unwrap()
        );
        let mut more = plan();
        more.scouts.push(Scout {
            question: "And next year?".into(),
            model: more.critic.clone(),
            round: 2,
        });
        assert!(store.approve_research(made.id, &more).await.unwrap());
        let row = store.research(made.id).await.unwrap().unwrap();
        assert_eq!(row.plan.map(|plan| plan.round()), Some(2));
        assert!(row.finished_at.is_none());
        store.interrupt_researches().await.unwrap();
        let row = store.research(made.id).await.unwrap().unwrap();
        assert_eq!(row.status, Status::Interrupted);
    }

    #[tokio::test]
    async fn entries_and_sources_keep_their_order() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let user = UserKey::for_test("alice");
        let made = store
            .start_research(&user, Uuid::new_v4(), "topic")
            .await
            .unwrap();
        let id = made.id;

        let new = |url| NewSource {
            url,
            title: "Title",
            quote: "A quote",
        };
        assert_eq!(
            store
                .add_research_source(id, Member::Scout(1), &new("https://a.example"))
                .await
                .unwrap(),
            1
        );
        assert_eq!(
            store
                .add_research_source(id, Member::Scout(2), &new("https://b.example"))
                .await
                .unwrap(),
            2
        );
        assert_eq!(
            store
                .add_research_source(id, Member::Scout(2), &new("https://a.example"))
                .await
                .unwrap(),
            1
        );
        let sources = store.research_sources(id).await.unwrap();
        assert_eq!(sources.len(), 2);
        assert_eq!(sources[0].member, Member::Scout(1));

        let said = Entry::Said {
            to: Some(Member::Scout(2)),
            content: "Check the cost".into(),
        };
        let note = Entry::Note {
            content: "Yields are low".into(),
            sources: vec![1, 2],
        };
        let step = Entry::Step {
            label: "Searched the web".into(),
            success: false,
        };
        let first = store
            .add_research_entry(id, Member::Lead, &said)
            .await
            .unwrap();
        store
            .add_research_entry(id, Member::Scout(1), &note)
            .await
            .unwrap();
        store
            .add_research_entry(id, Member::Scout(1), &step)
            .await
            .unwrap();

        let all = store.research_entries(id, 0).await.unwrap();
        let entries: Vec<_> = all
            .iter()
            .map(|row| (row.member, row.entry.clone()))
            .collect();
        assert_eq!(
            entries,
            [
                (Member::Lead, said),
                (Member::Scout(1), note),
                (Member::Scout(1), step)
            ]
        );
        assert_eq!(store.research_entries(id, first).await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn deleting_the_chat_deletes_the_research() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let user = UserKey::for_test("alice");
        let chat = Uuid::new_v4();
        let made = store.start_research(&user, chat, "topic").await.unwrap();
        sqlx::query("DELETE FROM chats WHERE id=$1")
            .bind(chat)
            .execute(&store.pool)
            .await
            .unwrap();
        assert!(store.research(made.id).await.unwrap().is_none());
    }
}
