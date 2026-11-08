use super::*;
use std::os::unix::fs::PermissionsExt;

#[test]
fn writes_a_new_secret_key_for_the_owner_only() {
    let dir = tempfile::tempdir().expect("a temp dir");
    let path = dir.path().join("keys/server.key");
    let key = load_secret_key(Some(&path)).expect("a new key");
    let mode = fs::metadata(&path)
        .expect("the key file")
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);
    let again = load_secret_key(Some(&path)).expect("the same key");
    assert_eq!(again.public(), key.public());
}

#[test]
fn a_config_with_agent_tables_still_loads() {
    let config: Config = toml::from_str(
        r#"
            model = "openrouter:z-ai/glm-5.3-flash"
            model_choices = ["openrouter:openai/gpt-6-luna"]

            [agents.general]
            name = "General"
            when = "Anything"
            model = "openrouter:z-ai/glm-5.3-flash"
            thinking = "low"

            [agents.shopping]
            name = "Shopping"
            when = "Buying things"
            model = "openrouter:openai/gpt-6-luna"
            thinking = "medium"
            "#,
    )
    .expect("old agent tables are ignored");
    assert_eq!(config.model_choices.len(), 1);
}

#[test]
fn reports_are_optional_and_read_keys_and_addresses() {
    let model = r#"model = "openrouter:z-ai/glm-5.3-flash""#;
    let config: Config = toml::from_str(model).expect("a config without [reports] loads");
    assert_eq!(config.reports.per_key_per_day, 20);
    assert!(config.reports.r2.is_none() && config.reports.folder.is_none());

    let key = SecretKey::generate().public();
    let config: Config = toml::from_str(&format!(
        r#"
            {model}
            [reports]
            per_key_per_day = 3
            blocked_keys = ["{key}"]
            blocked_ips = ["192.0.2.7", "2001:db8::1"]
            [reports.r2]
            account_id = "account"
            bucket = "reports"
            access_key_id = "id"
            secret_access_key = "secret"
            "#
    ))
    .expect("a [reports] section loads");
    assert_eq!(config.reports.per_key_per_day, 3);
    assert_eq!(config.reports.blocked_keys, [key]);
    assert_eq!(config.reports.blocked_ips.len(), 2);
    assert_eq!(config.reports.r2.expect("r2 is set").bucket, "reports");
}

#[test]
fn provider_order_keeps_the_rest_of_the_routing() {
    let config: Config = toml::from_str(
        r#"
            model = "openrouter:z-ai/glm-5.3-flash"
            [provider_order]
            "openrouter:openai/gpt-6-luna" = ["openai/flex"]
            "#,
    )
    .expect("the config is valid");
    let luna: Model = "openrouter:openai/gpt-6-luna"
        .parse()
        .expect("a valid model");
    let routing = &config.model_routing()[&luna];
    assert_eq!(routing.order, ["openai/flex"]);
    assert_eq!(routing.data_collection.as_deref(), Some("deny"));
    assert_eq!(routing.zdr, Some(true));
    assert!(matches!(routing.sort, Some(ProviderSort::Throughput)));
}
