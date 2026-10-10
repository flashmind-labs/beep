//! Group chats: their members, messages and invite links.

use super::{
    Store,
    chats::{DeletedChat, delete_chat, touch},
    updates::{Readers, UpdateOp, log_chat, log_messages},
};
use crate::user::UserKey;
use anyhow::Result;
use chrono::{DateTime, Utc};
use sqlx::{PgConnection, Row, postgres::PgRow};
use uuid::Uuid;

/// Messages a member who joins gets in their log, the ones a group chat
/// shows at once.
pub(crate) const JOIN_BACKLOG: i64 = 200;

/// A group's own columns.
#[derive(Debug, Clone)]
pub struct GroupRow {
    pub id: Uuid,
    pub name: String,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct MemberRow {
    pub user_id: String,
    pub name: String,
    pub creator: bool,
    pub joined_at: DateTime<Utc>,
    /// Pinned to the top of this member's list.
    pub pinned: bool,
    /// Beep may use this member's apps when someone else asks.
    pub shares_apps: bool,
}

#[derive(Debug, Clone)]
pub struct MessageRow {
    /// Grows with each message, so it orders the group's messages.
    pub id: i64,
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
    Joined(Uuid),
    /// Someone in the group already goes by the name.
    NameTaken,
    /// No group has the token.
    UnknownInvite,
}

/// A member's message to post.
pub struct NewMessage<'a> {
    pub group_id: Uuid,
    /// The client's id for it, so a retry posts once.
    pub request_id: Uuid,
    pub content: &'a str,
}

const MESSAGE_COLUMNS: &str = "id, author_id, COALESCE(author_name, ''), content, sent_at";

fn message(row: &PgRow) -> Result<MessageRow> {
    Ok(MessageRow {
        id: row.try_get(0)?,
        author_id: row.try_get(1)?,
        author_name: row.try_get(2)?,
        content: row.try_get(3)?,
        sent_at: row.try_get(4)?,
    })
}

/// Letters and digits that read the same aloud and in any font: no 0/O,
/// 1/I/L or U/V.
const CODE_ALPHABET: &[u8] = b"23456789ABCDEFGHJKMNPQRSTWXYZ";

/// A new invite code, ten characters from [`CODE_ALPHABET`] so it can be
/// typed: about 48 bits from a v4 UUID's random ones.
fn new_token() -> String {
    let base = CODE_ALPHABET.len() as u128;
    let mut random = Uuid::new_v4().as_u128();
    (0..10)
        .map(|_| {
            let c = CODE_ALPHABET[(random % base) as usize];
            random /= base;
            char::from(c)
        })
        .collect()
}

/// Whether `user` is in group `id`.
async fn member_of(tx: &mut PgConnection, user: &str, id: Uuid) -> Result<bool> {
    Ok(sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM chats c JOIN chat_members m ON m.chat_id = c.id
         WHERE c.id = $1 AND c.kind = 'group' AND m.user_id = $2)",
    )
    .bind(id)
    .bind(user)
    .fetch_one(tx)
    .await?)
}

