//! The webhooks Beep made on the user's repositories for live project
//! updates, kept until the node that made one says it is gone.

use super::Store;
use crate::user::UserKey;
use anyhow::{Context, Result};
use uuid::Uuid;

/// A watch about to ask a node for a webhook.
pub(crate) struct NewWebhook<'a> {
    pub(crate) id: Uuid,
    /// `IssueHost::stored`.
    pub(crate) host: &'a str,
    pub(crate) repo: &'a str,
    pub(crate) endpoint: &'a str,
    /// The webhook's secret, sealed.
    pub(crate) secret: &'a [u8],
}

fn hook_id(hook: u64) -> Result<i64> {
    i64::try_from(hook).context("a webhook id out of range")
}

impl Store {
    /// Notes a new watch of the repository, and gives the ids of the
    /// webhooks Beep made on it before that are still there: the node
    /// removes those, and only those. Rows of watches that never got a
    /// webhook go.
    pub(crate) async fn begin_webhook(
        &self,
        user: &UserKey,
        new: &NewWebhook<'_>,
    ) -> Result<Vec<u64>> {
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "DELETE FROM project_webhooks
             WHERE user_id=$1 AND host=$2 AND repo=$3 AND hook_id IS NULL",
        )
        .bind(user.as_str())
        .bind(new.host)
        .bind(new.repo)
        .execute(&mut *tx)
        .await?;
        let made: Vec<i64> = sqlx::query_scalar(
            "SELECT hook_id FROM project_webhooks
             WHERE user_id=$1 AND host=$2 AND repo=$3 AND hook_id IS NOT NULL
             ORDER BY created_at",
        )
        .bind(user.as_str())
        .bind(new.host)
        .bind(new.repo)
        .fetch_all(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO project_webhooks (id, user_id, host, repo, endpoint_id, secret)
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(new.id)
        .bind(user.as_str())
        .bind(new.host)
        .bind(new.repo)
        .bind(new.endpoint)
        .bind(new.secret)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(made
            .into_iter()
            .filter_map(|id| u64::try_from(id).ok())
            .collect())
    }

    /// The watch `id` made the webhook `hook`.
    pub(crate) async fn webhook_made(&self, user: &UserKey, id: Uuid, hook: u64) -> Result<()> {
        sqlx::query("UPDATE project_webhooks SET hook_id=$3 WHERE user_id=$1 AND id=$2")
            .bind(user.as_str())
            .bind(id)
            .bind(hook_id(hook)?)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// The webhook `hook` on the repository is gone.
    pub(crate) async fn webhook_removed(
        &self,
        user: &UserKey,
        host: &str,
        repo: &str,
        hook: u64,
    ) -> Result<()> {
        sqlx::query(
            "DELETE FROM project_webhooks
             WHERE user_id=$1 AND host=$2 AND repo=$3 AND hook_id=$4",
        )
        .bind(user.as_str())
        .bind(host)
        .bind(repo)
        .bind(hook_id(hook)?)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::test_db::TestDb;

    fn new<'a>(id: Uuid, repo: &'a str) -> NewWebhook<'a> {
        NewWebhook {
            id,
            host: "github",
            repo,
            endpoint: "node-1",
            secret: b"sealed",
        }
    }

    #[tokio::test]
    async fn only_the_webhooks_beep_made_and_kept_are_stale() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let alice = UserKey::for_test("alice");
        let bob = UserKey::for_test("bob");

        let first = Uuid::new_v4();
        assert!(
            store
                .begin_webhook(&alice, &new(first, "acme/widget"))
                .await
                .unwrap()
                .is_empty()
        );
        store.webhook_made(&alice, first, 41).await.unwrap();
        // A watch that never got its webhook leaves nothing to remove.
        let unmade = Uuid::new_v4();
        store
            .begin_webhook(&alice, &new(unmade, "acme/widget"))
            .await
            .unwrap();
        // Another repository's, and another user's, are not this one's.
        let other = Uuid::new_v4();
        store
            .begin_webhook(&alice, &new(other, "acme/gadget"))
            .await
            .unwrap();
        store.webhook_made(&alice, other, 7).await.unwrap();
        let bobs = Uuid::new_v4();
        store
            .begin_webhook(&bob, &new(bobs, "acme/widget"))
            .await
            .unwrap();
        store.webhook_made(&bob, bobs, 99).await.unwrap();

        let stale = store
            .begin_webhook(&alice, &new(Uuid::new_v4(), "acme/widget"))
            .await
            .unwrap();
        assert_eq!(stale, [41]);

        store
            .webhook_removed(&alice, "github", "acme/widget", 41)
            .await
            .unwrap();
        let stale = store
            .begin_webhook(&alice, &new(Uuid::new_v4(), "acme/widget"))
            .await
            .unwrap();
        assert!(stale.is_empty(), "{stale:?}");
    }
}
