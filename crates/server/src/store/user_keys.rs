//! The keys that sign into an account: the phone that made it, and the
//! computers it linked.

use super::Store;
use crate::{
    protocol::{DeviceKind, names::username},
    user::UserKey,
};
use anyhow::{Result, bail};
use chrono::{DateTime, Utc};
use iroh::EndpointId;
use uuid::Uuid;

/// An account, by its own id and the id outside services know it by.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    pub id: String,
    /// The Composio entity and sandbox folder. The old key id for accounts
    /// made before ids were their own, else the same as `id`.
    pub external_id: String,
}

impl Account {
    /// An account whose outside id is its own, for tests.
    pub fn new(id: &str) -> Self {
        Self {
            id: id.to_owned(),
            external_id: id.to_owned(),
        }
    }
}

/// Who a key signs in as.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignIn {
    Account {
        account: Account,
        device: DeviceKind,
        /// The key that made the account.
        primary: bool,
    },
    /// The key was linked and then removed.
    Revoked,
}

/// A key that signs into an account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountKey {
    pub endpoint_id: String,
    pub name: String,
    pub kind: DeviceKind,
    /// The key that made the account.
    pub primary: bool,
    pub linked_at: DateTime<Utc>,
}

/// A key that signs into an account now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveKey {
    pub endpoint_id: String,
    pub kind: DeviceKind,
}

/// A key to link to an account.
pub struct NewKey<'a> {
    pub endpoint: EndpointId,
    pub name: &'a str,
    pub kind: DeviceKind,
}

#[derive(Debug, PartialEq, Eq)]
pub enum LinkKey {
    Linked(AccountKey),
    /// The key already signs into an account, or was linked before.
    Taken,
}

/// `endpoint_id, name, kind, is_primary, created_at` of a key.
type KeyRow = (String, String, String, bool, DateTime<Utc>);

fn account_key((endpoint_id, name, kind, primary, created_at): KeyRow) -> AccountKey {
    AccountKey {
        endpoint_id,
        name,
        kind: DeviceKind::parse(&kind).unwrap_or_default(),
        primary,
        linked_at: created_at,
    }
}

/// `user_id, external_id, kind, is_primary, revoked` of a key's account.
type SignInRow = (String, String, String, bool, bool);

impl Store {
    /// The account `endpoint` signs in as. A key nobody linked makes its own
    /// account on its first connection, as that account's primary key.
    pub async fn sign_in(&self, endpoint: &EndpointId) -> Result<SignIn> {
        let id = endpoint.to_string();
        // A second try covers a first connection racing another one.
        for _ in 0..2 {
            if let Some(row) = self.key_account(&id).await? {
                return Ok(signed_in(row));
            }
            let user = Uuid::new_v4().simple().to_string();
            let mut tx = self.pool.begin().await?;
            if insert_account(&mut tx, &user, endpoint).await? {
                tx.commit().await?;
                return Ok(new_account(&user));
            }
            tx.rollback().await?;
        }
        bail!("signing in {id} kept racing another connection")
    }

    /// The account `endpoint` signs in as, or `None` for a key nobody
    /// linked. Unlike `sign_in` it never makes an account.
    pub async fn linked_sign_in(&self, endpoint: &EndpointId) -> Result<Option<SignIn>> {
        Ok(self
            .key_account(&endpoint.to_string())
            .await?
            .map(signed_in))
    }

    async fn key_account(&self, endpoint_id: &str) -> Result<Option<SignInRow>> {
        Ok(sqlx::query_as(
            "SELECT u.id, u.external_id, k.kind, k.is_primary, k.revoked_at IS NOT NULL
             FROM user_keys k JOIN users u ON u.id = k.user_id WHERE k.endpoint_id=$1",
        )
        .bind(endpoint_id)
        .fetch_optional(&self.pool)
        .await?)
    }

    /// Every account, for starting their crons and feeds.
    pub async fn accounts(&self) -> Result<Vec<Account>> {
        let rows: Vec<(String, String)> = sqlx::query_as("SELECT id, external_id FROM users")
            .fetch_all(&self.pool)
            .await?;
        Ok(rows
            .into_iter()
            .map(|(id, external_id)| Account { id, external_id })
            .collect())
    }

