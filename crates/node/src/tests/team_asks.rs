use super::*;
use crate::team::board::tests::{Pair, login, repo};
use std::sync::{Arc, Mutex};

const LIMIT: &str = "team_ask is limited to 10 messages an hour";

/// Sessions that keep what they are handed.
#[derive(Default)]
struct Kept(Mutex<Vec<(String, Delivery, String)>>);

impl Sessions for Kept {
    fn deliver<'a>(
        &'a self,
        card: &'a str,
        delivery: Delivery,
        text: String,
    ) -> BoxFuture<'a, Result<(), String>> {
        crate::lock(&self.0).push((card.to_owned(), delivery, text));
        Box::pin(async { Ok(()) })
    }
}

impl Kept {
    fn taken(&self) -> Vec<(String, Delivery, String)> {
        crate::lock(&self.0).clone()
    }
}

/// The board's gateway, alice's, with sessions that keep what they get.
async fn served() -> (Pair, Arc<Kept>) {
    let pair = Pair::new().await;
    let kept = Arc::new(Kept::default());
    pair.gateway.serve_sessions(kept.clone());
    pair.gateway
        .with(|teams| teams.boards.set(&repo(), Vec::new()));

    (pair, kept)
}

fn ask(id: &str, to: Target, text: &str) -> TeamMsg {
    TeamMsg::Ask {
        id: id.to_owned(),
        repo: repo(),
        from: AgentRef {
            login: login("bob"),
            card: "s9@bobs-box".to_owned(),
        },
        to,
        text: text.to_owned(),
        reply_to: None,
    }
}

#[test]
fn eleventh_ask_in_an_hour_fails() {
    let mut limit = AskLimit::default();
    let (session, other) = (SessionId::from("s1"), SessionId::from("s2"));
    let now = Instant::now();

    for _ in 0..10 {
        limit.take(&session, now).unwrap();
    }

    assert_eq!(limit.take(&session, now), Err(LIMIT.to_owned()));
    assert_eq!(limit.take(&other, now), Ok(()));
}

#[test]
fn ask_window_slides() {
    let mut limit = AskLimit::default();
    let session = SessionId::from("s1");
    let start = Instant::now();

    for _ in 0..10 {
        limit.take(&session, start).unwrap();
    }

    let later = start + Duration::from_secs(59 * 60);
    assert_eq!(limit.take(&session, later), Err(LIMIT.to_owned()));
    assert_eq!(
        limit.take(&session, start + Duration::from_secs(61 * 60)),
        Ok(())
    );
}

#[tokio::test]
async fn ask_to_person_without_session_is_needs_you_card() {
    let (pair, kept) = served().await;
    let text = "Who owns the fee model? ".repeat(10);
    let msg = ask(
        "a1",
        Target::Person {
            login: login("alice"),
        },
        &text,
    );

    // Alice's laptop does not serve the board: the ask goes on to her gateway.
    pair.bob_link.reach(&pair.alice, &msg).await.unwrap();

    let cards = pair
        .gateway
        .with(|teams| teams.boards.cards(&repo()))
        .unwrap();
    assert_eq!(cards.len(), 1);
    assert_eq!(cards[0].id, "ask:a1");
    assert_eq!(cards[0].lane, Lane::NeedsYou {});
    assert_eq!(cards[0].login, login("alice"));
    assert_eq!(cards[0].title, text.chars().take(140).collect::<String>());
    assert!(kept.taken().is_empty());

    // Dismissed, it leaves the board.
    pair.gateway.dismiss_ask(&repo(), "ask:a1");
    let cards = pair
        .gateway
        .with(|teams| teams.boards.cards(&repo()))
        .unwrap();
    assert!(cards.is_empty());
}

