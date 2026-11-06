//! Each user's update log.
//! Writes take ids from `sync_counters` under its row lock, so updates commit in id order and none are missed.

use super::Store;
use anyhow::Result;
use sqlx::{PgConnection, Row, postgres::PgRow};
use std::{
    collections::HashMap,
    sync::{Mutex, PoisonError},
    time::Duration,
};
use tokio::sync::watch;
use uuid::Uuid;

/// How long a tombstone stays before a client that missed it must start over.
const TOMBSTONE_KEEP: Duration = Duration::from_secs(30 * 24 * 60 * 60);

/// How often old tombstones are dropped.
const PRUNE_EVERY: Duration = Duration::from_secs(60 * 60);

/// Drops tombstones past `TOMBSTONE_KEEP`, ended grants and old notices, once an hour.
pub fn start_pruning(store: Store) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(PRUNE_EVERY);
        loop {
            interval.tick().await;
            match store.prune_updates().await {
                Ok(0) => {}
                Ok(users) => log::info!("pruned old tombstones of {users} user(s)"),
                Err(error) => log::warn!("unable to prune tombstones: {error}"),
            }
            match store.prune_grants().await {
                Ok(0) => {}
                Ok(rows) => log::info!("pruned {rows} ended grant(s) and old grant uses"),
                Err(error) => log::warn!("unable to prune grants: {error:#}"),
            }
            match store.prune_notices().await {
                Ok(0) => {}
                Ok(rows) => log::info!("pruned {rows} old notice(s)"),
                Err(error) => log::warn!("unable to prune notices: {error:#}"),
            }
        }
    });
}

/// What happened to a message or chat.
#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::IntoStaticStr, strum::EnumString)]
#[strum(serialize_all = "snake_case")]
pub enum UpdateOp {
    Message,
    Deleted,
    Chat,
    ChatDeleted,
}

impl UpdateOp {
    pub fn as_str(self) -> &'static str {
        self.into()
    }

    pub(super) fn parse(op: &str) -> Result<Self> {
        op.parse()
            .map_err(|_| anyhow::anyhow!("unknown update op {op}"))
    }
}

/// A user's newest update id after a write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bumped {
    pub user_id: String,
    pub last: i64,
}

/// Who gets an update: everyone in the chat, or one user.
#[derive(Debug, Clone, Copy)]
pub enum Readers<'a> {
    Members,
    User(&'a str),
}

impl<'a> Readers<'a> {
    /// The readers of chat `$1` as a query with the user as parameter `user`, ordered so counter locks cannot deadlock.
    fn query(self, user: usize) -> String {
        match self {
            Self::Members => {
                "SELECT user_id FROM chat_members WHERE chat_id = $1 ORDER BY user_id".to_owned()
            }
            Self::User(_) => format!("SELECT ${user}::text AS user_id"),
        }
    }

    fn user(self) -> Option<&'a str> {
        match self {
            Self::Members => None,
            Self::User(user) => Some(user),
        }
    }
}

/// Logs `op` for `messages` of `chat` to each reader under new ids in order; existing entries move to the new id.
pub(super) async fn log_messages(
    tx: &mut PgConnection,
    chat: Uuid,
    messages: &[i64],
    op: UpdateOp,
    readers: Readers<'_>,
) -> Result<Vec<Bumped>> {
    if messages.is_empty() {
        return Ok(Vec::new());
    }
    let rows = sqlx::query(&format!(
        "WITH bumped AS (
             INSERT INTO sync_counters (user_id, last)
             SELECT user_id, cardinality($2::bigint[]) FROM ({readers}) readers
             ON CONFLICT (user_id) DO UPDATE SET last = sync_counters.last + excluded.last
             RETURNING user_id, last
         ), logged AS (
             INSERT INTO updates (user_id, update_id, chat_id, message_id, op)
             SELECT b.user_id, b.last - cardinality($2::bigint[]) + m.n, $1, m.id, $3::update_op
             FROM bumped b CROSS JOIN unnest($2::bigint[]) WITH ORDINALITY m (id, n)
             ON CONFLICT (user_id, message_id) WHERE message_id IS NOT NULL
             DO UPDATE SET update_id = excluded.update_id, op = excluded.op, updated_at = now()
         )
         SELECT user_id, last FROM bumped",
        readers = readers.query(4)
    ))
    .bind(chat)
    .bind(messages)
    .bind(op.as_str())
    .bind(readers.user())
    .fetch_all(&mut *tx)
    .await?;
    bumped(&rows)
}

