use super::*;
use crate::{
    account::user::User,
    store::{Store, TestDb},
};
use serde_json::json;
use std::sync::Arc;

pub(super) async fn alice() -> (Arc<User>, Store, TestDb) {
    let db = TestDb::new().await.unwrap();
    let store = db.store().await.unwrap();
    (User::for_test(store.clone(), "alice"), store, db)
}

#[tokio::test]
pub(super) async fn lists_catalog_without_composio_key() {
    let (alice, store, _db) = alice().await;
    let list = alice.apps().await.unwrap();
    let slugs: Vec<&str> = list.iter().map(|a| a.slug.as_str()).collect();
    assert_eq!(
        slugs,
        [
            "gmail",
            "googlecalendar",
            "notion",
            "googledrive",
            "outlook",
            "microsoft_teams",
            "one_drive",
            "excel",
            "slack",
            "fastmail"
        ]
    );
    let (composio, fastmail) = list.split_at(9);
    assert!(
        composio
            .iter()
            .all(|a| !a.connected && !a.configured && a.logo_url.is_none())
    );
    assert!(!fastmail[0].connected && fastmail[0].configured);
    assert_eq!(fastmail[0].kind, AppKind::Fastmail {});

    alice
        .save_mcp_credentials("fastmail", json!({"client_id": "c"}))
        .await
        .unwrap();
    let list = alice.apps().await.unwrap();
    assert!(list[9].connected);
    User::for_test(store.clone(), "bob")
        .save_mcp_credentials("fastmail", json!({"client_id": "c"}))
        .await
        .unwrap();
    assert!(alice.apps().await.unwrap()[9].connected);
}

#[tokio::test]
pub(super) async fn mcp_auth_checks_app_and_redirect() {
    let (alice, _store, _db) = alice().await;
    let error = alice
        .start_app_auth("gmail", "http://localhost:4000/callback", false)
        .await
        .unwrap_err();
    assert!(matches!(error, ApiError::BadRequest { .. }));
    let error = alice
        .start_app_auth("fastmail", "https://example.com/callback", false)
        .await
        .unwrap_err();
    assert!(matches!(error, ApiError::BadRequest { .. }));

    let error = alice.disconnect_app("fastmail").await.unwrap_err();
    assert!(matches!(error, ApiError::NotFound {}));
    alice
        .save_mcp_credentials("fastmail", json!({"client_id": "c"}))
        .await
        .unwrap();
    alice.disconnect_app("fastmail").await.unwrap();
    assert!(alice.mcp_servers().await.unwrap().is_empty());
}
