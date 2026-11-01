//! One-time setup tokens that link a machine to an account (see
//! `beep_proto::setup`). The store keeps each token's hash, never its secret.

use super::{Account, Store};
use crate::{protocol::DeviceKind, user::UserKey};
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
    /// The machine's key already signs into another account, or was
    /// removed from one.
    Taken,
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
    /// Keeps a new token's `hash` for the user, working for `ttl`. Returns
    /// when it expires.
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
        .bind(user.as_str())
        .bind(name)
        .bind(expires_at)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(expires_at)
    }

    /// Stops the user's unused token `hash` from working. False when the
    /// user has no such token or it was used.
    pub(crate) async fn revoke_node_token(&self, user: &UserKey, hash: &str) -> Result<bool> {
        let deleted =
            sqlx::query("DELETE FROM node_tokens WHERE hash=$1 AND user_id=$2 AND used_at IS NULL")
                .bind(hash)
                .bind(user.as_str())
                .execute(&self.pool)
                .await?
                .rows_affected();
        Ok(deleted > 0)
    }

    /// Links the claiming machine's key to the account that made the token,
    /// as an accepted link offer does, and spends the token in the same
    /// transaction. A key already linked to that same account claims it
    /// too, as when a machine is set up again.
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
        .bind(DeviceKind::Desktop {}.as_str())
        .execute(&mut *tx)
        .await?
        .rows_affected()
            > 0;
        if !linked {
            let already_ours: bool = sqlx::query_scalar(
                "SELECT EXISTS (SELECT 1 FROM user_keys
                 WHERE endpoint_id=$1 AND user_id=$2 AND revoked_at IS NULL)",
            )
            .bind(&endpoint)
            .bind(&user)
            .fetch_one(&mut *tx)
            .await?;
            if !already_ours {
                tx.rollback().await?;
                return Ok(Claim::Taken);
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
mod tests {
    use super::*;
    use crate::store::{LinkKey, NewKey, SignIn, TestDb};
    use iroh::SecretKey;

    const TTL: Duration = Duration::from_secs(600);

    fn claim<'a>(hash: &'a str, endpoint: EndpointId) -> ClaimNode<'a> {
        ClaimNode {
            hash,
            endpoint,
            named: None,
            fallback: "host",
        }
    }

    #[tokio::test]
    async fn a_token_links_a_machine_once() {
        let db = TestDb::new().await.expect("test database");
        let store = db.store().await.expect("test store");
        let alice = UserKey::for_test("alice");
        let machine = SecretKey::generate().public();
        store
            .create_node_token(&alice, "hash-1", Some("build box"), TTL)
            .await
            .unwrap();

        let Claim::Linked(claimed) = store
            .claim_node_token(claim("hash-1", machine))
            .await
            .unwrap()
        else {
            panic!("a fresh token links");
        };
        assert_eq!(claimed.account, Account::new("alice"));
        assert_eq!(claimed.account_name, "alice");
        assert_eq!(claimed.name, "build box");
        let SignIn::Account {
            account, device, ..
        } = store.sign_in(&machine).await.unwrap()
        else {
            panic!("the machine signs in");
        };
        assert_eq!(account.id, "alice");
        assert_eq!(device, DeviceKind::Desktop {});

        // Spent, for this machine and any other.
        let other = SecretKey::generate().public();
        for endpoint in [machine, other] {
            assert_eq!(
                store
                    .claim_node_token(claim("hash-1", endpoint))
                    .await
                    .unwrap(),
                Claim::Used
            );
        }
        assert_eq!(
            store.claim_node_token(claim("nope", other)).await.unwrap(),
            Claim::Unknown
        );
    }

    #[tokio::test]
    async fn an_expired_token_links_nothing() {
        let db = TestDb::new().await.expect("test database");
        let store = db.store().await.expect("test store");
        let alice = UserKey::for_test("alice");
        store
            .create_node_token(&alice, "old", None, TTL)
            .await
            .unwrap();
        sqlx::query("UPDATE node_tokens SET expires_at = now() - interval '1 second'")
            .execute(&store.pool)
            .await
            .unwrap();
        let machine = SecretKey::generate().public();
        assert_eq!(
            store.claim_node_token(claim("old", machine)).await.unwrap(),
            Claim::Expired
        );
        assert_eq!(store.linked_sign_in(&machine).await.unwrap(), None);
    }

    #[tokio::test]
    async fn a_name_given_on_the_machine_wins_and_a_taken_key_spends_nothing() {
        let db = TestDb::new().await.expect("test database");
        let store = db.store().await.expect("test store");
        let (alice, bob) = (UserKey::for_test("alice"), UserKey::for_test("bob"));
        let machine = SecretKey::generate().public();
        let linked = store
            .link_key(
                &bob,
                NewKey {
                    endpoint: machine,
                    name: "bob's",
                    kind: DeviceKind::Desktop {},
                },
            )
            .await
            .unwrap();
        assert!(matches!(linked, LinkKey::Linked(_)));
        store
            .create_node_token(&alice, "t", None, TTL)
            .await
            .unwrap();
        assert_eq!(
            store.claim_node_token(claim("t", machine)).await.unwrap(),
            Claim::Taken
        );
        // Still good for the right machine.
        let fresh = SecretKey::generate().public();
        let named = ClaimNode {
            named: Some(" laptop "),
            ..claim("t", fresh)
        };
        let Claim::Linked(claimed) = store.claim_node_token(named).await.unwrap() else {
            panic!("the token was not spent");
        };
        assert_eq!(claimed.name, "laptop");
    }

    #[tokio::test]
    async fn only_the_maker_revokes_an_unused_token() {
        let db = TestDb::new().await.expect("test database");
        let store = db.store().await.expect("test store");
        let (alice, bob) = (UserKey::for_test("alice"), UserKey::for_test("bob"));
        store
            .create_node_token(&alice, "t", None, TTL)
            .await
            .unwrap();
        assert!(!store.revoke_node_token(&bob, "t").await.unwrap());
        assert!(store.revoke_node_token(&alice, "t").await.unwrap());
        let machine = SecretKey::generate().public();
        assert_eq!(
            store.claim_node_token(claim("t", machine)).await.unwrap(),
            Claim::Unknown
        );
    }
}
