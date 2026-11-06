//! The notices the apps' events streams read, and which devices took them.

use super::Store;
use crate::{
    app::{Reach, UserNotice},
    protocol::{Notice, http},
};
use anyhow::{Context, Result};
use iroh::EndpointId;
use sqlx::Row;
use std::time::Duration;

/// The channel every instance listens on (see the `0102` migration).
pub const EVENTS_CHANNEL: &str = "beep_events";

/// How long a notice stays for a stream to resume from.
const KEEP: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// The most notices one read gives.
const PAGE: i64 = 200;

/// A logged notice and its id.
#[derive(Debug, Clone)]
pub struct LoggedNotice {
    pub id: i64,
    pub notice: UserNotice,
}

impl Store {
    /// Logs `notice` and tells every instance; returns its id.
    pub async fn log_notice(&self, notice: &UserNotice) -> Result<i64> {
        let (reach, device) = match notice.to {
            Reach::Everyone => ("everyone", None),
            Reach::Desktops => ("desktops", None),
            Reach::Device(key) => ("device", Some(key.to_string())),
        };
        let desktop = notice.desktop.as_ref().map(http::encode).transpose()?;
        let id: i64 = sqlx::query_scalar(&format!(
            "WITH logged AS (
                 INSERT INTO app_notices (user_id, notice, desktop, reach, device)
                 VALUES ($1, $2, $3, $4, $5) RETURNING id
             )
             SELECT id FROM logged, LATERAL (
                 SELECT pg_notify('{EVENTS_CHANNEL}', 'n ' || logged.id || ' ' || $1)
             ) told"
        ))
        .bind(&notice.user)
        .bind(http::encode(&notice.notice)?)
        .bind(desktop)
        .bind(reach)
        .bind(device)
        .fetch_one(&self.pool)
        .await?;
        Ok(id)
    }

    /// `user`'s notices after `after`, oldest first, a page at most.
    pub async fn notices_after(&self, user: &str, after: i64) -> Result<Vec<LoggedNotice>> {
        let rows = sqlx::query(
            "SELECT id, notice, desktop, reach, device FROM app_notices
             WHERE user_id = $1 AND id > $2 ORDER BY id LIMIT $3",
        )
        .bind(user)
        .bind(after)
        .bind(PAGE)
        .fetch_all(&self.pool)
        .await?;
        rows.iter()
            .map(|row| {
                let id: i64 = row.try_get(0)?;
                let notice: Vec<u8> = row.try_get(1)?;
                let desktop: Option<Vec<u8>> = row.try_get(2)?;
                let reach: String = row.try_get(3)?;
                let device: Option<String> = row.try_get(4)?;
                let to = match (reach.as_str(), device) {
                    ("desktops", _) => Reach::Desktops,
                    ("device", Some(key)) => Reach::Device(
                        key.parse::<EndpointId>()
                            .context("a logged notice names a bad key")?,
                    ),
                    _ => Reach::Everyone,
                };
                let notice = UserNotice {
                    user: user.to_owned(),
                    notice: http::decode::<Notice>(&notice)?,
                    desktop: desktop
                        .map(|bytes| http::decode::<Notice>(&bytes))
                        .transpose()?,
                    to,
                };
                Ok(LoggedNotice { id, notice })
            })
            .collect()
    }

    /// The newest notice id of `user`, 0 for none.
    pub async fn newest_notice(&self, user: &str) -> Result<i64> {
        let id: Option<i64> =
            sqlx::query_scalar("SELECT max(id) FROM app_notices WHERE user_id = $1")
                .bind(user)
                .fetch_one(&self.pool)
                .await?;
        Ok(id.unwrap_or(0))
    }

    /// Notes that `key` took notice `id`. An id the key's account does not
    /// have is skipped.
    pub async fn ack_notice(&self, user: &str, id: i64, key: &EndpointId) -> Result<()> {
        sqlx::query(
            "INSERT INTO app_notice_acks (notice_id, key)
             SELECT id, $3 FROM app_notices WHERE id = $1 AND user_id = $2
             ON CONFLICT DO NOTHING",
        )
        .bind(id)
        .bind(user)
        .bind(key.to_string())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Which keys took notice `id`.
    pub async fn notice_acks(&self, id: i64) -> Result<Vec<String>> {
        Ok(
            sqlx::query_scalar("SELECT key FROM app_notice_acks WHERE notice_id = $1")
                .bind(id)
                .fetch_all(&self.pool)
                .await?,
        )
    }

    /// Ends the events streams of `key` on every instance, as when it was
    /// removed or its account moved.
    pub async fn end_streams(&self, key: &EndpointId) -> Result<()> {
        sqlx::query(&format!(
            "SELECT pg_notify('{EVENTS_CHANNEL}', 'k 0 ' || $1)"
        ))
        .bind(key.to_string())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Drops notices older than `KEEP`; returns how many went.
    pub async fn prune_notices(&self) -> Result<u64> {
        Ok(sqlx::query(
            "DELETE FROM app_notices WHERE created_at < now() - make_interval(secs => $1)",
        )
        .bind(KEEP.as_secs_f64())
        .execute(&self.pool)
        .await?
        .rows_affected())
    }

}
