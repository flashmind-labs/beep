//! Sessions and groups as one kind of row, and the page of a user's log
//! that `GetUpdates` sends.

use super::{
    Store,
    messages::StoredBody,
    updates::{Bumped, Readers, UpdateOp, log_chat},
};
use crate::{
    protocol::{ChatInfo, ChatKind, GroupMember, Update, UpdatePage},
    user::UserKey,
};
use anyhow::{Result, bail};
use chrono::{DateTime, Utc};
use sqlx::{PgConnection, Row};
use std::collections::HashMap;
use uuid::Uuid;

/// Updates one page holds at most.
const PAGE_UPDATES: i64 = 500;
/// About how many bytes of text a page holds before it stops early.
const PAGE_BYTES: usize = 2 * 1024 * 1024;

/// A session's title as the list shows it: the one it was given, else the
/// start of its first message. Reads `chats c`.
pub(super) const SESSION_TITLE: &str = "COALESCE(c.title, (SELECT substr(r.message, 1, 80)
    FROM runs r WHERE r.chat_id = c.id ORDER BY r.seq LIMIT 1), 'Conversation')";

/// Whose a session is, when a run starts in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SessionChat {
    /// The user's, made now.
    New,
    /// The user's already.
    Owned,
    /// Someone else's, or a group.
    Taken,
}

/// Makes session `chat` for `user` unless it exists, and says whose it is.
pub(super) async fn ensure_session(
    tx: &mut PgConnection,
    user: &str,
    chat: Uuid,
) -> Result<SessionChat> {
    let made =
        sqlx::query("INSERT INTO chats (id, kind) VALUES ($1, 'session') ON CONFLICT DO NOTHING")
            .bind(chat)
            .execute(&mut *tx)
            .await?
            .rows_affected();
    if made == 1 {
        sqlx::query("INSERT INTO chat_members (chat_id, user_id) VALUES ($1, $2)")
            .bind(chat)
            .bind(user)
            .execute(&mut *tx)
            .await?;
        return Ok(SessionChat::New);
    }
    let owned: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM chats c JOIN chat_members m ON m.chat_id = c.id
         WHERE c.id = $1 AND c.kind = 'session' AND m.user_id = $2)",
    )
    .bind(chat)
    .bind(user)
    .fetch_one(&mut *tx)
    .await?;
    Ok(if owned {
        SessionChat::Owned
    } else {
        SessionChat::Taken
    })
}

/// Marks `chat` as changed now.
pub(super) async fn touch(tx: &mut PgConnection, chat: Uuid) -> Result<()> {
    sqlx::query("UPDATE chats SET updated_at = now() WHERE id = $1")
        .bind(chat)
        .execute(tx)
        .await?;
    Ok(())
}

/// What deleting a chat leaves to do once committed.
pub(super) struct DeletedChat {
    /// The files whose bytes go next.
    pub files: Vec<String>,
    pub bumped: Vec<Bumped>,
}

/// Deletes `chat` with everything in it and logs it gone for its members.
pub(super) async fn delete_chat(tx: &mut PgConnection, chat: Uuid) -> Result<DeletedChat> {
    let bumped = log_chat(tx, chat, UpdateOp::ChatDeleted, Readers::Members).await?;
    let files: Vec<String> =
        sqlx::query_scalar("DELETE FROM files WHERE chat_id = $1 RETURNING id")
            .bind(chat)
            .fetch_all(&mut *tx)
            .await?;
    sqlx::query("DELETE FROM chats WHERE id = $1")
        .bind(chat)
        .execute(&mut *tx)
        .await?;
    Ok(DeletedChat { files, bumped })
}

/// One row of a user's log.
struct Logged {
    update_id: i64,
    chat_id: Uuid,
    message_id: Option<i64>,
    op: UpdateOp,
}