#[tokio::test]
async fn ask_to_a_card_reaches_its_session_and_only_it_answers() {
    let (pair, kept) = served().await;
    let card = Card {
        id: "s1@alices-box".to_owned(),
        title: "Fix the fee model".to_owned(),
        lane: Lane::Working {},
        login: login("alice"),
        machine: "Alice's box".to_owned(),
        issue: Some(52),
        pr: None,
        updated_at: SystemTime::now(),
    };
    pair.gateway
        .with(|teams| teams.boards.set(&repo(), vec![card.clone()]));
    let to = Target::Agent {
        card: card.id.clone(),
    };

    pair.bob_link
        .reach(&pair.alice, &ask("a2", to.clone(), "Can I take #52?"))
        .await
        .unwrap();

    let taken = kept.taken();
    assert_eq!(taken.len(), 1);
    assert_eq!(
        (taken[0].0.as_str(), taken[0].1),
        (card.id.as_str(), Delivery::Ask)
    );
    assert!(taken[0].2.contains("Can I take #52?"));
    assert!(taken[0].2.contains("reply_to a2"));

    // Only the session asked answers, and only once.
    let answer = |card: &str| Outgoing {
        repo: repo(),
        from: AgentRef {
            login: login("alice"),
            card: card.to_owned(),
        },
        to: Target::Agent {
            card: "s9@bobs-box".to_owned(),
        },
        text: "Yes".to_owned(),
        reply_to: Some("a2".to_owned()),
    };
    let other = pair.gateway.send_ask(answer("s3@alices-box")).await;
    assert!(other.unwrap_err().contains("No ask a2 waits"));
    // Bob's devices take no links here: an answer that did not reach him
    // still waits to be sent.
    assert!(pair.gateway.send_ask(answer(&card.id)).await.is_err());
    let again = pair.gateway.send_ask(answer(&card.id)).await;
    assert!(!again.unwrap_err().contains("No ask a2 waits"));
}

#[tokio::test]
async fn reply_reaches_asking_session() {
    let (pair, kept) = served().await;
    let bob = pair.bob.clone();
    pair.gateway.with(|teams| {
        let sent = Sent {
            repo: repo(),
            asker: "s1@alices-box".to_owned(),
            mesh: bob.mesh,
            asked: "s9@bobs-box".to_owned(),
            at: SystemTime::now(),
        };
        teams.asks.sent.insert("a3".to_owned(), sent);
    });
    let reply = TeamMsg::Reply {
        id: "a3".to_owned(),
        text: "Take it, I am on #53".to_owned(),
    };

    pair.bob_link.reach(&pair.alice, &reply).await.unwrap();

    let taken = kept.taken();
    assert_eq!(taken.len(), 1);
    assert_eq!(
        (taken[0].0.as_str(), taken[0].1),
        ("s1@alices-box", Delivery::Reply)
    );
    assert!(taken[0].2.contains("Take it, I am on #53"));
    assert!(taken[0].2.contains("bob's agent"));

    // It is answered: a second reply finds nothing waiting.
    let again = pair.bob_link.reach(&pair.alice, &reply).await;
    assert_eq!(again, Err(NOT_WAITING.to_owned()));
}

#[tokio::test]
async fn asks_are_refused_when_switched_off_or_too_long() {
    let (pair, kept) = served().await;
    pair.gateway.with(|teams| {
        let mut record = teams.rosters.get(&repo()).unwrap().clone();
        record.asks = false;
        teams.rosters.refresh([(repo(), record)].into());
    });
    let to = Target::Person {
        login: login("alice"),
    };

    let off = pair
        .bob_link
        .reach(&pair.alice, &ask("a4", to.clone(), "Hi"))
        .await;
    assert_eq!(
        off,
        Err("alice does not take messages from teammates' agents".to_owned())
    );

    pair.gateway.with(|teams| {
        let mut record = teams.rosters.get(&repo()).unwrap().clone();
        record.asks = true;
        teams.rosters.refresh([(repo(), record)].into());
    });
    let long = "x".repeat(MAX_TEXT + 1);
    let refused = pair
        .bob_link
        .reach(&pair.alice, &ask("a5", to, &long))
        .await;
    assert!(refused.is_err());
    assert!(kept.taken().is_empty());
}

#[tokio::test]
async fn an_ask_in_another_members_name_is_refused() {
    let (pair, _kept) = served().await;
    let mut msg = ask(
        "a6",
        Target::Person {
            login: login("alice"),
        },
        "Hi",
    );
    if let TeamMsg::Ask { from, .. } = &mut msg {
        from.login = login("alice");
    }

    assert!(pair.bob_link.reach(&pair.alice, &msg).await.is_err());
    let cards = pair
        .gateway
        .with(|teams| teams.boards.cards(&repo()))
        .unwrap();
    assert!(cards.is_empty());
}
