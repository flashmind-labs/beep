//! Route cards: what a `get_route` call found, by the call's id.

use super::Store;
use crate::{account::user::UserKey, protocol::RouteCard};
use anyhow::Result;
use serde_json::Value;
use uuid::Uuid;

impl Store {
    pub(crate) async fn save_route_card(
        &self,
        user: &UserKey,
        chat: Uuid,
        call_id: &str,
        card: &RouteCard,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO route_cards (user_id, call_id, chat_id, card) VALUES ($1, $2, $3, $4)
             ON CONFLICT (user_id, call_id) DO UPDATE SET card = excluded.card",
        )
        .bind(user.as_ref())
        .bind(call_id)
        .bind(chat)
        .bind(serde_json::to_value(card)?)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// The card `call_id` saved, when it is the user's.
    pub(crate) async fn route_card(
        &self,
        user: &UserKey,
        call_id: &str,
    ) -> Result<Option<RouteCard>> {
        let card: Option<Value> =
            sqlx::query_scalar("SELECT card FROM route_cards WHERE user_id = $1 AND call_id = $2")
                .bind(user.as_ref())
                .bind(call_id)
                .fetch_optional(&self.pool)
                .await?;
        Ok(card.map(serde_json::from_value).transpose()?)
    }
}

#[cfg(test)]
#[path = "../tests/store_routes.rs"]
mod tests;
