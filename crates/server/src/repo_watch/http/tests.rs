use super::*;
use crate::repo_watch::test_host::{Reply, TestHost};
use reqwest::{Client, redirect::Policy};
use secrecy::SecretString;
use serde::Deserialize;
use std::sync::Arc;
use tokio::sync::Semaphore;

#[derive(Debug, Deserialize, PartialEq)]
struct Count {
    n: u32,
}

/// Calls `url` on the host with a plain client: the test host is on
/// localhost, which the server's own client refuses.
async fn call<T: DeserializeOwned>(
    calls: &HostCalls,
    who: &Who,
    url: &str,
) -> Result<T, HostError> {
    let client = Client::new();
    let limit = Arc::new(Semaphore::new(2));
    let token = SecretString::from("tok");
    let caller = Caller {
        client: &client,
        calls: &limit,
        host: "github.com",
        token: &token,
    };
    calls.get_json(&caller, who, url).await
}

fn who(name: &str) -> Who {
    Who(name.to_owned())
}

#[tokio::test]
async fn an_unchanged_reply_is_a_304_served_from_the_kept_copy() {
    let host = TestHost::start(|seen| {
        if seen.header("if-none-match").as_deref() == Some("\"v1\"") {
            return Reply::status("304 Not Modified")
                .with("x-ratelimit-remaining", "4999")
                .with("x-ratelimit-reset", "4102444800");
        }
        Reply::ok(r#"{"n": 7}"#)
            .with("etag", "\"v1\"")
            .with("x-ratelimit-remaining", "4999")
            .with("x-ratelimit-reset", "4102444800")
    })
    .await;
    let calls = HostCalls::default();
    let url = format!("{}/repos/acme/widget/actions/runs", host.base);
    let first: Count = call(&calls, &who("alice"), &url).await.expect("a reply");
    let again: Count = call(&calls, &who("alice"), &url).await.expect("a reply");
    assert_eq!(first, Count { n: 7 });
    assert_eq!(again, first);
    let seen = host.seen();
    assert_eq!(seen.len(), 2);
    assert_eq!(seen[0].header("if-none-match"), None);
    assert_eq!(seen[1].header("if-none-match").as_deref(), Some("\"v1\""));
    assert_eq!(
        seen[1].header("authorization").as_deref(),
        Some("token tok")
    );
    // Another user's ask never reuses alice's copy.
    let _: Count = call(&calls, &who("bob"), &url).await.expect("a reply");
    assert_eq!(host.seen()[2].header("if-none-match"), None);
    assert_eq!(calls.limited_until(&who("alice"), SystemTime::now()), None);
}

#[tokio::test]
async fn a_low_rate_limit_pauses_polling_until_its_reset() {
    let host = TestHost::start(|_| {
        Reply::ok(r#"{"n": 1}"#)
            .with("x-ratelimit-remaining", "12")
            .with("x-ratelimit-reset", "4102444800")
    })
    .await;
    let calls = HostCalls::default();
    let _: Count = call(&calls, &who("alice"), &format!("{}/x", host.base))
        .await
        .expect("a reply");
    let until = calls
        .limited_until(&who("alice"), SystemTime::now())
        .expect("limited");
    assert_eq!(until, UNIX_EPOCH + Duration::from_secs(4_102_444_800));
    assert_eq!(calls.limited_until(&who("bob"), SystemTime::now()), None);
    // Past the reset, polling goes on.
    assert_eq!(
        calls.limited_until(&who("alice"), until + Duration::from_secs(1)),
        None
    );
}

#[tokio::test]
async fn a_log_redirect_is_followed_without_the_token_and_only_its_end_kept() {
    let log: String = (1..=500).map(|i| format!("line {i}\n")).collect();
    let body = log.clone();
    let host = TestHost::start(move |seen| match seen.path.as_str() {
        "/repos/acme/widget/actions/jobs/9/logs" => {
            Reply::status("302 Found").with("location", "/blob/log.txt?sig=abc")
        }
        "/blob/log.txt?sig=abc" => Reply::ok(&body),
        _ => Reply::status("404 Not Found"),
    })
    .await;
    let calls = HostCalls::default();
    // As the server's own client: it follows no redirect itself.
    let client = Client::builder()
        .redirect(Policy::none())
        .build()
        .expect("a client");
    let limit = Arc::new(Semaphore::new(2));
    let token = SecretString::from("tok");
    let caller = Caller {
        client: &client,
        calls: &limit,
        host: "github.com",
        token: &token,
    };
    let tail = calls
        .tail(
            &caller,
            &format!("{}/repos/acme/widget/actions/jobs/9/logs", host.base),
            TailLimits {
                keep: 100,
                max: 1 << 20,
            },
        )
        .await
        .expect("the log");
    assert!(tail.cut);
    assert_eq!(tail.text.len(), 100);
    assert!(tail.text.ends_with("line 500\n"), "{}", tail.text);
    let seen = host.seen();
    assert_eq!(seen.len(), 2);
    assert!(seen[0].header("authorization").is_some());
    assert_eq!(
        seen[1].header("authorization"),
        None,
        "the storage URL never sees the token"
    );
    assert!(log.len() > 100);
}
