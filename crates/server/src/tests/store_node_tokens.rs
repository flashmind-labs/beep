use super::*;
use crate::store::{LinkKey, NewKey, SignIn, TestDb};
use iroh::SecretKey;

const TTL: Duration = Duration::from_secs(600);

fn claim<'a>(hash: &'a str, endpoint: EndpointId) -> ClaimNode<'a> {
    ClaimNode {
        hash,
        endpoint,
        named: None,
        fallback: "host",
    }
}

#[tokio::test]
async fn a_token_links_a_machine_once() {
    let db = TestDb::new().await.expect("test database");
    let store = db.store().await.expect("test store");
    let alice = UserKey::for_test("alice");
    let machine = SecretKey::generate().public();
    store
        .create_node_token(&alice, "hash-1", Some("build box"), TTL)
        .await
        .unwrap();

    assert_eq!(
        store.node_token_claimer(&alice, "hash-1").await.unwrap(),
        Some(None)
    );
    let Claim::Linked(claimed) = store
        .claim_node_token(claim("hash-1", machine))
        .await
        .unwrap()
    else {
        panic!("a fresh token links");
    };
    assert_eq!(claimed.account, Account::new("alice"));
    assert_eq!(
        store.node_token_claimer(&alice, "hash-1").await.unwrap(),
        Some(Some(machine.to_string()))
    );
    let bob = UserKey::for_test("bob");
    assert_eq!(
        store.node_token_claimer(&bob, "hash-1").await.unwrap(),
        None
    );
    assert_eq!(claimed.account_name, "alice");
    assert_eq!(claimed.name, "build box");
    let SignIn::Account {
        account, device, ..
    } = store.sign_in(&machine).await.unwrap()
    else {
        panic!("the machine signs in");
    };
    assert_eq!(account.id, "alice");
    assert_eq!(device, DeviceKind::Desktop {});

    // Spent, for this machine and any other.
    let other = SecretKey::generate().public();
    for endpoint in [machine, other] {
        assert_eq!(
            store
                .claim_node_token(claim("hash-1", endpoint))
                .await
                .unwrap(),
            Claim::Used
        );
    }
    assert_eq!(
        store.claim_node_token(claim("nope", other)).await.unwrap(),
        Claim::Unknown
    );
}

#[tokio::test]
async fn an_expired_token_links_nothing() {
    let db = TestDb::new().await.expect("test database");
    let store = db.store().await.expect("test store");
    let alice = UserKey::for_test("alice");
    store
        .create_node_token(&alice, "old", None, TTL)
        .await
        .unwrap();
    sqlx::query("UPDATE node_tokens SET expires_at = now() - interval '1 second'")
        .execute(&store.pool)
        .await
        .unwrap();
    let machine = SecretKey::generate().public();
    assert_eq!(
        store.claim_node_token(claim("old", machine)).await.unwrap(),
        Claim::Expired
    );
    assert_eq!(store.linked_sign_in(&machine).await.unwrap(), None);
}

#[tokio::test]
async fn a_name_given_on_the_machine_wins_and_a_taken_key_spends_nothing() {
    let db = TestDb::new().await.expect("test database");
    let store = db.store().await.expect("test store");
    let (alice, bob) = (UserKey::for_test("alice"), UserKey::for_test("bob"));
    let machine = SecretKey::generate().public();
    let linked = store
        .link_key(
            &bob,
            NewKey {
                endpoint: machine,
                name: "bob's",
                kind: DeviceKind::Desktop {},
            },
        )
        .await
        .unwrap();
    assert!(matches!(linked, LinkKey::Linked(_)));
    store
        .create_node_token(&alice, "t", None, TTL)
        .await
        .unwrap();
    assert_eq!(
        store.claim_node_token(claim("t", machine)).await.unwrap(),
        Claim::OtherAccount
    );
    // Still good for the right machine.
    let fresh = SecretKey::generate().public();
    let named = ClaimNode {
        named: Some(" laptop "),
        ..claim("t", fresh)
    };
    let Claim::Linked(claimed) = store.claim_node_token(named).await.unwrap() else {
        panic!("the token was not spent");
    };
    assert_eq!(claimed.name, "laptop");
}

#[tokio::test]
async fn a_removed_machine_comes_back_under_its_new_name() {
    let db = TestDb::new().await.expect("test database");
    let store = db.store().await.expect("test store");
    let alice = UserKey::for_test("alice");
    let machine = SecretKey::generate().public();
    store
        .create_node_token(&alice, "first", Some("old box"), TTL)
        .await
        .unwrap();
    let first = store
        .claim_node_token(claim("first", machine))
        .await
        .unwrap();
    assert!(matches!(first, Claim::Linked(_)));
    assert!(
        store
            .revoke_key(&alice, &machine.to_string())
            .await
            .unwrap()
            .is_some()
    );
    assert_eq!(store.sign_in(&machine).await.unwrap(), SignIn::Revoked);

    store
        .create_node_token(&alice, "again", Some("new box"), TTL)
        .await
        .unwrap();
    let again = store
        .claim_node_token(claim("again", machine))
        .await
        .unwrap();
    assert!(matches!(again, Claim::Linked(ref node) if node.name == "new box"));
    let keys = store.account_keys(&alice).await.unwrap();
    let key = keys
        .iter()
        .find(|key| key.endpoint_id == machine.to_string())
        .expect("the key is back");
    assert_eq!(key.name, "new box");
    assert!(matches!(
        store.sign_in(&machine).await.unwrap(),
        SignIn::Account { .. }
    ));
}

#[tokio::test]
async fn only_the_maker_revokes_an_unused_token() {
    let db = TestDb::new().await.expect("test database");
    let store = db.store().await.expect("test store");
    let (alice, bob) = (UserKey::for_test("alice"), UserKey::for_test("bob"));
    store
        .create_node_token(&alice, "t", None, TTL)
        .await
        .unwrap();
    assert!(!store.revoke_node_token(&bob, "t").await.unwrap());
    assert!(store.revoke_node_token(&alice, "t").await.unwrap());
    let machine = SecretKey::generate().public();
    assert_eq!(
        store.claim_node_token(claim("t", machine)).await.unwrap(),
        Claim::Unknown
    );
}
