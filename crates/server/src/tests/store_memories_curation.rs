use super::*;
use crate::store::TestDb;

#[tokio::test]
async fn signals_noted_during_curation_are_kept() {
    let db = TestDb::new().await.unwrap();
    let store = db.store().await.unwrap();
    let alice = UserKey::for_test("alice");
    for _ in 0..2 {
        store
            .note_signal(&alice, &Scope::User, Signal::Close)
            .await
            .unwrap();
    }
    let counted = store.curation_states(&alice).await.unwrap()[&Scope::User].clone();
    // Noted while curation ran.
    store
        .note_signal(&alice, &Scope::User, Signal::Close)
        .await
        .unwrap();
    store
        .mark_curated(&alice, &Scope::User, 5, &counted)
        .await
        .unwrap();
    let state = &store.curation_states(&alice).await.unwrap()[&Scope::User];
    assert_eq!(state.close, 1);
    assert_eq!(state.crowded, 0);
    assert_eq!(state.size, 5);
}
