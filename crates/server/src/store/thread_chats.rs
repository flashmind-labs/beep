//! Chats about one pull request or issue of a project, and Beep's replies on others' threads.

use super::{
    Store,
    chats::{SessionChat, ensure_session},
    projects::end,
};
use crate::{
    account::user::UserKey,
    projects::{model::ProjectId, threads::ThreadKind},
};
use anyhow::{Result, bail};
use sqlx::{PgConnection, Row, postgres::PgRow};
use std::str::FromStr;
use uuid::Uuid;

/// One chat about a thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ThreadChatRow {
    pub(crate) project: ProjectId,
    /// `org/repo`, lowercase.
    pub(crate) repo: String,
    pub(crate) number: u64,
    pub(crate) kind: ThreadKind,
    pub(crate) title: String,
    pub(crate) chat: Uuid,
}

/// What a new thread chat is about.
pub(crate) struct NewThreadChat<'a> {
    pub(crate) repo: &'a str,
    pub(crate) number: u64,
    pub(crate) kind: ThreadKind,
    pub(crate) title: &'a str,
}

const COLUMNS: &str = "project_id, repo, number, kind, title, chat_id";

fn row_from(row: &PgRow) -> Result<ThreadChatRow> {
    let kind: String = row.try_get("kind")?;
    let number: i64 = row.try_get("number")?;
    Ok(ThreadChatRow {
        project: ProjectId(row.try_get("project_id")?),
        repo: row.try_get("repo")?,
        number: u64::try_from(number)?,
        kind: ThreadKind::from_str(&kind)?,
        title: row.try_get("title")?,
        chat: row.try_get("chat_id")?,
    })
}

impl Store {
    /// The chat about thread `new` of `project`, created on first ask; concurrent callers get the same chat.
    pub(crate) async fn open_thread_chat(
        &self,
        user: &UserKey,
        project: ProjectId,
        new: &NewThreadChat<'_>,
    ) -> Result<ThreadChatRow> {
        let mut tx = self.pool.begin().await?;
        let done = thread_chat_in(&mut tx, user, project, new).await;
        let keep = done.is_ok();
        end(tx, keep, done).await
    }

    /// The thread chats of `project`, newest first.
    pub(crate) async fn thread_chats(&self, project: ProjectId) -> Result<Vec<ThreadChatRow>> {
        let rows = sqlx::query(&format!(
            "SELECT {COLUMNS} FROM project_thread_chats WHERE project_id=$1
             ORDER BY created_at DESC, number DESC"
        ))
        .bind(project.0)
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(row_from).collect()
    }

    /// The thread `chat` is about, if it is a thread chat of the user's.
    pub(crate) async fn thread_in(
        &self,
        user: &UserKey,
        chat: Uuid,
    ) -> Result<Option<ThreadChatRow>> {
        let row = sqlx::query(
            "SELECT t.project_id, t.repo, t.number, t.kind, t.title, t.chat_id
             FROM project_thread_chats t JOIN projects p ON p.id = t.project_id
             WHERE t.chat_id=$1 AND p.user_id=$2",
        )
        .bind(chat)
        .bind(user.as_str())
        .fetch_optional(&self.pool)
        .await?;
        row.as_ref().map(row_from).transpose()
    }

    /// Notes Beep replied on someone else's thread; false if it already had.
    pub(crate) async fn note_thread_reply(
        &self,
        project: ProjectId,
        repo: &str,
        number: u64,
    ) -> Result<bool> {
        let noted = sqlx::query(
            "INSERT INTO project_thread_replies (project_id, repo, number) VALUES ($1, $2, $3)
             ON CONFLICT DO NOTHING",
        )
        .bind(project.0)
        .bind(repo)
        .bind(i64::try_from(number)?)
        .execute(&self.pool)
        .await?
        .rows_affected();
        Ok(noted == 1)
    }
}

async fn thread_chat_in(
    tx: &mut PgConnection,
    user: &UserKey,
    project: ProjectId,
    new: &NewThreadChat<'_>,
) -> Result<ThreadChatRow> {
    // Locking the project makes a second caller wait and find the chat.
    let found: Option<Uuid> =
        sqlx::query_scalar("SELECT id FROM projects WHERE id=$1 AND user_id=$2 FOR UPDATE")
            .bind(project.0)
            .bind(user.as_str())
            .fetch_optional(&mut *tx)
            .await?;
    if found.is_none() {
        bail!("the project is not the user's");
    }
    let number = i64::try_from(new.number)?;
    let existing = sqlx::query(&format!(
        "SELECT {COLUMNS} FROM project_thread_chats WHERE project_id=$1 AND repo=$2 AND number=$3"
    ))
    .bind(project.0)
    .bind(new.repo)
    .bind(number)
    .fetch_optional(&mut *tx)
    .await?;
    if let Some(row) = existing {
        return row_from(&row);
    }
    let chat = Uuid::new_v4();
    if ensure_session(&mut *tx, user.as_str(), chat).await? != SessionChat::New {
        bail!("a fresh chat id was already taken");
    }
    sqlx::query("UPDATE chats SET title=$2 WHERE id=$1")
        .bind(chat)
        .bind(new.title)
        .execute(&mut *tx)
        .await?;
    let kind: &'static str = new.kind.into();
    sqlx::query(
        "INSERT INTO project_thread_chats (project_id, repo, number, kind, title, chat_id)
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(project.0)
    .bind(new.repo)
    .bind(number)
    .bind(kind)
    .bind(new.title)
    .bind(chat)
    .execute(&mut *tx)
    .await?;
    Ok(ThreadChatRow {
        project,
        repo: new.repo.to_owned(),
        number: new.number,
        kind: new.kind,
        title: new.title.to_owned(),
        chat,
    })
}
