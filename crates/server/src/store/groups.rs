//! Group chats: their members, messages and invite links.

use super::Store;
use crate::user::UserKey;
use anyhow::Result;
use chrono::{DateTime, Utc};
use sqlx::{Row, postgres::PgRow};
use uuid::Uuid;

/// A group's own columns.
#[derive(Debug, Clone)]
pub struct GroupRow {
    pub id: String,
    pub name: String,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct MemberRow {
    pub user_id: String,
    pub name: String,
    pub creator: bool,
    pub joined_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct MessageRow {
    pub seq: i64,
    /// `None` for Beep.
    pub author_id: Option<String>,
    pub author_name: String,
    pub content: String,
    pub sent_at: DateTime<Utc>,
}

/// A group with who is in it and its latest message.
#[derive(Debug, Clone)]
pub struct JoinedGroup {
    pub group: GroupRow,
    /// Oldest first.
    pub members: Vec<MemberRow>,
    pub last: Option<MessageRow>,
}

/// A member's message once posted.
#[derive(Debug, Clone)]
pub struct Posted {
    pub message: MessageRow,
    /// `false` when the request was posted before.
    pub new: bool,
}

/// How joining through an invite went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Join {
    /// In the group with this id.
    Joined(String),
    /// Someone in the group already goes by the name.
    NameTaken,
    /// No group has the token.
    UnknownInvite,
}

/// A member's message to post.
pub struct NewMessage<'a> {
    pub group_id: &'a str,
    /// The client's id for it, so a retry posts once.
    pub request_id: &'a str,
    pub content: &'a str,
}

const MESSAGE_COLUMNS: &str = "seq, author_id, author_name, content, sent_at";

fn message(row: &PgRow) -> Result<MessageRow> {
    Ok(MessageRow {
        seq: row.try_get(0)?,
        author_id: row.try_get(1)?,
        author_name: row.try_get(2)?,
        content: row.try_get(3)?,
        sent_at: row.try_get(4)?,
    })
}

/// A new invite token, as long and plain as a share's.
fn new_token() -> String {
    Uuid::new_v4().simple().to_string()
}

impl Store {
    /// Makes the group `id` with `user` as its creator. `false` when the id
    /// is taken.
    pub(crate) async fn create_group(
        &self,
        user: &UserKey,
        id: &str,
        name: &str,
        member_name: &str,
    ) -> Result<bool> {
        let mut tx = self.pool.begin().await?;
        let created = sqlx::query(
            "INSERT INTO groups (id, name, invite) VALUES ($1, $2, $3) ON CONFLICT DO NOTHING",
        )
        .bind(id)
        .bind(name)
        .bind(new_token())
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if created == 0 {
            return Ok(false);
        }
        sqlx::query(
            "INSERT INTO group_members (group_id, user_id, name, creator) VALUES ($1, $2, $3, TRUE)",
        )
        .bind(id)
        .bind(user.as_str())
        .bind(member_name)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(true)
    }

    /// The groups `user` is in, most recently active first.
    pub(crate) async fn groups(&self, user: &UserKey) -> Result<Vec<JoinedGroup>> {
        let ids: Vec<String> = sqlx::query_scalar(
            "SELECT g.id FROM groups g JOIN group_members m ON m.group_id = g.id
             WHERE m.user_id = $1 ORDER BY g.updated_at DESC LIMIT 200",
        )
        .bind(user.as_str())
        .fetch_all(&self.pool)
        .await?;
        let mut groups = Vec::with_capacity(ids.len());
        for id in ids {
            if let Some(group) = self.joined_group(&id).await? {
                groups.push(group);
            }
        }
        Ok(groups)
    }

    /// The group `id`, if `user` is in it.
    pub(crate) async fn group(&self, user: &UserKey, id: &str) -> Result<Option<JoinedGroup>> {
        if !self.is_member(user, id).await? {
            return Ok(None);
        }
        self.joined_group(id).await
    }

