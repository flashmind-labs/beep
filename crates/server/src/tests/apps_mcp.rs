use super::*;
use crate::store::TestDb;
use serde_json::json;

#[test]
fn empty_arguments_are_left_out() {
    assert_eq!(
        drop_empty(json!({"query": "hotel", "cursor": null, "limit": 10})),
        json!({"query": "hotel", "limit": 10})
    );
    assert_eq!(
        drop_empty(json!({"query": "hotel", "cursor": "None"})),
        json!({"query": "hotel"})
    );
    assert_eq!(drop_empty(Value::Null), json!({}));
}

#[test]
fn gmail_qualifiers_become_fastmail_ones() {
    let mut args =
        json!({"query": "from:bank newer_than:7d in:anywhere OLDER_THAN:1y", "limit": 5});
    fix_query(&mut args);
    assert_eq!(
        args,
        json!({"query": "from:bank after:7d before:1y", "limit": 5})
    );
    let mut args = json!({"limit": 5});
    fix_query(&mut args);
    assert_eq!(args, json!({"limit": 5}));
}

#[test]
fn redirect_must_be_loopback_http_with_port() {
    assert!(check_redirect("http://localhost:43123/callback").is_ok());
    assert!(check_redirect("http://127.0.0.1:43123/callback").is_ok());
    assert!(check_redirect("http://[::1]:43123/callback").is_ok());
    assert!(check_redirect("http://localhost/callback").is_err());
    assert!(check_redirect("https://localhost:443/callback").is_err());
    assert!(check_redirect("http://evil.example:8080/callback").is_err());
    assert!(check_redirect("http://localhost.evil.example:8080/").is_err());
}

#[tokio::test]
async fn provider_keeps_credentials_out_of_config() {
    let db = TestDb::new().await.unwrap();
    let store = db.store().await.unwrap();
    let user = User::for_test(store, "alice");
    let provider = StoreProvider {
        user: Arc::downgrade(&user),
        url: None,
    };

    let mut config = preset(FASTMAIL).unwrap();
    provider.save_config(&config).await.unwrap();
    assert!(provider.list_configs().await.unwrap().is_empty());

    let credentials = serde_json::json!({"client_id": "c", "token_response": null});
    provider
        .save_credentials(FASTMAIL, &credentials)
        .await
        .unwrap();
    // A stale copy without credentials must not clear the saved ones.
    config.url = Some("https://evil.example/mcp".to_owned());
    provider.save_config(&config).await.unwrap();

    let configs = provider.list_configs().await.unwrap();
    assert_eq!(configs.len(), 1);
    assert_eq!(configs[0].credentials, Some(credentials));
    assert_eq!(
        configs[0].url.as_deref(),
        Some("https://api.fastmail.com/mcp")
    );

    provider
        .save_credentials(FASTMAIL, &Value::Null)
        .await
        .unwrap();
    assert!(provider.list_configs().await.unwrap().is_empty());
    assert!(provider.load_credentials(FASTMAIL).await.unwrap().is_none());
}
