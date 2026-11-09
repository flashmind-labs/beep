//! One-time setup tokens linking a machine to an account; only their hashes are stored.

use super::{Account, Store};
use crate::{account::user::UserKey, protocol::DeviceKind};
use anyhow::Result;
use chrono::{DateTime, Utc};
use iroh::EndpointId;
use sqlx::Row;
use std::time::Duration;

/// A machine claiming a token with its key.
pub struct ClaimNode<'a> {
    /// `SetupToken::hash` of the token it gave.
    pub hash: &'a str,
    pub endpoint: EndpointId,
    /// The name given on the machine, which wins over the token's.
    pub named: Option<&'a str>,
    /// The name when neither the machine nor the token gives one.
    pub fallback: &'a str,
}

/// How claiming a token went.
#[derive(Debug, PartialEq, Eq)]
pub enum Claim {
    /// The machine's key now signs into `account`.
    Linked(ClaimedNode),
    /// No such token, or it was revoked.
    Unknown,
    Expired,
    Used,
    /// The machine's key belongs to another account; only a new key can link it here.
    OtherAccount,
}

#[derive(Debug, PartialEq, Eq)]
pub struct ClaimedNode {
    pub account: Account,
    /// The account's readable name, such as mega-fox-trot.
    pub account_name: String,
    /// What the machine is called.
    pub name: String,
}

impl Store {
    /// Keeps a new token `hash` valid for `ttl`; returns its expiry.
    pub(crate) async fn create_node_token(
        &self,
        user: &UserKey,
        hash: &str,
        name: Option<&str>,
        ttl: Duration,
    ) -> Result<DateTime<Utc>> {
        let expires_at = Utc::now() + ttl;
        let mut tx = self.pool.begin().await?;
        // Tokens nobody can use any more are only worth keeping a day.
        sqlx::query("DELETE FROM node_tokens WHERE expires_at < now() - interval '1 day'")
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            "INSERT INTO node_tokens (hash, user_id, name, expires_at) VALUES ($1, $2, $3, $4)",
        )
        .bind(hash)
        .bind(user.as_ref())
        .bind(name)
        .bind(expires_at)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(expires_at)
    }

    /// Revokes the user's unused token `hash`; false when missing or used.
    pub(crate) async fn revoke_node_token(&self, user: &UserKey, hash: &str) -> Result<bool> {
        let deleted =
            sqlx::query("DELETE FROM node_tokens WHERE hash=$1 AND user_id=$2 AND used_at IS NULL")
                .bind(hash)
                .bind(user.as_ref())
                .execute(&self.pool)
                .await?
                .rows_affected();
        Ok(deleted > 0)
    }

    /// The key that claimed token `hash`: `None` if no such token, `Some(None)` while unused.
    pub(crate) async fn node_token_claimer(
        &self,
        user: &UserKey,
        hash: &str,
    ) -> Result<Option<Option<String>>> {
        Ok(
            sqlx::query_scalar("SELECT endpoint_id FROM node_tokens WHERE hash=$1 AND user_id=$2")
                .bind(hash)
                .bind(user.as_ref())
                .fetch_optional(&self.pool)
                .await?,
        )
    }

    /// Links the claiming key to the token's account and spends the token in one transaction.
    /// A key of the same account may reclaim (machine set up again): its row is restored and renamed.
    pub async fn claim_node_token(&self, claim: ClaimNode<'_>) -> Result<Claim> {
        let mut tx = self.pool.begin().await?;
        let row = sqlx::query(
            "SELECT t.user_id, u.external_id, u.name AS account_name, t.name,
                    t.used_at IS NOT NULL AS used, t.expires_at <= now() AS expired
             FROM node_tokens t JOIN users u ON u.id = t.user_id
             WHERE t.hash=$1 FOR UPDATE OF t",
        )
        .bind(claim.hash)
        .fetch_optional(&mut *tx)
        .await?;
        // A dropped transaction only rolls back when its connection is next
        // used, keeping the row locked until then, so each refusal ends it.
        let Some(row) = row else {
            tx.rollback().await?;
            return Ok(Claim::Unknown);
        };
        let refused = if row.try_get::<bool, _>("used")? {
            Some(Claim::Used)
        } else if row.try_get::<bool, _>("expired")? {
            Some(Claim::Expired)
        } else {
            None
        };

        if let Some(refused) = refused {
            tx.rollback().await?;
            return Ok(refused);
        }

        let user: String = row.try_get("user_id")?;
        let token_name: Option<String> = row.try_get("name")?;
        let name = claim
            .named
            .or(token_name.as_deref())
            .unwrap_or(claim.fallback)
            .trim()
            .to_owned();
        let endpoint = claim.endpoint.to_string();
        let linked = sqlx::query(
            "INSERT INTO user_keys (endpoint_id, user_id, name, kind) VALUES ($1, $2, $3, $4)
             ON CONFLICT (endpoint_id) DO NOTHING",
        )
        .bind(&endpoint)
        .bind(&user)
        .bind(&name)
        .bind(DeviceKind::Desktop {}.as_ref())
        .execute(&mut *tx)
        .await?
        .rows_affected()
            > 0;
        if !linked {
            let ours = sqlx::query(
                "UPDATE user_keys SET name=$3, revoked_at=NULL
                 WHERE endpoint_id=$1 AND user_id=$2 AND NOT is_primary",
            )
            .bind(&endpoint)
            .bind(&user)
            .bind(&name)
            .execute(&mut *tx)
            .await?
            .rows_affected()
                > 0;
            if !ours {
                tx.rollback().await?;
                return Ok(Claim::OtherAccount);
            }
        }

        sqlx::query("UPDATE node_tokens SET used_at=now(), endpoint_id=$2 WHERE hash=$1")
            .bind(claim.hash)
            .bind(&endpoint)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(Claim::Linked(ClaimedNode {
            account: Account {
                id: user,
                external_id: row.try_get("external_id")?,
            },
            account_name: row.try_get("account_name")?,
            name,
        }))
    }
}

#[cfg(test)]
#[path = "../tests/store_node_tokens.rs"]
mod tests;
