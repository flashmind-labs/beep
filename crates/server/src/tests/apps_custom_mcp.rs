use super::*;

fn defs(names: &[&str]) -> Vec<McpToolDef> {
    names
        .iter()
        .map(|name| McpToolDef {
            name: (*name).to_owned(),
            description: None,
            input_schema: json!({"type": "object"}),
        })
        .collect()
}

#[test]
fn a_clashing_name_is_not_usable() {
    let defs = defs(&["get-item", "get_item", "other"]);
    let usable: Vec<bool> = tool_list("mcp_notes", &defs, None)
        .tools
        .iter()
        .map(|tool| tool.usable)
        .collect();
    assert_eq!(usable, [true, false, true]);
}

#[test]
fn only_public_addresses_pass() {
    for ip in [
        "127.0.0.1",
        "10.1.2.3",
        "172.16.0.1",
        "192.168.1.1",
        "169.254.169.254",
        "100.64.0.1",
        "0.0.0.0",
        "::1",
        "fe80::1",
        "fd00::1",
        "::ffff:127.0.0.1",
        "200:1234::1",
        "2001:db8::1",
        "2002:7f00:1::1",
    ] {
        assert!(!is_public(ip.parse().unwrap()), "{ip} passed");
    }
    for ip in ["1.1.1.1", "8.8.8.8", "2606:4700:4700::1111"] {
        assert!(is_public(ip.parse().unwrap()), "{ip} refused");
    }
}

#[tokio::test]
async fn urls_must_be_https_without_credentials() {
    assert_eq!(
        check_url("http://example.com/mcp").await,
        Err(UrlError::NotHttps)
    );
    assert_eq!(check_url("example.com/mcp").await, Err(UrlError::NotHttps));
    assert_eq!(
        check_url("https://user:pw@example.com/mcp").await,
        Err(UrlError::Credentials)
    );
    assert_eq!(
        check_url("https://127.0.0.1/mcp").await,
        Err(UrlError::Private)
    );
    assert_eq!(
        check_url("https://[::1]:8443/mcp").await,
        Err(UrlError::Private)
    );
    assert_eq!(
        check_url("https://localhost/mcp").await,
        Err(UrlError::Private)
    );
}

#[test]
fn a_host_takes_a_few_calls_a_second_from_everyone() {
    let limits = Limits::default();
    let now = Instant::now();
    let mut permits = Vec::new();
    for i in 0..HOST_CALLS_PER_SECOND {
        permits.push(
            limits
                .begin_at("mcp.example", &format!("user{i}"), now)
                .unwrap(),
        );
    }
    assert_eq!(
        limits.begin_at("mcp.example", "other", now).err(),
        Some(Refused::HostBusy)
    );
    // Another host has its own budget.
    assert!(limits.begin_at("other.example", "other", now).is_ok());
    drop(permits);
    let later = now + Duration::from_secs(1);
    assert!(limits.begin_at("mcp.example", "other", later).is_ok());
}

#[test]
fn one_user_gets_a_few_calls_a_minute() {
    let limits = Limits::default();
    let start = Instant::now();
    for i in 0..USER_CALLS_PER_MINUTE {
        // Spread out so the host's per-second budget is not what stops it.
        let at = start + Duration::from_secs(u64::from(i));
        limits.begin_at("mcp.example", "alice", at).unwrap();
    }
    let at = start + Duration::from_secs(30);
    assert_eq!(
        limits.begin_at("mcp.example", "alice", at).err(),
        Some(Refused::UserLimit)
    );
    assert!(limits.begin_at("mcp.example", "bob", at).is_ok());
    let next_minute = start + Duration::from_secs(61);
    assert!(limits.begin_at("mcp.example", "alice", next_minute).is_ok());
}
