//! One-time login codes, and the website logins accounts are found by.

use super::{NewKey, Store, user_keys::insert_account};
use anyhow::{Result, bail};
use chrono::{DateTime, Utc};
use iroh::EndpointId;
use sqlx::PgConnection;
use uuid::Uuid;

/// Live codes one key may hold; a new one pushes out its oldest.
const MAX_CODES_PER_KEY: i64 = 5;

/// Who the website signed in.
pub struct Login<'a> {
    /// Stable for the login: `google:<sub>`, `apple:<sub>` or `email:<address>`.
    pub sub: &'a str,
    pub email: Option<&'a str>,
    pub name: Option<&'a str>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum LoginEnd {
    /// The key signs into `user`, with the login on that account.
    SignedIn { user: String },
    /// The code is unknown, used, expired or another key's.
    BadCode,
    /// The key has an account and another account holds the login.
    Taken,
}

impl Store {
    /// Saves a code for `endpoint` by its hash, dropping expired codes and the key's oldest extras.
    pub(crate) async fn add_login_code(
        &self,
        hash: &[u8],
        endpoint: &EndpointId,
        expires_at: DateTime<Utc>,
    ) -> Result<()> {
        let endpoint = endpoint.to_string();
        let mut tx = self.pool.begin().await?;
        sqlx::query("DELETE FROM login_codes WHERE expires_at < now()")
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            "INSERT INTO login_codes (code_hash, endpoint_id, expires_at) VALUES ($1, $2, $3)",
        )
        .bind(hash)
        .bind(&endpoint)
        .bind(expires_at)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "DELETE FROM login_codes WHERE endpoint_id=$1 AND code_hash NOT IN (
                 SELECT code_hash FROM login_codes WHERE endpoint_id=$1
                 ORDER BY expires_at DESC LIMIT $2)",
        )
        .bind(&endpoint)
        .bind(MAX_CODES_PER_KEY)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    /// In one go: uses up the code, finds or makes the login's account, and adds the key to it.
    /// A key that already has an account attaches the login to it instead.
    pub(crate) async fn complete_login(
        &self,
        hash: &[u8],
        login: &Login<'_>,
        key: NewKey<'_>,
    ) -> Result<LoginEnd> {
        // A second try covers a first login racing another one.
        for _ in 0..2 {
            let mut tx = self.pool.begin().await?;

            match complete_in(&mut tx, hash, login, &key).await? {
                Some(LoginEnd::SignedIn { user }) => {
                    tx.commit().await?;
                    return Ok(LoginEnd::SignedIn { user });
                }
                Some(end) => {
                    tx.rollback().await?;
                    return Ok(end);
                }
                None => tx.rollback().await?,
            }
        }

        bail!("a login kept racing another connection")
    }
}

/// `None` when another connection got in first and it is worth trying again.
async fn complete_in(
    tx: &mut PgConnection,
    hash: &[u8],
    login: &Login<'_>,
    key: &NewKey<'_>,
) -> Result<Option<LoginEnd>> {
    let endpoint = key.endpoint.to_string();
    let used = sqlx::query(
        "UPDATE login_codes SET used_at=now()
         WHERE code_hash=$1 AND endpoint_id=$2 AND used_at IS NULL AND expires_at > now()",
    )
    .bind(hash)
    .bind(&endpoint)
    .execute(&mut *tx)
    .await?
    .rows_affected();

    if used == 0 {
        return Ok(Some(LoginEnd::BadCode));
    }

    let own: Option<(String, bool)> = sqlx::query_as(
        "SELECT user_id, revoked_at IS NOT NULL FROM user_keys WHERE endpoint_id=$1",
    )
    .bind(&endpoint)
    .fetch_optional(&mut *tx)
    .await?;
    let holder: Option<String> = sqlx::query_scalar("SELECT user_id FROM user_logins WHERE sub=$1")
        .bind(login.sub)
        .fetch_optional(&mut *tx)
        .await?;
    let user = match (own, holder) {
        (Some((_, true)), _) => return Ok(Some(LoginEnd::BadCode)),
        (Some((own, false)), Some(holder)) if own != holder => {
            return Ok(Some(LoginEnd::Taken));
        }
        (Some((own, false)), _) => own,
        (None, Some(holder)) => {
            let added = sqlx::query(
                "INSERT INTO user_keys (endpoint_id, user_id, name, kind) VALUES ($1, $2, $3, $4)
                 ON CONFLICT (endpoint_id) DO NOTHING",
            )
            .bind(&endpoint)
            .bind(&holder)
            .bind(key.name)
            .bind(key.kind.as_ref())
            .execute(&mut *tx)
            .await?
            .rows_affected();

            if added == 0 {
                return Ok(None);
            }

            holder
        }
        (None, None) => {
            let user = Uuid::new_v4().simple().to_string();

            if !insert_account(&mut *tx, &user, &key.endpoint).await? {
                return Ok(None);
            }

            user
        }
    };
    let saved = sqlx::query(
        "INSERT INTO user_logins (sub, user_id, email, name) VALUES ($1, $2, $3, $4)
         ON CONFLICT (sub) DO UPDATE SET email=excluded.email, name=excluded.name
         WHERE user_logins.user_id=excluded.user_id",
    )
    .bind(login.sub)
    .bind(&user)
    .bind(login.email)
    .bind(login.name)
    .execute(&mut *tx)
    .await?
    .rows_affected();

    if saved == 0 {
        return Ok(None);
    }

    Ok(Some(LoginEnd::SignedIn { user }))
}

#[cfg(test)]
#[path = "../tests/store_logins.rs"]
mod tests;