impl Store {
    /// Whether `user` is in `chat`, a session they own or a group they are in.
    pub(crate) async fn in_chat(&self, user: &UserKey, chat: Uuid) -> Result<bool> {
        Ok(sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM chat_members WHERE chat_id = $1 AND user_id = $2)",
        )
        .bind(chat)
        .bind(user.as_str())
        .fetch_one(&self.pool)
        .await?)
    }

    /// Whether `chat` is a session `user` owns.
    pub(crate) async fn owns_session(&self, user: &UserKey, chat: Uuid) -> Result<bool> {
        Ok(sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM chats c JOIN chat_members m ON m.chat_id = c.id
             WHERE c.id = $1 AND c.kind = 'session' AND m.user_id = $2)",
        )
        .bind(chat)
        .bind(user.as_str())
        .fetch_one(&self.pool)
        .await?)
    }

    /// The chats `ids` as `user` sees them. Chats they are not in are left out.
    pub(crate) async fn chat_infos(&self, user: &UserKey, ids: &[Uuid]) -> Result<Vec<ChatInfo>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let rows = sqlx::query(&format!(
            "SELECT c.id, c.kind::text, CASE WHEN c.kind = 'session' THEN {SESSION_TITLE}
                 ELSE COALESCE(c.title, '') END, m.pinned, m.shares_apps
             FROM chats c JOIN chat_members m ON m.chat_id = c.id AND m.user_id = $1
             WHERE c.id = ANY($2)"
        ))
        .bind(user.as_str())
        .bind(ids)
        .fetch_all(&self.pool)
        .await?;
        let members = sqlx::query(
            "SELECT chat_id, user_id, name, creator, joined_at FROM chat_members
             WHERE chat_id = ANY($1) AND name IS NOT NULL ORDER BY chat_id, joined_at, user_id",
        )
        .bind(ids)
        .fetch_all(&self.pool)
        .await?;
        let mut by_chat: HashMap<Uuid, Vec<GroupMember>> = HashMap::new();
        for row in members {
            let member: String = row.try_get(1)?;
            let joined_at: DateTime<Utc> = row.try_get(4)?;
            by_chat
                .entry(row.try_get(0)?)
                .or_default()
                .push(GroupMember {
                    name: row.try_get(2)?,
                    you: member == user.as_str(),
                    creator: row.try_get(3)?,
                    joined_at: joined_at.timestamp_millis(),
                });
        }
        rows.iter()
            .map(|row| {
                let id: Uuid = row.try_get(0)?;
                let kind = match row.try_get::<&str, _>(1)? {
                    "session" => ChatKind::Session {},
                    "group" => ChatKind::Group {},
                    other => bail!("unknown chat kind {other}"),
                };
                Ok(ChatInfo {
                    id,
                    members: match kind {
                        ChatKind::Group {} => by_chat.remove(&id).unwrap_or_default(),
                        ChatKind::Session {} => Vec::new(),
                    },
                    kind,
                    title: row.try_get(2)?,
                    pinned: row.try_get(3)?,
                    apps_shared: row.try_get(4)?,
                })
            })
            .collect()
    }

    /// `user`'s updates after `after`, oldest first, as many as fit a page.
    /// A client behind the tombstones already dropped gets everything again
    /// with `reset`.
    pub(crate) async fn updates_page(&self, user: &UserKey, after: i64) -> Result<UpdatePage> {
        let pruned_below: Option<i64> =
            sqlx::query_scalar("SELECT pruned_below FROM sync_counters WHERE user_id = $1")
                .bind(user.as_str())
                .fetch_optional(&self.pool)
                .await?;
        let reset = after > 0 && after < pruned_below.unwrap_or(0) - 1;
        let after = if reset { 0 } else { after.max(0) };
        // Each write holds the user's counter until it commits, so whatever
        // is visible here has no gaps below it still to fill in.
        let rows = sqlx::query(
            "SELECT update_id, chat_id, message_id, op::text FROM updates
             WHERE user_id = $1 AND update_id > $2 ORDER BY update_id LIMIT $3",
        )
        .bind(user.as_str())
        .bind(after)
        .bind(PAGE_UPDATES)
        .fetch_all(&self.pool)
        .await?;
        let full = rows.len() as i64 == PAGE_UPDATES;
        let logged = rows
            .iter()
            .map(|row| {
                Ok(Logged {
                    update_id: row.try_get(0)?,
                    chat_id: row.try_get(1)?,
                    message_id: row.try_get(2)?,
                    op: UpdateOp::parse(row.try_get(3)?)?,
                })
            })
            .collect::<Result<Vec<_>>>()?;

        let wanted: Vec<i64> = logged
            .iter()
            .filter(|row| row.op == UpdateOp::Message)
            .filter_map(|row| row.message_id)
            .collect();
        let mut messages: HashMap<i64, _> = self
            .messages_by_id(&wanted)
            .await?
            .into_iter()
            .map(|message| (message.id, message))
            .collect();
        let chats: Vec<Uuid> = logged
            .iter()
            .filter(|row| row.op == UpdateOp::Chat)
            .map(|row| row.chat_id)
            .collect();
        let mut infos: HashMap<Uuid, ChatInfo> = self
            .chat_infos(user, &chats)
            .await?
            .into_iter()
            .map(|info| (info.id, info))
            .collect();

        let mut page = UpdatePage {
            updates: Vec::with_capacity(logged.len()),
            last: after,
            more: full,
            reset,
        };
        let mut bytes = 0;
        for row in logged {
            if bytes >= PAGE_BYTES {
                page.more = true;
                break;
            }
            page.last = row.update_id;
            let update = match (row.op, row.message_id) {
                (UpdateOp::Message, Some(id)) => {
                    // Deleted since: its tombstone comes later in the log.
                    let Some(message) = messages.remove(&id) else {
                        continue;
                    };
                    bytes += size(&message.body) + 64;
                    Update::Message {
                        update_id: row.update_id,
                        chat_id: row.chat_id,
                        message: message.to_wire(user.as_str()),
                    }
                }
                (UpdateOp::Deleted, Some(message_id)) => Update::Deleted {
                    update_id: row.update_id,
                    chat_id: row.chat_id,
                    message_id,
                },
                (UpdateOp::Chat, _) => {
                    let Some(chat) = infos.remove(&row.chat_id) else {
                        continue;
                    };
                    bytes += 256;
                    Update::Chat {
                        update_id: row.update_id,
                        chat,
                    }
                }
                (UpdateOp::ChatDeleted, _) => Update::ChatDeleted {
                    update_id: row.update_id,
                    chat_id: row.chat_id,
                },
                (UpdateOp::Message | UpdateOp::Deleted, None) => continue,
            };
            page.updates.push(update);
        }
        Ok(page)
    }
}

