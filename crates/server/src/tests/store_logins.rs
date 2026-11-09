use super::*;
use crate::{protocol::DeviceKind, store::TestDb};
use chrono::Duration;
use iroh::SecretKey;

fn key(endpoint: EndpointId) -> NewKey<'static> {
    NewKey {
        endpoint,
        name: "Mac",
        kind: DeviceKind::Desktop {},
    }
}

const LOGIN: Login<'static> = Login {
    sub: "google:1",
    email: Some("a@example.com"),
    name: None,
};

#[tokio::test]
async fn an_expired_code_is_refused_and_cleared() {
    let db = TestDb::new().await.expect("test database");
    let store = db.store().await.expect("test store");
    let phone = SecretKey::generate().public();
    store
        .add_login_code(b"old", &phone, Utc::now() - Duration::seconds(1))
        .await
        .unwrap();
    assert_eq!(
        store
            .complete_login(b"old", &LOGIN, key(phone))
            .await
            .unwrap(),
        LoginEnd::BadCode
    );
    store
        .add_login_code(b"new", &phone, Utc::now() + Duration::minutes(10))
        .await
        .unwrap();
    let left: i64 = sqlx::query_scalar("SELECT count(*) FROM login_codes")
        .fetch_one(&store.pool)
        .await
        .unwrap();
    assert_eq!(left, 1);
}

#[tokio::test]
async fn a_key_holds_only_so_many_codes() {
    let db = TestDb::new().await.expect("test database");
    let store = db.store().await.expect("test store");
    let phone = SecretKey::generate().public();
    let now = Utc::now() + Duration::minutes(10);

    for i in 0..MAX_CODES_PER_KEY + 2 {
        store
            .add_login_code(&i.to_be_bytes(), &phone, now + Duration::seconds(i))
            .await
            .unwrap();
    }

    assert_eq!(
        store
            .complete_login(&0i64.to_be_bytes(), &LOGIN, key(phone))
            .await
            .unwrap(),
        LoginEnd::BadCode,
        "the oldest code went"
    );
    assert!(matches!(
        store
            .complete_login(&(MAX_CODES_PER_KEY + 1).to_be_bytes(), &LOGIN, key(phone))
            .await
            .unwrap(),
        LoginEnd::SignedIn { .. }
    ));
}
