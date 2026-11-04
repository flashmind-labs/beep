//! The machines running `beep-node`, and what each last reported.

use super::Store;
use crate::{account::user::UserKey, protocol::node::EnvReport};
use anyhow::Result;
use chrono::{DateTime, Utc};
use iroh::EndpointId;
use sqlx::Row;

/// A node as the store keeps it.
#[derive(Debug, Clone)]
pub struct NodeRow {
    pub endpoint: String,
    pub name: String,
    pub version: String,
    pub env: Option<EnvReport>,
    pub seen_at: DateTime<Utc>,
}

/// A node that just said hello.
pub struct NewNode<'a> {
    pub endpoint: EndpointId,
    pub name: &'a str,
    pub version: &'a str,
}

impl Store {
    pub(crate) async fn node_connected(&self, user: &UserKey, node: NewNode<'_>) -> Result<()> {
        sqlx::query(
            "INSERT INTO nodes (endpoint_id, user_id, name, version) VALUES ($1, $2, $3, $4)
             ON CONFLICT (endpoint_id) DO UPDATE SET user_id=excluded.user_id, name=excluded.name,
                 version=excluded.version, connected_at=now(), seen_at=now()",
        )
        .bind(node.endpoint.to_string())
        .bind(user.as_str())
        .bind(node.name)
        .bind(node.version)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub(crate) async fn save_node_env(
        &self,
        user: &UserKey,
        endpoint: &EndpointId,
        env: &EnvReport,
    ) -> Result<()> {
        sqlx::query("UPDATE nodes SET env=$3, seen_at=now() WHERE endpoint_id=$1 AND user_id=$2")
            .bind(endpoint.to_string())
            .bind(user.as_str())
            .bind(serde_json::to_value(env)?)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub(crate) async fn nodes(&self, user: &UserKey) -> Result<Vec<NodeRow>> {
        let rows = sqlx::query(
            "SELECT endpoint_id, name, version, env, seen_at FROM nodes WHERE user_id=$1 ORDER BY name",
        )
        .bind(user.as_str())
        .fetch_all(&self.pool)
        .await?;
        rows.iter()
            .map(|row| {
                let env: Option<serde_json::Value> = row.try_get(3)?;
                Ok(NodeRow {
                    endpoint: row.try_get(0)?,
                    name: row.try_get(1)?,
                    version: row.try_get(2)?,
                    env: env.map(serde_json::from_value).transpose()?,
                    seen_at: row.try_get(4)?,
                })
            })
            .collect()
    }
}
