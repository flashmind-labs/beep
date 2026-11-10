use super::*;
use crate::beep::identity::load_or_create;
use beep_mesh::transport_config;
use iroh::{SecretKey, endpoint::presets};
use std::future::pending;
use tempfile::TempDir;
use tokio::time::{Instant, sleep};

/// The app on a loopback endpoint with no relay, so nothing reaches a
/// network, and no server.
async fn without_server(dir: &TempDir) -> Arc<Beep> {
    app(dir, false).await
}

async fn app(dir: &TempDir, server: bool) -> Arc<Beep> {
    let identity = load_or_create(&dir.path().join("identity.key")).expect("a key is made");
    let endpoint = Endpoint::builder(presets::Minimal)
        .secret_key(SecretKey::generate())
        .transport_config(transport_config())
        .bind()
        .await
        .expect("the endpoint binds");
    let file = StateFile::new(dir.path().join("state.json"));
    let (beep, _notices) = Beep::start(
        endpoint,
        identity,
        server,
        file,
        Chats::load(dir.path().join("sync")),
    );
    beep
}

#[tokio::test]
async fn a_request_only_a_server_answers_says_there_is_none() {
    let dir = TempDir::new().expect("a temp dir");
    let beep = without_server(&dir).await;
    assert!(!beep.has_server());
    // Linked or not, there is nothing to connect to.
    for link in [LinkState::Unlinked, LinkState::Linked, LinkState::Revoked] {
        beep.set_link_state(link);
        let error = beep.call(ListSessions {}).await.expect_err("no server");
        assert!(matches!(error, DesktopError::NoServer), "{error:?}");
    }

    assert!(matches!(
        beep.link_offer("Studio").await,
        Err(DesktopError::NoServer)
    ));
    assert!(!beep.has_login().await);
}

#[tokio::test]
async fn a_signed_out_mac_connects_to_nothing_and_signs_back_in_with_its_key() {
    let dir = TempDir::new().expect("a temp dir");
    // A file as an earlier run left it: linked, then signed out.
    fs::write(
        dir.path().join("state.json"),
        r#"{"link":"linked","signed_out":true}"#,
    )
    .expect("the state is written");
    let beep = app(&dir, true).await;
    assert!(beep.signed_out());
    assert_eq!(beep.link_state(), LinkState::Unlinked);
    assert!(!beep.linked());
    assert!(!beep.has_server());
    let error = beep.call(ListSessions {}).await.expect_err("signed out");
    assert!(matches!(error, DesktopError::Unlinked), "{error:?}");
    let error = beep
        .guest_call(ListSessions {})
        .await
        .expect_err("signed out");
    assert!(matches!(error, DesktopError::Unlinked), "{error:?}");
    assert!(!beep.has_login().await);
    assert!(!beep.following(), "no events stream was opened");
    // Signing in again is the one change: the same link, the same key.
    beep.set_signed_out(false);
    assert!(!beep.signed_out());
    assert!(beep.linked());
    assert!(beep.has_server());
    // The link set while signed out ends the sign-out.
    beep.sign_out().await;
    assert!(beep.signed_out());
    beep.set_link_state(LinkState::Linked);
    assert!(!beep.signed_out());
}

#[tokio::test]
async fn a_linked_saved_state_with_no_server_is_unlinked_and_calls_nothing() {
    let dir = TempDir::new().expect("a temp dir");
    let server = SecretKey::generate().public();
    // What a Mac linked to a server saved, started with the server switched off.
    fs::write(
        dir.path().join("state.json"),
        format!(r#"{{"link":"linked","server":"{server}"}}"#),
    )
    .expect("the state is written");
    let beep = app(&dir, false).await;
    assert_eq!(beep.link_state(), LinkState::Unlinked);
    assert!(!beep.linked());
    assert!(!beep.has_server());
    let error = beep.call(ListSessions {}).await.expect_err("no server");
    assert!(matches!(error, DesktopError::NoServer), "{error:?}");
    assert!(!beep.following(), "no events stream was opened");
    // The saved link is kept for the day the switch is on.
    let saved = fs::read_to_string(dir.path().join("state.json")).expect("state is kept");
    assert!(saved.contains("linked"));
}

#[tokio::test]
async fn a_server_is_a_link_only_once_the_app_is_linked() {
    let dir = TempDir::new().expect("a temp dir");
    let none = without_server(&dir).await;
    none.set_link_state(LinkState::Linked);
    assert!(!none.linked(), "a link with no server is no link");
    let dir = TempDir::new().expect("a temp dir");
    let beep = app(&dir, true).await;
    assert!(!beep.linked(), "a server alone is no link");
    beep.set_link_state(LinkState::Linked);
    assert!(beep.linked());
    beep.set_link_state(LinkState::Revoked);
    assert!(!beep.linked());
}

#[tokio::test]
async fn what_waits_for_a_link_wakes_on_the_sign_in_alone() {
    let dir = TempDir::new().expect("a temp dir");
    let none = without_server(&dir).await;
    none.set_link_state(LinkState::Linked);
    let waited = tokio::time::timeout(Duration::from_millis(50), none.until_linked()).await;
    assert!(waited.is_err(), "with no server it never links");

    let dir = TempDir::new().expect("a temp dir");
    let beep = app(&dir, true).await;
    let waiting = tokio::spawn({
        let beep = beep.clone();
        async move { beep.until_linked().await }
    });
    tokio::task::yield_now().await;
    assert!(!waiting.is_finished());

    beep.set_link_state(LinkState::Linked);
    tokio::time::timeout(Duration::from_secs(5), waiting)
        .await
        .expect("it wakes on the link")
        .expect("it ends");
}

// The clock is paused, so tokio moves it straight to the next timer
// instead of the test sleeping.
#[tokio::test(start_paused = true)]
async fn a_connect_that_never_answers_gives_up_in_time() {
    let started = Instant::now();
    let connect = pending::<anyhow::Result<()>>();
    let error = connect_in_time(connect, CONNECT_TIMEOUT)
        .await
        .expect_err("a server that never answers is an error");
    assert_eq!(started.elapsed(), CONNECT_TIMEOUT);
    assert_eq!(error.to_string(), "the server did not answer in 15 seconds");
}

#[tokio::test(start_paused = true)]
async fn a_connect_inside_the_limit_is_kept() {
    let connect = async {
        sleep(CONNECT_TIMEOUT - Duration::from_secs(1)).await;
        Ok(7)
    };
    let connected = connect_in_time(connect, CONNECT_TIMEOUT).await;
    assert_eq!(connected.expect("it answered in time"), 7);
}

#[tokio::test(start_paused = true)]
async fn a_failed_connect_keeps_its_error() {
    let connect = async { Err::<(), _>(anyhow::anyhow!("no route to the server")) };
    let error = connect_in_time(connect, CONNECT_TIMEOUT).await.unwrap_err();
    assert_eq!(error.to_string(), "no route to the server");
}

#[tokio::test]
async fn chat_lists_and_search_are_empty_without_a_server() {
    let dir = TempDir::new().expect("a temp dir");
    let beep = without_server(&dir).await;
    assert!(beep.sessions().await.expect("local sessions").is_empty());
    assert!(
        beep.search("example".to_owned())
            .await
            .expect("local search")
            .is_empty()
    );
}
