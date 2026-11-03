//! Accounts and the model settings each user picks.

use super::Store;
use crate::user::UserKey;
use anyhow::Result;

impl Store {
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

    /// Whether Jev picks thinking per prompt; on until the user turns it off.
    pub(crate) async fn user_auto_model(&self, user: &UserKey) -> Result<bool> {
        let auto =
            sqlx::query_scalar("SELECT auto_model OR NOT auto_model_set FROM users WHERE id=$1")
                .bind(user.as_str())
                .fetch_optional(&self.pool)
                .await?;
        Ok(auto.unwrap_or(true))
    }

    pub(crate) async fn set_user_auto_model(&self, user: &UserKey, enabled: bool) -> Result<()> {
        sqlx::query("UPDATE users SET auto_model=$2, auto_model_set=true WHERE id=$1")
            .bind(user.as_str())
            .bind(enabled)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}
