//! Route cards: what a `get_route` call found, by the call's id.

use super::Store;
use crate::{protocol::RouteCard, user::UserKey};
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
        .bind(user.as_str())
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
                .bind(user.as_str())
                .bind(call_id)
                .fetch_optional(&self.pool)
                .await?;
        Ok(card.map(serde_json::from_value).transpose()?)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        protocol::{RouteCard, RouteOption},
        store::{TestDb, test_chat, test_turn},
        user::UserKey,
    };

    fn card(minutes: u32) -> RouteCard {
        RouteCard {
            routes: vec![RouteOption {
                departs: "19:01".to_owned(),
                arrives: "19:24".to_owned(),
                minutes,
                transfers: 1,
                live: false,
                legs: Vec::new(),
            }],
        }
    }

    #[tokio::test]
    async fn a_card_is_its_owners_and_goes_with_the_chat() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let alice = UserKey::for_test("alice");
        let bob = UserKey::for_test("bob");
        let chat = test_chat("route");
        test_turn(&store, &alice, chat, "how do I get there", 1000)
            .await
            .unwrap();

        store
            .save_route_card(&alice, chat, "call_1", &card(23))
            .await
            .unwrap();
        // A retried call keeps the newer routes.
        store
            .save_route_card(&alice, chat, "call_1", &card(25))
            .await
            .unwrap();
        assert_eq!(
            store.route_card(&alice, "call_1").await.unwrap(),
            Some(card(25))
        );
        assert_eq!(store.route_card(&bob, "call_1").await.unwrap(), None);

        assert!(store.delete_session(&alice, chat).await.unwrap());
        assert_eq!(store.route_card(&alice, "call_1").await.unwrap(), None);
    }
}