impl Store {
    /// Makes the group `id` with `user` as its creator. `false` when the id
    /// is taken.
    pub(crate) async fn create_group(
        &self,
        user: &UserKey,
        id: Uuid,
        name: &str,
        member_name: &str,
    ) -> Result<bool> {
        let mut tx = self.pool.begin().await?;
        let created = sqlx::query(
            "INSERT INTO chats (id, kind, title) VALUES ($1, 'group', $2) ON CONFLICT DO NOTHING",
        )
        .bind(id)
        .bind(name)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if created == 0 {
            tx.rollback().await?;
            return Ok(false);
        }
        sqlx::query("INSERT INTO group_invites (chat_id, invite) VALUES ($1, $2)")
            .bind(id)
            .bind(new_token())
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            "INSERT INTO chat_members (chat_id, user_id, name, creator) VALUES ($1, $2, $3, TRUE)",
        )
        .bind(id)
        .bind(user.as_str())
        .bind(member_name)
        .execute(&mut *tx)
        .await?;
        let bumped = log_chat(&mut tx, id, UpdateOp::Chat, Readers::Members).await?;
        tx.commit().await?;
        self.announce(&bumped);
        Ok(true)
    }

    /// The groups `user` is in, most recently active first.
    pub(crate) async fn groups(&self, user: &UserKey) -> Result<Vec<JoinedGroup>> {
        let ids: Vec<Uuid> = sqlx::query_scalar(
            "SELECT c.id FROM chats c JOIN chat_members m ON m.chat_id = c.id
             WHERE c.kind = 'group' AND m.user_id = $1 ORDER BY c.updated_at DESC LIMIT 200",
        )
        .bind(user.as_str())
        .fetch_all(&self.pool)
        .await?;
        let mut groups = Vec::with_capacity(ids.len());
        for id in ids {
            if let Some(group) = self.joined_group(id).await? {
                groups.push(group);
            }
        }
        Ok(groups)
    }

    /// The group `id`, if `user` is in it.
    pub(crate) async fn group(&self, user: &UserKey, id: Uuid) -> Result<Option<JoinedGroup>> {
        if !self.is_member(user, id).await? {
            return Ok(None);
        }
        self.joined_group(id).await
    }

    pub(crate) async fn is_member(&self, user: &UserKey, id: Uuid) -> Result<bool> {
        let mut conn = self.pool.acquire().await?;
        member_of(&mut conn, user.as_str(), id).await
    }

    async fn joined_group(&self, id: Uuid) -> Result<Option<JoinedGroup>> {
        let row = sqlx::query(
            "SELECT id, COALESCE(title, ''), updated_at FROM chats WHERE id = $1 AND kind = 'group'",
        )
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
    pub(crate) async fn group_members(&self, id: Uuid) -> Result<Vec<MemberRow>> {
        let rows = sqlx::query(
            "SELECT user_id, COALESCE(name, ''), creator, joined_at, pinned, shares_apps
             FROM chat_members WHERE chat_id = $1 ORDER BY joined_at, user_id",
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
                    pinned: row.try_get(4)?,
                    shares_apps: row.try_get(5)?,
                })
            })
            .collect()
    }

    /// The last `limit` messages of group `id`, oldest first. Callers check
    /// membership first.
    pub(crate) async fn group_messages(&self, id: Uuid, limit: i64) -> Result<Vec<MessageRow>> {
        self.group_messages_until(id, i64::MAX, limit).await
    }

    /// The last `limit` messages of group `id` up to and including message
    /// `until`, oldest first.
    pub(crate) async fn group_messages_until(
        &self,
        id: Uuid,
        until: i64,
        limit: i64,
    ) -> Result<Vec<MessageRow>> {
        let rows = sqlx::query(&format!(
            "SELECT * FROM (SELECT {MESSAGE_COLUMNS} FROM messages
             WHERE chat_id = $1 AND id <= $2 ORDER BY id DESC LIMIT $3) recent ORDER BY id"
        ))
        .bind(id)
        .bind(until)
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
        let name: Option<Option<String>> = sqlx::query_scalar(
            "SELECT m.name FROM chat_members m JOIN chats c ON c.id = m.chat_id
             WHERE m.chat_id = $1 AND m.user_id = $2 AND c.kind = 'group'",
        )
        .bind(new.group_id)
        .bind(user.as_str())
        .fetch_optional(&mut *tx)
        .await?;
        let Some(name) = name else {
            tx.rollback().await?;
            return Ok(None);
        };
        let inserted = sqlx::query(&format!(
            "INSERT INTO messages (chat_id, role, author_id, author_name, request_id, content)
             VALUES ($1, 'member', $2, $3, $4, $5) ON CONFLICT DO NOTHING
             RETURNING {MESSAGE_COLUMNS}"
        ))
        .bind(new.group_id)
        .bind(user.as_str())
        .bind(name.unwrap_or_default())
        .bind(new.request_id)
        .bind(new.content)
        .fetch_optional(&mut *tx)
        .await?;
        let fresh = inserted.is_some();
        let mut bumped = Vec::new();
        let row =
            match inserted {
                Some(row) => {
                    touch(&mut tx, new.group_id).await?;
                    let id: i64 = row.try_get(0)?;
                    bumped = log_messages(
                        &mut tx,
                        new.group_id,
                        &[id],
                        UpdateOp::Message,
                        Readers::Members,
                    )
                    .await?;
                    row
                }
                None => sqlx::query(&format!(
                    "SELECT {MESSAGE_COLUMNS} FROM messages WHERE chat_id = $1 AND request_id = $2"
                ))
                .bind(new.group_id)
                .bind(new.request_id)
                .fetch_one(&mut *tx)
                .await?,
            };
        tx.commit().await?;
        self.announce(&bumped);
        Ok(Some(Posted {
            message: message(&row)?,
            new: fresh,
        }))
    }

    /// Posts Beep's reply to group `id`.
    pub(crate) async fn post_beep(&self, id: Uuid, content: &str) -> Result<MessageRow> {
        let mut tx = self.pool.begin().await?;
        let row = sqlx::query(&format!(
            "INSERT INTO messages (chat_id, role, author_name, content)
             VALUES ($1, 'beep', 'Beep', $2) RETURNING {MESSAGE_COLUMNS}"
        ))
        .bind(id)
        .bind(content)
        .fetch_one(&mut *tx)
        .await?;
        touch(&mut tx, id).await?;
        let posted = message(&row)?;
        let bumped = log_messages(
            &mut tx,
            id,
            &[posted.id],
            UpdateOp::Message,
            Readers::Members,
        )
        .await?;
        tx.commit().await?;
        self.announce(&bumped);
        Ok(posted)
    }

    /// The invite token of group `id`, a new one with `reset`. `None` when
    /// `user` is not in it.
    pub(crate) async fn group_invite(
        &self,
        user: &UserKey,
        id: Uuid,
        reset: bool,
    ) -> Result<Option<String>> {
        if !self.is_member(user, id).await? {
            return Ok(None);
        }
        let query = if reset {
            sqlx::query_scalar(
                "UPDATE group_invites SET invite = $2 WHERE chat_id = $1 RETURNING invite",
            )
            .bind(id)
            .bind(new_token())
        } else {
            sqlx::query_scalar("SELECT invite FROM group_invites WHERE chat_id = $1").bind(id)
        };
        Ok(query.fetch_optional(&self.pool).await?)
    }

    /// The group behind an invite `token`, whoever asks.
    pub(crate) async fn group_by_invite(&self, token: &str) -> Result<Option<JoinedGroup>> {
        let id: Option<Uuid> =
            sqlx::query_scalar("SELECT chat_id FROM group_invites WHERE invite = $1")
                .bind(token)
                .fetch_optional(&self.pool)
                .await?;
        match id {
            Some(id) => self.joined_group(id).await,
            None => Ok(None),
        }
    }

    /// Adds `user` to the group behind `token` as `name`, or keeps them in
    /// it under their old name. A new member gets the group's recent
    /// messages in their log, and everyone the new member list.
    pub(crate) async fn join_group(&self, user: &UserKey, token: &str, name: &str) -> Result<Join> {
        let mut tx = self.pool.begin().await?;
        let joined: Result<Option<Uuid>, sqlx::Error> = sqlx::query_scalar(
            "INSERT INTO chat_members (chat_id, user_id, name)
             SELECT chat_id, $2, $3 FROM group_invites WHERE invite = $1
             ON CONFLICT (chat_id, user_id) DO UPDATE SET name = chat_members.name
             RETURNING chat_id",
        )
        .bind(token)
        .bind(user.as_str())
        .bind(name)
        .fetch_optional(&mut *tx)
        .await;
        let id = match joined {
            Ok(Some(id)) => id,
            Ok(None) => {
                tx.rollback().await?;
                return Ok(Join::UnknownInvite);
            }
            Err(sqlx::Error::Database(error)) if error.is_unique_violation() => {
                tx.rollback().await?;
                return Ok(Join::NameTaken);
            }
            Err(error) => return Err(error.into()),
        };
        let backlog: Vec<i64> = sqlx::query_scalar(
            "SELECT id FROM (SELECT id FROM messages WHERE chat_id = $1 ORDER BY id DESC LIMIT $2)
             recent ORDER BY id",
        )
        .bind(id)
        .bind(JOIN_BACKLOG)
        .fetch_all(&mut *tx)
        .await?;
        let mut bumped = log_chat(&mut tx, id, UpdateOp::Chat, Readers::Members).await?;
        bumped.extend(
            log_messages(
                &mut tx,
                id,
                &backlog,
                UpdateOp::Message,
                Readers::User(user.as_str()),
            )
            .await?,
        );
        tx.commit().await?;
        self.announce(&bumped);
        Ok(Join::Joined(id))
    }

    /// Pins or unpins group `id` in `user`'s list. `false` when they are not in it.
    pub(crate) async fn pin_group(&self, user: &UserKey, id: Uuid, pinned: bool) -> Result<bool> {
        self.set_member_flag(user, id, "pinned", pinned).await
    }

    /// Lets Beep use `user`'s apps in group `id` when others ask, or stops it.
    /// `false` when they are not in it.
    pub(crate) async fn share_group_apps(
        &self,
        user: &UserKey,
        id: Uuid,
        shared: bool,
    ) -> Result<bool> {
        self.set_member_flag(user, id, "shares_apps", shared).await
    }

    /// Sets `user`'s own `column` for chat `id` and sends them the chat
    /// again. `false` when they are not in it.
    pub(super) async fn set_member_flag(
        &self,
        user: &UserKey,
        id: Uuid,
        column: &'static str,
        value: bool,
    ) -> Result<bool> {
        let mut tx = self.pool.begin().await?;
        let changed = sqlx::query(&format!(
            "UPDATE chat_members SET {column} = $3 WHERE chat_id = $1 AND user_id = $2"
        ))
        .bind(id)
        .bind(user.as_str())
        .bind(value)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if changed == 0 {
            tx.rollback().await?;
            return Ok(false);
        }
        let bumped = log_chat(&mut tx, id, UpdateOp::Chat, Readers::User(user.as_str())).await?;
        tx.commit().await?;
        self.announce(&bumped);
        Ok(true)
    }

    /// Takes `user` out of group `id`, deleting it and its pictures when
    /// nobody is left.
    /// `false` when they were not in it.
    pub(crate) async fn leave_group(&self, user: &UserKey, id: Uuid) -> Result<bool> {
        let mut tx = self.pool.begin().await?;
        if !member_of(&mut tx, user.as_str(), id).await? {
            tx.rollback().await?;
            return Ok(false);
        }
        let mut bumped = log_chat(
            &mut tx,
            id,
            UpdateOp::ChatDeleted,
            Readers::User(user.as_str()),
        )
        .await?;
        sqlx::query("DELETE FROM chat_members WHERE chat_id = $1 AND user_id = $2")
            .bind(id)
            .bind(user.as_str())
            .execute(&mut *tx)
            .await?;
        let left: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM chat_members WHERE chat_id = $1)")
                .bind(id)
                .fetch_one(&mut *tx)
                .await?;
        let mut files = Vec::new();
        if left {
            bumped.extend(log_chat(&mut tx, id, UpdateOp::Chat, Readers::Members).await?);
        } else {
            let deleted: DeletedChat = delete_chat(&mut tx, id).await?;
            files = deleted.files;
        }
        tx.commit().await?;
        self.announce(&bumped);
        self.blobs.delete(&files).await?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{TestDb, test_chat};

    fn post<'a>(group_id: Uuid, request: &str, content: &'a str) -> NewMessage<'a> {
        NewMessage {
            group_id,
            request_id: test_chat(request),
            content,
        }
    }

    #[test]
    fn invite_codes_are_ten_typeable_characters() {
        let code = new_token();
        assert_eq!(code.len(), 10);
        assert!(code.bytes().all(|b| CODE_ALPHABET.contains(&b)));
        assert_ne!(code, new_token());
    }

    #[tokio::test]
    async fn members_join_by_invite_and_a_reset_breaks_the_old_link() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let g1 = test_chat("g1");
        let (alice, bob) = (UserKey::for_test("alice"), UserKey::for_test("bob"));
        assert!(store.create_group(&alice, g1, "Trip", "Ali").await.unwrap());
        assert!(!store.create_group(&bob, g1, "Mine", "Bo").await.unwrap());

        assert!(store.group_invite(&bob, g1, false).await.unwrap().is_none());
        let token = store
            .group_invite(&alice, g1, false)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            store.group_invite(&alice, g1, false).await.unwrap(),
            Some(token.clone())
        );
        assert!(store.group(&bob, g1).await.unwrap().is_none());
        assert_eq!(
            store.join_group(&bob, &token, "Bo").await.unwrap(),
            Join::Joined(g1)
        );
        assert_eq!(
            store
                .join_group(&UserKey::for_test("user"), &token, "bo")
                .await
                .unwrap(),
            Join::NameTaken
        );
        let group = store.group(&bob, g1).await.unwrap().unwrap();
        let names: Vec<_> = group.members.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, ["Ali", "Bo"]);
        assert!(group.members[0].creator && !group.members[1].creator);

        let fresh = store.group_invite(&bob, g1, true).await.unwrap().unwrap();
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
        let g1 = test_chat("g1");
        let (alice, bob) = (UserKey::for_test("alice"), UserKey::for_test("bob"));
        store.create_group(&alice, g1, "Trip", "Ali").await.unwrap();
        assert!(
            store
                .post_to_group(&bob, post(g1, "r0", "let me in"))
                .await
                .unwrap()
                .is_none()
        );
        let first = store
            .post_to_group(&alice, post(g1, "r1", "hi"))
            .await
            .unwrap()
            .unwrap();
        let again = store
            .post_to_group(&alice, post(g1, "r1", "hi"))
            .await
            .unwrap()
            .unwrap();
        assert!(first.new && !again.new);
        assert_eq!(first.message.id, again.message.id);
        store.post_beep(g1, "hello Ali").await.unwrap();
        let messages = store.group_messages(g1, 10).await.unwrap();
        let texts: Vec<_> = messages
            .iter()
            .map(|m| (m.author_name.as_str(), m.content.as_str()))
            .collect();
        assert_eq!(texts, [("Ali", "hi"), ("Beep", "hello Ali")]);
        assert_eq!(messages[1].author_id, None);
        assert_eq!(
            store.group_messages(g1, 1).await.unwrap()[0].content,
            "hello Ali"
        );
    }

    #[tokio::test]
    async fn the_last_member_out_deletes_the_group() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let g1 = test_chat("g1");
        let (alice, bob) = (UserKey::for_test("alice"), UserKey::for_test("bob"));
        store.create_group(&alice, g1, "Trip", "Ali").await.unwrap();
        let token = store
            .group_invite(&alice, g1, false)
            .await
            .unwrap()
            .unwrap();
        store.join_group(&bob, &token, "Bo").await.unwrap();
        assert_eq!(store.groups(&bob).await.unwrap().len(), 1);
        assert!(store.pin_group(&bob, g1, true).await.unwrap());
        let members = store.group_members(g1).await.unwrap();
        let pinned: Vec<bool> = members.iter().map(|member| member.pinned).collect();
        assert_eq!(pinned, [false, true]);
        assert!(members.iter().all(|member| member.shares_apps));
        assert!(store.share_group_apps(&bob, g1, false).await.unwrap());
        let members = store.group_members(g1).await.unwrap();
        let shared: Vec<bool> = members.iter().map(|member| member.shares_apps).collect();
        assert_eq!(shared, [true, false]);
        assert!(
            !store
                .pin_group(&UserKey::for_test("carol"), g1, true)
                .await
                .unwrap()
        );
        assert!(store.leave_group(&alice, g1).await.unwrap());
        assert!(!store.leave_group(&alice, g1).await.unwrap());
        assert!(store.group_by_invite(&token).await.unwrap().is_some());
        assert!(store.leave_group(&bob, g1).await.unwrap());
        assert!(store.group_by_invite(&token).await.unwrap().is_none());
    }
}
