use crate::store::{SignIn, TestDb};
use iroh::SecretKey;

#[tokio::test]
async fn the_first_claim_wins() {
    let db = TestDb::new().await.expect("test database");
    let store = db.store().await.expect("test store");
    assert_eq!(store.instance_owner().await.unwrap(), None);
    let first = SecretKey::generate().public();
    let Some(SignIn::Account { account, .. }) = store.claim_instance_new(&first).await.unwrap()
    else {
        panic!("the first key claims the instance");
    };
    assert_eq!(
        store.instance_owner().await.unwrap().as_deref(),
        Some(account.id.as_str())
    );
    let second = SecretKey::generate().public();
    assert_eq!(store.claim_instance_new(&second).await.unwrap(), None);
    assert!(!store.claim_instance("alice", &second).await.unwrap());
    // The refused key got no account.
    assert_eq!(store.linked_sign_in(&second).await.unwrap(), None);
}