    /// Links `key` to the user's account, unless the key is already in use.
    pub(crate) async fn link_key(&self, user: &UserKey, key: NewKey<'_>) -> Result<LinkKey> {
        let row: Option<KeyRow> = sqlx::query_as(
            "INSERT INTO user_keys (endpoint_id, user_id, name, kind) VALUES ($1, $2, $3, $4)
             ON CONFLICT (endpoint_id) DO NOTHING
             RETURNING endpoint_id, name, kind, is_primary, created_at",
        )
        .bind(key.endpoint.to_string())
        .bind(user.as_str())
        .bind(key.name)
        .bind(key.kind.as_str())
        .fetch_optional(&self.pool)
        .await?;
        Ok(match row {
            Some(row) => LinkKey::Linked(account_key(row)),
            None => LinkKey::Taken,
        })
    }

    /// The account's primary key first, then the keys it linked.
    pub(crate) async fn account_keys(&self, user: &UserKey) -> Result<Vec<AccountKey>> {
        let rows: Vec<KeyRow> = sqlx::query_as(
            "SELECT endpoint_id, name, kind, is_primary, created_at FROM user_keys
             WHERE user_id=$1 AND revoked_at IS NULL ORDER BY is_primary DESC, created_at",
        )
        .bind(user.as_str())
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(account_key).collect())
    }

    /// Stops a linked key signing in and drops its push tokens. False if
    /// the account has no such key. The primary key cannot be revoked.
    pub(crate) async fn revoke_key(&self, user: &UserKey, endpoint_id: &str) -> Result<bool> {
        let mut tx = self.pool.begin().await?;
        let revoked = sqlx::query(
            "UPDATE user_keys SET revoked_at=now()
             WHERE user_id=$1 AND endpoint_id=$2 AND NOT is_primary AND revoked_at IS NULL",
        )
        .bind(user.as_str())
        .bind(endpoint_id)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if revoked == 0 {
            tx.rollback().await?;
            return Ok(false);
        }
        sqlx::query("DELETE FROM devices WHERE user_id=$1 AND endpoint_id=$2")
            .bind(user.as_str())
            .bind(endpoint_id)
            .execute(&mut *tx)
            .await?;
        // A removed machine is no longer one the agent can run work on.
        sqlx::query("DELETE FROM nodes WHERE user_id=$1 AND endpoint_id=$2")
            .bind(user.as_str())
            .bind(endpoint_id)
            .execute(&mut *tx)
            .await?;
        // Nor one that syncs projects; its peers drop it on the next push.
        sqlx::query("DELETE FROM project_sync WHERE user_id=$1 AND endpoint_id=$2")
            .bind(user.as_str())
            .bind(endpoint_id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(true)
    }

    /// Records what kind of device `endpoint` is.
    pub async fn set_key_kind(&self, endpoint: &EndpointId, kind: DeviceKind) -> Result<()> {
        sqlx::query("UPDATE user_keys SET kind=$2 WHERE endpoint_id=$1")
            .bind(endpoint.to_string())
            .bind(kind.as_str())
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Every key that signs into account `user` and the device it is, for
    /// pushing notices.
    pub async fn live_keys(&self, user: &str) -> Result<Vec<LiveKey>> {
        let rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT endpoint_id, kind FROM user_keys WHERE user_id=$1 AND revoked_at IS NULL
             ORDER BY is_primary DESC, created_at",
        )
        .bind(user)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(endpoint_id, kind)| LiveKey {
                endpoint_id,
                kind: DeviceKind::parse(&kind).unwrap_or_default(),
            })
            .collect())
    }
}

/// Makes account `user` with `endpoint` as its primary key, unless the key
/// already signs into one: false then, and the transaction must roll back.
pub(super) async fn insert_account(
    tx: &mut sqlx::PgConnection,
    user: &str,
    endpoint: &EndpointId,
) -> Result<bool> {
    let name = username(endpoint);
    sqlx::query("INSERT INTO users (id, name, external_id) VALUES ($1, $2, $1)")
        .bind(user)
        .bind(&name)
        .execute(&mut *tx)
        .await?;
    let made = sqlx::query(
        "INSERT INTO user_keys (endpoint_id, user_id, name, kind, is_primary)
         VALUES ($1, $2, $3, $4, true) ON CONFLICT (endpoint_id) DO NOTHING",
    )
    .bind(endpoint.to_string())
    .bind(user)
    .bind(&name)
    .bind(DeviceKind::Phone {}.as_str())
    .execute(&mut *tx)
    .await?
    .rows_affected();
    Ok(made > 0)
}

