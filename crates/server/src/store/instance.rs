//! The owner of a single-user instance, kept in `server_settings` so the first claim wins.

use super::{
    Store,
    user_keys::{insert_account, new_account},
};
use crate::store::SignIn;
use anyhow::Result;
use iroh::EndpointId;
use serde_json::Value;
use uuid::Uuid;

/// The `server_settings` key that holds the owner's account id.
const OWNER: &str = "owner";

impl Store {
    /// The account that owns the instance, once one claimed it.
    pub async fn instance_owner(&self) -> Result<Option<String>> {
        let value: Option<Value> =
            sqlx::query_scalar("SELECT value FROM server_settings WHERE key = $1")
                .bind(OWNER)
                .fetch_optional(&self.pool)
                .await?;
        Ok(value.and_then(|value| value.as_str().map(str::to_owned)))
    }

    /// Makes `user` the owner, claimed by key `by`; false when already claimed.
    pub async fn claim_instance(&self, user: &str, by: &EndpointId) -> Result<bool> {
        let claimed = sqlx::query(
            "INSERT INTO server_settings (key, value, updated_by) VALUES ($1, $2, $3)
             ON CONFLICT (key) DO NOTHING",
        )
        .bind(OWNER)
        .bind(Value::String(user.to_owned()))
        .bind(by.to_string())
        .execute(&self.pool)
        .await?
        .rows_affected();
        Ok(claimed > 0)
    }

    /// Makes a new account for `endpoint` and the owner, atomically; `None` if owned or the key already has an account.
    pub async fn claim_instance_new(&self, endpoint: &EndpointId) -> Result<Option<SignIn>> {
        let user = Uuid::new_v4().simple().to_string();
        let mut tx = self.pool.begin().await?;
        // Claimed first: a second claim waits on this row and then finds it.
        let claimed = sqlx::query(
            "INSERT INTO server_settings (key, value, updated_by) VALUES ($1, $2, $3)
             ON CONFLICT (key) DO NOTHING",
        )
        .bind(OWNER)
        .bind(Value::String(user.clone()))
        .bind(endpoint.to_string())
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if claimed == 0 || !insert_account(&mut tx, &user, endpoint).await? {
            tx.rollback().await?;
            return Ok(None);
        }
        tx.commit().await?;
        Ok(Some(new_account(&user)))
    }
}

#[cfg(test)]
mod tests {
    use crate::store::{SignIn, TestDb};
    use iroh::SecretKey;

    #[tokio::test]
    async fn the_first_claim_wins() {
        let db = TestDb::new().await.expect("test database");
        let store = db.store().await.expect("test store");
        assert_eq!(store.instance_owner().await.unwrap(), None);
        let first = SecretKey::generate().public();
        let Some(SignIn::Account { account, .. }) = store.claim_instance_new(&first).await.unwrap()
        else {
            panic!("the first key claims the instance");
        };
        assert_eq!(
            store.instance_owner().await.unwrap().as_deref(),
            Some(account.id.as_str())
        );
        let second = SecretKey::generate().public();
        assert_eq!(store.claim_instance_new(&second).await.unwrap(), None);
        assert!(!store.claim_instance("alice", &second).await.unwrap());
        // The refused key got no account.
        assert_eq!(store.linked_sign_in(&second).await.unwrap(), None);
    }
}
