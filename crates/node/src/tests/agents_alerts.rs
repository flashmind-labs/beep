use super::*;
use beep_proto::SessionId;
use beep_proto::node::agent::AlertKind;
use iroh::SecretKey;
use std::time::Duration;
use tokio::time::timeout;

fn alert(session: &str) -> Alert {
    Alert {
        session: session.into(),
        agent: "claude".into(),
        title: None,
        cwd: String::new(),
        kind: AlertKind::Failed {
            reason: String::new(),
        },
    }
}

fn follower() -> EndpointId {
    SecretKey::generate().public()
}

/// The next alert, taken as a stream does: read, then sent.
async fn take(follow: &Follow<'_, Alert>) -> SessionId {
    let alert = timeout(Duration::from_secs(10), follow.next())
        .await
        .expect("an alert is there");
    follow.sent();
    alert.session
}

async fn nothing(follow: &Follow<'_, Alert>) -> bool {
    timeout(Duration::from_millis(50), follow.next())
        .await
        .is_err()
}

/// The instant `seconds` ago, or `None` on a host booted more recently,
/// where it cannot be represented and the test has nothing to check.
fn ago(seconds: u64) -> Option<Instant> {
    Instant::now().checked_sub(Duration::from_secs(seconds))
}

#[tokio::test]
async fn a_new_follower_starts_at_the_last_ten_minutes_and_a_returning_one_where_it_left() {
    let hub = AlertHub::<Alert>::default();
    let (new, returning) = (follower(), follower());
    let (Some(stale), Some(older), Some(newer)) = (ago(30 * 60), ago(9 * 60), ago(60)) else {
        return;
    };
    hub.record_at(alert("stale"), stale);
    hub.record_at(alert("older"), older);
    hub.record_at(alert("newer"), newer);
    // A follower with a place before the stale alert still gets it.
    lock(&hub.held).place(returning, 0);
    let (new, returning) = (hub.follow(new), hub.follow(returning));
    assert_eq!(take(&new).await, "older");
    assert_eq!(take(&new).await, "newer");
    assert!(nothing(&new).await);
    assert_eq!(take(&returning).await, "stale");
    assert_eq!(take(&returning).await, "older");
}

#[tokio::test]
async fn a_new_follower_with_only_stale_alerts_waits_for_the_next() {
    let hub = AlertHub::<Alert>::default();
    let Some(stale) = ago(FRESH.as_secs() + 1) else {
        return;
    };
    hub.record_at(alert("stale"), stale);
    let new = hub.follow(follower());
    assert!(nothing(&new).await);
    hub.record(alert("now"));
    assert_eq!(take(&new).await, "now");
}

#[tokio::test]
async fn an_offline_desktop_gets_what_it_missed() {
    let hub = AlertHub::<Alert>::default();
    let (server, desktop) = (hub.follow(follower()), follower());
    hub.record(alert("one"));
    assert_eq!(take(&server).await, "one");
    // The desktop was never connected, and the server was.
    hub.record(alert("two"));
    assert_eq!(take(&server).await, "two");
    let desktop = hub.follow(desktop);
    assert_eq!(take(&desktop).await, "one");
    assert_eq!(take(&desktop).await, "two");
}

#[tokio::test]
async fn the_server_and_a_desktop_each_get_every_alert() {
    let hub = AlertHub::<Alert>::default();
    let (server, desktop) = (hub.follow(follower()), hub.follow(follower()));

    for session in ["a", "b", "c"] {
        hub.record(alert(session));
    }

    for follow in [&server, &desktop] {
        for session in ["a", "b", "c"] {
            assert_eq!(take(follow).await, session);
        }

        assert!(nothing(follow).await);
    }
}

#[tokio::test]
async fn a_reconnecting_follower_gets_nothing_twice() {
    let hub = AlertHub::<Alert>::default();
    let id = follower();
    hub.record(alert("a"));
    hub.record(alert("b"));
    let first = hub.follow(id);
    assert_eq!(take(&first).await, "a");
    // A new stream of the same follower carries on, and an alert read
    // but never sent comes again.
    let again = hub.follow(id);
    let unsent = again.next().await;
    assert_eq!(unsent.session, "b");
    let again = hub.follow(id);
    assert_eq!(take(&again).await, "b");
    assert!(nothing(&again).await);
    hub.record(alert("c"));
    assert_eq!(take(&again).await, "c");
}

#[tokio::test]
async fn a_slow_follower_holds_nobody_up() {
    let hub = AlertHub::<Alert>::default();
    let (slow, quick) = (hub.follow(follower()), hub.follow(follower()));

    for n in 0..MAX_KEPT + 10 {
        hub.record(alert(&n.to_string()));
        assert_eq!(take(&quick).await, n.to_string());
    }
    // The slow one fell off the ring and resumes from the oldest kept.
    assert_eq!(take(&slow).await, "10");
}

#[tokio::test]
async fn only_the_most_recent_followers_are_remembered() {
    let hub = AlertHub::<Alert>::default();
    let ids: Vec<EndpointId> = (0..=MAX_FOLLOWERS).map(|_| follower()).collect();
    hub.record(alert("a"));

    for id in &ids {
        let follow = hub.follow(*id);
        follow.next().await;
        follow.sent();
    }

    assert_eq!(lock(&hub.held).cursors.len(), MAX_FOLLOWERS);
    assert!(!lock(&hub.held).cursors.contains_key(&ids[0]));
}

#[tokio::test]
async fn sent_marks_the_alert_next_gave_even_when_older_ones_were_dropped() {
    let hub = AlertHub::<Alert>::default();
    let follow = hub.follow(follower());
    hub.record(alert("first"));
    assert_eq!(follow.next().await.session, "first");

    for n in 0..MAX_KEPT {
        hub.record(alert(&format!("later {n}")));
    }

    follow.sent();
    // "first" went out; the oldest kept alert was not sent and still comes.
    assert_eq!(follow.next().await.session, "later 0");
}