    pub(crate) async fn is_member(&self, user: &UserKey, id: &str) -> Result<bool> {
        Ok(sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM group_members WHERE group_id = $1 AND user_id = $2)",
        )
        .bind(id)
        .bind(user.as_str())
        .fetch_one(&self.pool)
        .await?)
    }

    async fn joined_group(&self, id: &str) -> Result<Option<JoinedGroup>> {
        let row = sqlx::query("SELECT id, name, updated_at FROM groups WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        let Some(row) = row else {
            return Ok(None);
        };
        let group = GroupRow {
            id: row.try_get(0)?,
            name: row.try_get(1)?,
            updated_at: row.try_get(2)?,
        };
        let last = self.group_messages(id, 1).await?.pop();
        Ok(Some(JoinedGroup {
            members: self.group_members(id).await?,
            group,
            last,
        }))
    }

    /// Who is in group `id`, oldest first.
    pub(crate) async fn group_members(&self, id: &str) -> Result<Vec<MemberRow>> {
        let rows = sqlx::query(
            "SELECT user_id, name, creator, joined_at FROM group_members
             WHERE group_id = $1 ORDER BY joined_at, user_id",
        )
        .bind(id)
        .fetch_all(&self.pool)
        .await?;
        rows.iter()
            .map(|row| {
                Ok(MemberRow {
                    user_id: row.try_get(0)?,
                    name: row.try_get(1)?,
                    creator: row.try_get(2)?,
                    joined_at: row.try_get(3)?,
                })
            })
            .collect()
    }

    /// The last `limit` messages of group `id`, oldest first. Callers check
    /// membership first.
    pub(crate) async fn group_messages(&self, id: &str, limit: i64) -> Result<Vec<MessageRow>> {
        self.group_messages_until(id, i64::MAX, limit).await
    }

    /// The last `limit` messages of group `id` up to and including `seq`,
    /// oldest first.
    pub(crate) async fn group_messages_until(
        &self,
        id: &str,
        seq: i64,
        limit: i64,
    ) -> Result<Vec<MessageRow>> {
        let rows = sqlx::query(&format!(
            "SELECT {MESSAGE_COLUMNS} FROM (SELECT {MESSAGE_COLUMNS} FROM group_messages
             WHERE group_id = $1 AND seq <= $2 ORDER BY seq DESC LIMIT $3) recent ORDER BY seq"
        ))
        .bind(id)
        .bind(seq)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(message).collect()
    }

    /// Posts `user`'s message under their member name. Posting the same
    /// request again returns the first message. `None` when `user` is not
    /// in the group.
    pub(crate) async fn post_to_group(
        &self,
        user: &UserKey,
        new: NewMessage<'_>,
    ) -> Result<Option<Posted>> {
        let mut tx = self.pool.begin().await?;
        let name: Option<String> = sqlx::query_scalar(
            "SELECT name FROM group_members WHERE group_id = $1 AND user_id = $2",
        )
        .bind(new.group_id)
        .bind(user.as_str())
        .fetch_optional(&mut *tx)
        .await?;
        let Some(name) = name else {
            return Ok(None);
        };
        let inserted = sqlx::query(&format!(
            "INSERT INTO group_messages (group_id, author_id, author_name, request_id, content)
             VALUES ($1, $2, $3, $4, $5) ON CONFLICT DO NOTHING RETURNING {MESSAGE_COLUMNS}"
        ))
        .bind(new.group_id)
        .bind(user.as_str())
        .bind(name)
        .bind(new.request_id)
        .bind(new.content)
        .fetch_optional(&mut *tx)
        .await?;
        let fresh = inserted.is_some();
        let row = match inserted {
            Some(row) => {
                touch(&mut tx, new.group_id).await?;
                row
            }
            None => {
                sqlx::query(&format!(
                    "SELECT {MESSAGE_COLUMNS} FROM group_messages
                     WHERE group_id = $1 AND request_id = $2"
                ))
                .bind(new.group_id)
                .bind(new.request_id)
                .fetch_one(&mut *tx)
                .await?
            }
        };
        tx.commit().await?;
        Ok(Some(Posted {
            message: message(&row)?,
            new: fresh,
        }))
    }

    /// Posts Beep's reply to group `id`.
    pub(crate) async fn post_beep(&self, id: &str, content: &str) -> Result<MessageRow> {
        let mut tx = self.pool.begin().await?;
        let row = sqlx::query(&format!(
            "INSERT INTO group_messages (group_id, author_name, content)
             VALUES ($1, 'Beep', $2) RETURNING {MESSAGE_COLUMNS}"
        ))
        .bind(id)
        .bind(content)
        .fetch_one(&mut *tx)
        .await?;
        touch(&mut tx, id).await?;
        tx.commit().await?;
        message(&row)
    }

    /// The invite token of group `id`, a new one with `reset`. `None` when
    /// `user` is not in it.
    pub(crate) async fn group_invite(
        &self,
        user: &UserKey,
        id: &str,
        reset: bool,
    ) -> Result<Option<String>> {
        if !self.is_member(user, id).await? {
            return Ok(None);
        }
        let query = if reset {
            sqlx::query_scalar("UPDATE groups SET invite = $2 WHERE id = $1 RETURNING invite")
                .bind(id)
                .bind(new_token())
        } else {
            sqlx::query_scalar("SELECT invite FROM groups WHERE id = $1").bind(id)
        };
        Ok(query.fetch_optional(&self.pool).await?)
    }

    /// The group behind an invite `token`, whoever asks.
    pub(crate) async fn group_by_invite(&self, token: &str) -> Result<Option<JoinedGroup>> {
        let id: Option<String> = sqlx::query_scalar("SELECT id FROM groups WHERE invite = $1")
            .bind(token)
            .fetch_optional(&self.pool)
            .await?;
        match id {
            Some(id) => self.joined_group(&id).await,
            None => Ok(None),
        }
    }

    /// Adds `user` to the group behind `token` as `name`, or keeps them in
    /// it under their old name.
    pub(crate) async fn join_group(&self, user: &UserKey, token: &str, name: &str) -> Result<Join> {
        let joined: Result<Option<String>, sqlx::Error> = sqlx::query_scalar(
            "INSERT INTO group_members (group_id, user_id, name)
             SELECT id, $2, $3 FROM groups WHERE invite = $1
             ON CONFLICT (group_id, user_id) DO UPDATE SET name = group_members.name
             RETURNING group_id",
        )
        .bind(token)
        .bind(user.as_str())
        .bind(name)
        .fetch_optional(&self.pool)
        .await;
        match joined {
            Ok(Some(id)) => Ok(Join::Joined(id)),
            Ok(None) => Ok(Join::UnknownInvite),
            Err(sqlx::Error::Database(error)) if error.is_unique_violation() => Ok(Join::NameTaken),
            Err(error) => Err(error.into()),
        }
    }

    /// Takes `user` out of group `id`, deleting it when nobody is left.
    /// `false` when they were not in it.
    pub(crate) async fn leave_group(&self, user: &UserKey, id: &str) -> Result<bool> {
        let mut tx = self.pool.begin().await?;
        let left = sqlx::query("DELETE FROM group_members WHERE group_id = $1 AND user_id = $2")
            .bind(id)
            .bind(user.as_str())
            .execute(&mut *tx)
            .await?
            .rows_affected();
        sqlx::query(
            "DELETE FROM groups WHERE id = $1
             AND NOT EXISTS(SELECT 1 FROM group_members WHERE group_id = $1)",
        )
        .bind(id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(left > 0)
    }
}