/// How a key that just made account `user` signs in.
pub(super) fn new_account(user: &str) -> SignIn {
    SignIn::Account {
        account: Account::new(user),
        device: DeviceKind::Phone {},
        primary: true,
    }
}

fn signed_in((user, external_id, kind, primary, revoked): SignInRow) -> SignIn {
    if revoked {
        return SignIn::Revoked;
    }
    SignIn::Account {
        account: Account {
            id: user,
            external_id,
        },
        device: DeviceKind::parse(&kind).unwrap_or_default(),
        primary,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::TestDb;
    use iroh::SecretKey;

    fn key(seed: u8) -> EndpointId {
        SecretKey::from_bytes(&[seed; 32]).public()
    }

    #[tokio::test]
    async fn a_linked_key_signs_into_the_account_until_revoked() {
        let db = TestDb::new().await.expect("test database");
        let store = db.store().await.expect("test store");
        let phone = key(1);
        let desktop = key(2);
        let SignIn::Account {
            account, primary, ..
        } = store.sign_in(&phone).await.unwrap()
        else {
            panic!("a new key makes an account");
        };
        assert!(primary);
        assert_ne!(account.id, phone.to_string(), "an account id is its own");
        assert_eq!(account.external_id, account.id);
        let user = account.id.clone();
        let owner = UserKey::for_test(&user);
        let new = NewKey {
            endpoint: desktop,
            name: "MacBook",
            kind: DeviceKind::Desktop {},
        };
        assert!(matches!(
            store.link_key(&owner, new).await.unwrap(),
            LinkKey::Linked(_)
        ));
        assert_eq!(
            store.sign_in(&desktop).await.unwrap(),
            SignIn::Account {
                account: account.clone(),
                device: DeviceKind::Desktop {},
                primary: false,
            }
        );
        let keys = store.account_keys(&owner).await.unwrap();
        assert_eq!(keys.len(), 2);
        assert!(keys[0].primary);
        assert_eq!(keys[0].endpoint_id, phone.to_string());
        assert_eq!(
            store.live_keys(&user).await.unwrap(),
            [
                LiveKey {
                    endpoint_id: phone.to_string(),
                    kind: DeviceKind::Phone {},
                },
                LiveKey {
                    endpoint_id: desktop.to_string(),
                    kind: DeviceKind::Desktop {},
                },
            ]
        );

        // The phone's own key and a used key cannot be linked again.
        for endpoint in [phone, desktop] {
            let again = NewKey {
                endpoint,
                name: "Again",
                kind: DeviceKind::Desktop {},
            };
            assert_eq!(store.link_key(&owner, again).await.unwrap(), LinkKey::Taken);
        }

        // Nor can the primary key be removed.
        assert!(!store.revoke_key(&owner, &phone.to_string()).await.unwrap());
        // A removed machine stops being one the agent can use.
        store
            .node_connected(
                &owner,
                crate::store::NewNode {
                    endpoint: desktop,
                    name: "desk",
                    version: "0.1.0",
                },
            )
            .await
            .unwrap();
        store
            .set_project_sync(
                &owner,
                &crate::store::SyncRow {
                    project: "game".into(),
                    endpoint: desktop.to_string(),
                    folder: "/srv/game".into(),
                },
            )
            .await
            .unwrap();
        assert!(
            store
                .revoke_key(&owner, &desktop.to_string())
                .await
                .unwrap()
        );
        assert!(store.nodes(&owner).await.unwrap().is_empty());
        // Nor one that syncs projects.
        assert!(store.project_syncs(&owner).await.unwrap().is_empty());
        assert_eq!(store.sign_in(&desktop).await.unwrap(), SignIn::Revoked);
        assert_eq!(
            store.live_keys(&user).await.unwrap(),
            [LiveKey {
                endpoint_id: phone.to_string(),
                kind: DeviceKind::Phone {},
            }]
        );
        assert!(
            !store
                .revoke_key(&owner, &desktop.to_string())
                .await
                .unwrap()
        );
    }
}