/// About how many bytes `body` takes on the wire.
fn size(body: &StoredBody) -> usize {
    match body {
        StoredBody::User { content, turn } => content.len() + turn.reasoning.len(),
        StoredBody::Scheduled { label, turn } => label.len() + turn.reasoning.len(),
        StoredBody::Assistant { content }
        | StoredBody::Member { content, .. }
        | StoredBody::Beep { content } => content.len(),
        StoredBody::Tool { output, .. } => output.len(),
        StoredBody::Approval { approval, .. } => approval
            .fields
            .iter()
            .map(|field| field.label.len() + field.value.len())
            .sum(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        protocol::MessageBody,
        store::{NewMessage, TestDb, TurnsToDelete, test_chat, test_turn},
    };

    /// A page as short labels, oldest first.
    fn labels(page: &UpdatePage) -> Vec<String> {
        page.updates
            .iter()
            .map(|update| match update {
                Update::Message { message, .. } => match &message.body {
                    MessageBody::User { content, .. } => format!("user:{content}"),
                    MessageBody::Assistant { content } => format!("beep:{content}"),
                    MessageBody::Member { content, .. } => format!("member:{content}"),
                    MessageBody::Beep { content } => format!("beep:{content}"),
                    other => format!("{other:?}"),
                },
                Update::Deleted { .. } => "deleted".to_owned(),
                Update::Chat { .. } => "chat".to_owned(),
                Update::ChatDeleted { .. } => "chat_deleted".to_owned(),
            })
            .collect()
    }

    #[tokio::test]
    async fn a_client_gets_what_it_lacks_and_hears_of_deletes() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let (alice, bob) = (UserKey::for_test("alice"), UserKey::for_test("bob"));
        let s1 = test_chat("s1");
        test_turn(&store, &alice, s1, "one", 1000).await.unwrap();
        test_turn(&store, &alice, s1, "two", 2000).await.unwrap();

        let all = store.updates_page(&alice, 0).await.unwrap();
        assert_eq!(
            labels(&all),
            [
                "chat",
                "user:one",
                "beep:re: one",
                "user:two",
                "beep:re: two"
            ]
        );
        assert!(!all.more && !all.reset);
        let ids: Vec<i64> = all
            .updates
            .iter()
            .map(|update| match update {
                Update::Message { update_id, .. }
                | Update::Deleted { update_id, .. }
                | Update::Chat { update_id, .. }
                | Update::ChatDeleted { update_id, .. } => *update_id,
            })
            .collect();
        assert!(ids.windows(2).all(|pair| pair[0] < pair[1]));
        assert_eq!(all.last, *ids.last().unwrap());

        let none = store.updates_page(&alice, all.last).await.unwrap();
        assert!(none.updates.is_empty());
        assert_eq!(none.last, all.last);
        assert!(
            store
                .updates_page(&bob, 0)
                .await
                .unwrap()
                .updates
                .is_empty()
        );

        store
            .delete_turn(&alice, s1, 1000, TurnsToDelete::One)
            .await
            .unwrap();
        let since = store.updates_page(&alice, all.last).await.unwrap();
        assert_eq!(labels(&since), ["deleted", "deleted"]);
        // A client starting now never sees the deleted messages.
        let fresh = store.updates_page(&alice, 0).await.unwrap();
        assert_eq!(
            labels(&fresh),
            ["chat", "user:two", "beep:re: two", "deleted", "deleted"]
        );
    }

    #[tokio::test]
    async fn group_members_share_one_copy_and_newcomers_get_the_backlog() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let (alice, bob) = (UserKey::for_test("alice"), UserKey::for_test("bob"));
        let g1 = test_chat("g1");
        store.create_group(&alice, g1, "Trip", "Ali").await.unwrap();
        store
            .post_to_group(
                &alice,
                NewMessage {
                    group_id: g1,
                    request_id: test_chat("r1"),
                    content: "hi",
                },
            )
            .await
            .unwrap();
        store.post_beep(g1, "hello Ali").await.unwrap();
        let before = store.updates_page(&alice, 0).await.unwrap();
        assert_eq!(labels(&before), ["chat", "member:hi", "beep:hello Ali"]);

        let token = store
            .group_invite(&alice, g1, false)
            .await
            .unwrap()
            .unwrap();
        store.join_group(&bob, &token, "Bo").await.unwrap();
        let joined = store.updates_page(&bob, 0).await.unwrap();
        assert_eq!(labels(&joined), ["chat", "member:hi", "beep:hello Ali"]);
        let Some(Update::Chat { chat, .. }) = joined.updates.first() else {
            panic!("no chat first: {joined:?}");
        };
        assert_eq!(chat.members.len(), 2);
        assert_eq!(
            labels(&store.updates_page(&alice, before.last).await.unwrap()),
            ["chat"]
        );

        let seen = store.last_update(alice.as_str()).await.unwrap();
        assert!(store.leave_group(&bob, g1).await.unwrap());
        assert_eq!(
            labels(&store.updates_page(&bob, 0).await.unwrap()),
            ["chat_deleted"]
        );
        assert_eq!(
            labels(&store.updates_page(&alice, seen).await.unwrap()),
            ["chat"]
        );
    }

    #[tokio::test]
    async fn a_client_behind_dropped_tombstones_starts_over() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let alice = UserKey::for_test("alice");
        let s1 = test_chat("s1");
        test_turn(&store, &alice, s1, "one", 1000).await.unwrap();
        test_turn(&store, &alice, s1, "two", 2000).await.unwrap();
        store
            .delete_turn(&alice, s1, 1000, TurnsToDelete::One)
            .await
            .unwrap();
        let last = store.last_update(alice.as_str()).await.unwrap();
        assert_eq!(store.prune_updates().await.unwrap(), 0);

        sqlx::query(
            "UPDATE updates SET updated_at = now() - interval '40 days' WHERE op = 'deleted'",
        )
        .execute(store.pool())
        .await
        .unwrap();
        assert_eq!(store.prune_updates().await.unwrap(), 1);
        let behind = store.updates_page(&alice, 1).await.unwrap();
        assert!(behind.reset);
        assert_eq!(labels(&behind), ["chat", "user:two", "beep:re: two"]);
        let current = store.updates_page(&alice, last).await.unwrap();
        assert!(!current.reset && current.updates.is_empty());
        let ahead = store.updates_page(&alice, i64::MAX).await.unwrap();
        assert!(!ahead.reset && ahead.updates.is_empty());
        assert!(!store.updates_page(&alice, 0).await.unwrap().reset);
    }

    #[tokio::test]
    async fn a_write_wakes_the_users_watchers() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let alice = UserKey::for_test("alice");
        let mut watch = store.watch_updates(alice.as_str()).await.unwrap();
        assert_eq!(*watch.borrow_and_update(), 0);
        test_turn(&store, &alice, test_chat("s1"), "one", 1000)
            .await
            .unwrap();
        assert!(watch.has_changed().unwrap());
        assert_eq!(
            *watch.borrow_and_update(),
            store.last_update(alice.as_str()).await.unwrap()
        );
    }
}
