//! Keys linked to an account besides the one that made it, such as a
//! computer the phone approved.

use super::Store;
use crate::{
    protocol::{DeviceKind, names::username},
    user::UserKey,
};
use anyhow::Result;
use chrono::{DateTime, Utc};
use iroh::EndpointId;

/// Who a key signs in as.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignIn {
    Account {
        user: String,
        device: DeviceKind,
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

/// A key to link to an account.
pub struct NewKey<'a> {
    pub endpoint: EndpointId,
    pub name: &'a str,
    pub kind: DeviceKind,
}

#[derive(Debug, PartialEq, Eq)]
pub enum LinkKey {
    Linked(AccountKey),
    /// The key already has an account or was linked before.
    Taken,
}

/// `endpoint_id, name, kind, created_at` of a linked key.
type KeyRow = (String, String, String, DateTime<Utc>);

fn linked_key((endpoint_id, name, kind, created_at): KeyRow) -> AccountKey {
    AccountKey {
        endpoint_id,
        name,
        kind: DeviceKind::parse(&kind).unwrap_or_default(),
        primary: false,
        linked_at: created_at,
    }
}

impl Store {
    /// The account `endpoint` signs in as. A key nobody linked makes its own
    /// account on its first connection.
    pub async fn sign_in(&self, endpoint: &EndpointId) -> Result<SignIn> {
        let id = endpoint.to_string();
        let linked: Option<(String, String, bool)> = sqlx::query_as(
            "SELECT user_id, kind, revoked_at IS NOT NULL FROM account_keys WHERE endpoint_id=$1",
        )
        .bind(&id)
        .fetch_optional(&self.pool)
        .await?;
        if let Some((user, kind, revoked)) = linked {
            if revoked {
                return Ok(SignIn::Revoked);
            }
            let device = DeviceKind::parse(&kind).unwrap_or_default();
            return Ok(SignIn::Account { user, device });
        }
        sqlx::query("INSERT INTO users (id, name) VALUES ($1, $2) ON CONFLICT (id) DO NOTHING")
            .bind(&id)
            .bind(username(endpoint))
            .execute(&self.pool)
            .await?;
        Ok(SignIn::Account {
            user: id,
            device: DeviceKind::Phone {},
        })
    }

    /// Links `key` to the user's account, unless the key is already in use.
    pub(crate) async fn link_key(&self, user: &UserKey, key: NewKey<'_>) -> Result<LinkKey> {
        let id = key.endpoint.to_string();
        let mut tx = self.pool.begin().await?;
        let owns_account: bool =
            sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM users WHERE id=$1)")
                .bind(&id)
                .fetch_one(&mut *tx)
                .await?;
        if owns_account {
            return Ok(LinkKey::Taken);
        }
        let row: Option<KeyRow> = sqlx::query_as(
            "INSERT INTO account_keys (endpoint_id, user_id, name, kind) VALUES ($1, $2, $3, $4)
             ON CONFLICT (endpoint_id) DO NOTHING
             RETURNING endpoint_id, name, kind, created_at",
        )
        .bind(&id)
        .bind(user.as_str())
        .bind(key.name)
        .bind(key.kind.as_str())
        .fetch_optional(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(match row {
            Some(row) => LinkKey::Linked(linked_key(row)),
            None => LinkKey::Taken,
        })
    }

    /// The account's own key first, then the keys linked to it.
    pub(crate) async fn account_keys(&self, user: &UserKey) -> Result<Vec<AccountKey>> {
        let primary: Option<(String, DateTime<Utc>)> =
            sqlx::query_as("SELECT name, created_at FROM users WHERE id=$1")
                .bind(user.as_str())
                .fetch_optional(&self.pool)
                .await?;
        let mut keys: Vec<AccountKey> = primary
            .into_iter()
            .map(|(name, created_at)| AccountKey {
                endpoint_id: user.as_str().to_owned(),
                name,
                kind: DeviceKind::Phone {},
                primary: true,
                linked_at: created_at,
            })
            .collect();
        let linked: Vec<KeyRow> = sqlx::query_as(
            "SELECT endpoint_id, name, kind, created_at FROM account_keys
             WHERE user_id=$1 AND revoked_at IS NULL ORDER BY created_at",
        )
        .bind(user.as_str())
        .fetch_all(&self.pool)
        .await?;
        keys.extend(linked.into_iter().map(linked_key));
        Ok(keys)
    }

    /// Stops a linked key signing in. False if the account has no such key.
    pub(crate) async fn revoke_key(&self, user: &UserKey, endpoint_id: &str) -> Result<bool> {
        let revoked = sqlx::query(
            "UPDATE account_keys SET revoked_at=now()
             WHERE user_id=$1 AND endpoint_id=$2 AND revoked_at IS NULL",
        )
        .bind(user.as_str())
        .bind(endpoint_id)
        .execute(&self.pool)
        .await?
        .rows_affected();
        Ok(revoked > 0)
    }

    /// Every key that signs into account `user`, for pushing notices.
    pub async fn live_keys(&self, user: &str) -> Result<Vec<String>> {
        let mut keys = vec![user.to_owned()];
        keys.extend(
            sqlx::query_scalar::<_, String>(
                "SELECT endpoint_id FROM account_keys WHERE user_id=$1 AND revoked_at IS NULL",
            )
            .bind(user)
            .fetch_all(&self.pool)
            .await?,
        );
        Ok(keys)
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
        let SignIn::Account { user, .. } = store.sign_in(&phone).await.unwrap() else {
            panic!("a new key makes an account");
        };
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
                user: user.clone(),
                device: DeviceKind::Desktop {}
            }
        );
        let keys = store.account_keys(&owner).await.unwrap();
        assert_eq!(keys.len(), 2);
        assert!(keys[0].primary);
        assert_eq!(store.live_keys(&user).await.unwrap().len(), 2);

        // The phone's own key and a used key cannot be linked again.
        for endpoint in [phone, desktop] {
            let again = NewKey {
                endpoint,
                name: "Again",
                kind: DeviceKind::Desktop {},
            };
            assert_eq!(store.link_key(&owner, again).await.unwrap(), LinkKey::Taken);
        }

        assert!(
            store
                .revoke_key(&owner, &desktop.to_string())
                .await
                .unwrap()
        );
        assert_eq!(store.sign_in(&desktop).await.unwrap(), SignIn::Revoked);
        assert_eq!(store.live_keys(&user).await.unwrap(), vec![user.clone()]);
        assert!(
            !store
                .revoke_key(&owner, &desktop.to_string())
                .await
                .unwrap()
        );
    }
}
