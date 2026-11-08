use crate::{
    account::user::UserKey,
    protocol::{RouteCard, RouteOption},
    store::{TestDb, test_chat, test_turn},
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