async fn touch(tx: &mut sqlx::PgConnection, id: &str) -> Result<()> {
    sqlx::query("UPDATE groups SET updated_at = now() WHERE id = $1")
        .bind(id)
        .execute(tx)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::TestDb;

    fn post<'a>(group_id: &'a str, request_id: &'a str, content: &'a str) -> NewMessage<'a> {
        NewMessage {
            group_id,
            request_id,
            content,
        }
    }

    #[tokio::test]
    async fn members_join_by_invite_and_a_reset_breaks_the_old_link() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let (alice, bob) = (UserKey::for_test("alice"), UserKey::for_test("bob"));
        assert!(
            store
                .create_group(&alice, "g1", "Trip", "Ali")
                .await
                .unwrap()
        );
        assert!(!store.create_group(&bob, "g1", "Mine", "Bo").await.unwrap());

        assert!(
            store
                .group_invite(&bob, "g1", false)
                .await
                .unwrap()
                .is_none()
        );
        let token = store
            .group_invite(&alice, "g1", false)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            store.group_invite(&alice, "g1", false).await.unwrap(),
            Some(token.clone())
        );
        assert!(store.group(&bob, "g1").await.unwrap().is_none());
        assert_eq!(
            store.join_group(&bob, &token, "Bo").await.unwrap(),
            Join::Joined("g1".into())
        );
        assert_eq!(
            store
                .join_group(&UserKey::for_test("user"), &token, "bo")
                .await
                .unwrap(),
            Join::NameTaken
        );
        let group = store.group(&bob, "g1").await.unwrap().unwrap();
        let names: Vec<_> = group.members.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, ["Ali", "Bo"]);
        assert!(group.members[0].creator && !group.members[1].creator);

        let fresh = store.group_invite(&bob, "g1", true).await.unwrap().unwrap();
        assert_ne!(fresh, token);
        assert!(store.group_by_invite(&token).await.unwrap().is_none());
        assert_eq!(
            store.join_group(&alice, &token, "x").await.unwrap(),
            Join::UnknownInvite
        );
    }

    #[tokio::test]
    async fn messages_post_once_and_only_from_members() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let (alice, bob) = (UserKey::for_test("alice"), UserKey::for_test("bob"));
        store
            .create_group(&alice, "g1", "Trip", "Ali")
            .await
            .unwrap();
        assert!(
            store
                .post_to_group(&bob, post("g1", "r0", "let me in"))
                .await
                .unwrap()
                .is_none()
        );
        let first = store
            .post_to_group(&alice, post("g1", "r1", "hi"))
            .await
            .unwrap()
            .unwrap();
        let again = store
            .post_to_group(&alice, post("g1", "r1", "hi"))
            .await
            .unwrap()
            .unwrap();
        assert!(first.new && !again.new);
        assert_eq!(first.message.seq, again.message.seq);
        store.post_beep("g1", "hello Ali").await.unwrap();
        let messages = store.group_messages("g1", 10).await.unwrap();
        let texts: Vec<_> = messages
            .iter()
            .map(|m| (m.author_name.as_str(), m.content.as_str()))
            .collect();
        assert_eq!(texts, [("Ali", "hi"), ("Beep", "hello Ali")]);
        assert_eq!(messages[1].author_id, None);
        assert_eq!(
            store.group_messages("g1", 1).await.unwrap()[0].content,
            "hello Ali"
        );
    }

    #[tokio::test]
    async fn the_last_member_out_deletes_the_group() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let (alice, bob) = (UserKey::for_test("alice"), UserKey::for_test("bob"));
        store
            .create_group(&alice, "g1", "Trip", "Ali")
            .await
            .unwrap();
        let token = store
            .group_invite(&alice, "g1", false)
            .await
            .unwrap()
            .unwrap();
        store.join_group(&bob, &token, "Bo").await.unwrap();
        assert_eq!(store.groups(&bob).await.unwrap().len(), 1);
        assert!(store.leave_group(&alice, "g1").await.unwrap());
        assert!(!store.leave_group(&alice, "g1").await.unwrap());
        assert!(store.group_by_invite(&token).await.unwrap().is_some());
        assert!(store.leave_group(&bob, "g1").await.unwrap());
        assert!(store.group_by_invite(&token).await.unwrap().is_none());
    }
}
