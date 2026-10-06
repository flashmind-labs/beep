//! Accounts and the model settings each user picks.

use super::Store;
use crate::{protocol::names::username, user::UserKey};
use anyhow::Result;
use iroh::EndpointId;

impl Store {
    /// The user `endpoint` signs in as: its own id, made on its first
    /// connection.
    pub async fn sign_in(&self, endpoint: &EndpointId) -> Result<String> {
        let id = endpoint.to_string();
        sqlx::query("INSERT INTO users (id, name) VALUES ($1, $2) ON CONFLICT (id) DO NOTHING")
            .bind(&id)
            .bind(username(endpoint))
            .execute(&self.pool)
            .await?;
        Ok(id)
    }

    /// The model the user picked, if any.
    pub(crate) async fn user_model(&self, user: &UserKey) -> Result<Option<String>> {
        let model = sqlx::query_scalar("SELECT model FROM users WHERE id=$1")
            .bind(user.as_str())
            .fetch_optional(&self.pool)
            .await?;
        Ok(model.flatten())
    }

    /// Picks the user's model; `None` goes back to the server default.
    pub(crate) async fn set_user_model(&self, user: &UserKey, model: Option<&str>) -> Result<()> {
        sqlx::query("UPDATE users SET model=$2 WHERE id=$1")
            .bind(user.as_str())
            .bind(model)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// The reasoning level the user picked, if any.
    pub(crate) async fn user_thinking(&self, user: &UserKey) -> Result<Option<String>> {
        let thinking = sqlx::query_scalar("SELECT thinking FROM users WHERE id=$1")
            .bind(user.as_str())
            .fetch_optional(&self.pool)
            .await?;
        Ok(thinking.flatten())
    }

    pub(crate) async fn set_user_thinking(&self, user: &UserKey, thinking: &str) -> Result<()> {
        sqlx::query("UPDATE users SET thinking=$2 WHERE id=$1")
            .bind(user.as_str())
            .bind(thinking)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Whether Jev picks the user's model and thinking per prompt.
    pub(crate) async fn user_auto_model(&self, user: &UserKey) -> Result<bool> {
        let auto = sqlx::query_scalar("SELECT auto_model FROM users WHERE id=$1")
            .bind(user.as_str())
            .fetch_optional(&self.pool)
            .await?;
        Ok(auto.unwrap_or(false))
    }

    pub(crate) async fn set_user_auto_model(&self, user: &UserKey, enabled: bool) -> Result<()> {
        sqlx::query("UPDATE users SET auto_model=$2 WHERE id=$1")
            .bind(user.as_str())
            .bind(enabled)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Every account.
    pub async fn user_ids(&self) -> Result<Vec<String>> {
        let users = sqlx::query_scalar("SELECT id FROM users")
            .fetch_all(&self.pool)
            .await?;
        Ok(users)
    }
}