/// Writes `op` for `chat` itself to the log of each reader.
pub(super) async fn log_chat(
    tx: &mut PgConnection,
    chat: Uuid,
    op: UpdateOp,
    readers: Readers<'_>,
) -> Result<Vec<Bumped>> {
    // A deleted chat's messages need no tombstones of their own.
    if op == UpdateOp::ChatDeleted {
        sqlx::query(&format!(
            "DELETE FROM updates WHERE chat_id = $1 AND message_id IS NOT NULL
             AND user_id IN ({})",
            readers.query(2)
        ))
        .bind(chat)
        .bind(readers.user())
        .execute(&mut *tx)
        .await?;
    }
    let rows = sqlx::query(&format!(
        "WITH bumped AS (
             INSERT INTO sync_counters (user_id, last)
             SELECT user_id, 1 FROM ({readers}) readers
             ON CONFLICT (user_id) DO UPDATE SET last = sync_counters.last + 1
             RETURNING user_id, last
         ), logged AS (
             INSERT INTO updates (user_id, update_id, chat_id, op)
             SELECT user_id, last, $1, $2::update_op FROM bumped
             ON CONFLICT (user_id, chat_id) WHERE message_id IS NULL
             DO UPDATE SET update_id = excluded.update_id, op = excluded.op, updated_at = now()
         )
         SELECT user_id, last FROM bumped",
        readers = readers.query(3)
    ))
    .bind(chat)
    .bind(op.as_str())
    .bind(readers.user())
    .fetch_all(&mut *tx)
    .await?;
    bumped(&rows)
}

fn bumped(rows: &[PgRow]) -> Result<Vec<Bumped>> {
    rows.iter()
        .map(|row| {
            Ok(Bumped {
                user_id: row.try_get(0)?,
                last: row.try_get(1)?,
            })
        })
        .collect()
}

/// The newest update id of each user with an open stream, to wake them.
#[derive(Default)]
pub struct UpdateHub {
    users: Mutex<HashMap<String, watch::Sender<i64>>>,
}

impl UpdateHub {
    fn subscribe(&self, user: &str, last: i64) -> watch::Receiver<i64> {
        let mut users = self.users.lock().unwrap_or_else(PoisonError::into_inner);
        let sender = users
            .entry(user.to_owned())
            .or_insert_with(|| watch::channel(last).0);
        sender.send_if_modified(|current| {
            let newer = last > *current;
            if newer {
                *current = last;
            }
            newer
        });
        sender.subscribe()
    }

    fn announce(&self, bumped: &[Bumped]) {
        let mut users = self.users.lock().unwrap_or_else(PoisonError::into_inner);
        for Bumped { user_id, last } in bumped {
            if let Some(sender) = users.get(user_id) {
                sender.send_if_modified(|current| {
                    let newer = *last > *current;
                    if newer {
                        *current = *last;
                    }
                    newer
                });
            }
        }
        users.retain(|_, sender| sender.receiver_count() > 0);
    }
}

impl Store {
    /// Wakes the update streams of the users a committed write logged for.
    pub(super) fn announce(&self, bumped: &[Bumped]) {
        self.updates.announce(bumped);
    }

    /// Wakes the update streams here for a write another instance committed.
    pub(crate) fn updated_elsewhere(&self, bumped: Bumped) {
        self.updates.announce(&[bumped]);
    }

    /// `user`'s newest update id, now and as it moves.
    pub(crate) async fn watch_updates(&self, user: &str) -> Result<watch::Receiver<i64>> {
        let last = self.last_update(user).await?;
        Ok(self.updates.subscribe(user, last))
    }

    pub(super) async fn last_update(&self, user: &str) -> Result<i64> {
        let last: Option<i64> =
            sqlx::query_scalar("SELECT last FROM sync_counters WHERE user_id = $1")
                .bind(user)
                .fetch_optional(&self.pool)
                .await?;
        Ok(last.unwrap_or(0))
    }

    /// Drops tombstones older than `TOMBSTONE_KEEP`; returns how many users' logs lost some.
    pub async fn prune_updates(&self) -> Result<u64> {
        let pruned = sqlx::query(
            "WITH gone AS (
                 DELETE FROM updates
                 WHERE op IN ('deleted', 'chat_deleted')
                   AND updated_at < now() - make_interval(secs => $1)
                 RETURNING user_id, update_id
             ), per_user AS (
                 SELECT user_id, max(update_id) + 1 AS below FROM gone GROUP BY user_id
             )
             UPDATE sync_counters c SET pruned_below = GREATEST(c.pruned_below, p.below)
             FROM per_user p WHERE c.user_id = p.user_id",
        )
        .bind(TOMBSTONE_KEEP.as_secs_f64())
        .execute(&self.pool)
        .await?
        .rows_affected();
        Ok(pruned)
    }
}
