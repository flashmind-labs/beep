use super::*;

#[tokio::test]
async fn search_finds_words_inside_messages() {
    use crate::account::user::User;
    use crate::store::{TestDb, test_chat, test_turn};

    let db = TestDb::new().await.unwrap();
    let store = db.store().await.unwrap();
    let alice = User::for_test(store.clone(), "alice");
    let s1 = test_chat("s1");
    test_turn(
        &store,
        &UserKey::for_test("alice"),
        s1,
        "Book the \"Kimpton\" for 100%",
        1000,
    )
    .await
    .unwrap();

    let ids = |found: Vec<FoundSession>| -> Vec<String> {
        found.into_iter().map(|f| f.session.id).collect()
    };
    assert_eq!(
        ids(alice.search_sessions("\"kimpton\"", 10).await.unwrap()),
        [s1.to_string()]
    );
    assert_eq!(
        ids(alice.search_sessions("100%", 10).await.unwrap()),
        [s1.to_string()]
    );
    assert!(alice.search_sessions("10_%", 10).await.unwrap().is_empty());
    let bob = User::for_test(store.clone(), "bob");
    assert!(bob.search_sessions("kimpton", 10).await.unwrap().is_empty());
}

#[tokio::test]
async fn a_quiet_chat_is_kept_once_another_run_uses_it() {
    use crate::store::{CreateRun, RunStatus, TestDb, test_chat, test_turn};

    let db = TestDb::new().await.unwrap();
    let store = db.store().await.unwrap();
    let alice = UserKey::for_test("alice");
    let quiet = |chat, request: &'static str| {
        let store = store.clone();
        let alice = alice.clone();
        async move {
            let CreateRun::Created(run) = store
                .create_run(&alice, chat, request, "check")
                .await
                .unwrap()
            else {
                panic!("the run was not created");
            };
            store
                .finish(&alice, &run, None, RunStatus::Completed)
                .await
                .unwrap();
        }
    };

    // Another run started in the chat after the quiet one ended.
    let busy = test_chat("busy");
    quiet(busy, "req-1").await;
    let CreateRun::Created(_) = store.create_run(&alice, busy, "req-2", "hi").await.unwrap() else {
        panic!("the run was not created");
    };
    assert!(!store.delete_empty_session(&alice, busy).await.unwrap());
    assert!(store.in_chat(&alice, busy).await.unwrap());

    // Or it already shows a turn.
    let used = test_chat("used");
    quiet(used, "req-3").await;
    test_turn(&store, &alice, used, "hi", 1000).await.unwrap();
    assert!(!store.delete_empty_session(&alice, used).await.unwrap());
    assert!(store.in_chat(&alice, used).await.unwrap());

    let empty = test_chat("empty");
    quiet(empty, "req-4").await;
    assert!(store.delete_empty_session(&alice, empty).await.unwrap());
    assert!(!store.in_chat(&alice, empty).await.unwrap());
}

#[tokio::test]
async fn a_prompt_shows_while_its_run_goes_and_gives_way_to_the_turn() {
    use crate::store::{
        CreateRun, FinishedTurn, RunStatus, TestDb, TurnRecord, test_chat, test_steps,
    };
    use flashmind::core::ConversationEntry;

    let db = TestDb::new().await.unwrap();
    let store = db.store().await.unwrap();
    let alice = UserKey::for_test("alice");
    let start = |chat, request: &'static str| {
        let store = store.clone();
        let alice = alice.clone();
        async move {
            let CreateRun::Created(run) =
                store.create_run(&alice, chat, request, "hi").await.unwrap()
            else {
                panic!("the run was not created");
            };
            store.save_prompt(&alice, &run, "hi").await.unwrap();
            run
        }
    };
    let listed = || async {
        let sessions = store.sessions(&alice).await.unwrap();
        sessions.into_iter().map(|s| s.id).collect::<Vec<_>>()
    };
    let texts = |chat| {
        let store = store.clone();
        async move {
            let messages = store.chat_messages(chat).await.unwrap();
            messages
                .iter()
                .map(|m| m.text().to_owned())
                .collect::<Vec<_>>()
        }
    };

    // Listed with its prompt as soon as it starts, and found by its run.
    let s1 = test_chat("s1");
    let run = start(s1, "req-1").await;
    assert_eq!(listed().await, vec![s1.to_string()]);
    assert_eq!(texts(s1).await, ["hi"]);
    assert_eq!(
        store.active_run(&alice, s1).await.unwrap(),
        Some(run.clone())
    );
    // Every running chat of the user's, and none of anyone else's.
    assert_eq!(
        store.active_runs(&alice).await.unwrap(),
        std::slice::from_ref(&run)
    );
    let bob = UserKey::for_test("bob");
    assert!(store.active_runs(&bob).await.unwrap().is_empty());

    // The finished turn holds the prompt once.
    let mut turn = Conversation::new();
    turn.add(ConversationEntry::user("hi"));
    turn.add(ConversationEntry::assistant("re: hi"));
    let steps = test_steps(&turn);
    let finished = FinishedTurn {
        context: &turn,
        prompt_tokens: Some(0),
        steps: &steps,
        stands_alone: false,
        carried: TurnRecord::default(),
    };
    store
        .finish(&alice, &run, Some(finished), RunStatus::Completed)
        .await
        .unwrap();
    assert_eq!(texts(s1).await, ["hi", "re: hi"]);
    assert_eq!(store.active_run(&alice, s1).await.unwrap(), None);
    assert!(store.active_runs(&alice).await.unwrap().is_empty());

    // A first run that saved nothing takes its prompt and listing along.
    let s2 = test_chat("s2");
    let run = start(s2, "req-2").await;
    store
        .finish(&alice, &run, None, RunStatus::Failed)
        .await
        .unwrap();
    assert!(texts(s2).await.is_empty());
    assert_eq!(listed().await, vec![s1.to_string()]);

    // So does one a restart cut off.
    let s3 = test_chat("s3");
    start(s3, "req-3").await;
    store.recover().await.unwrap();
    assert!(texts(s3).await.is_empty());
    assert_eq!(listed().await, vec![s1.to_string()]);
}

#[tokio::test]
async fn titles_count_characters_not_bytes() {
    use crate::store::{TestDb, test_chat, test_turn};

    let db = TestDb::new().await.unwrap();
    let store = db.store().await.unwrap();
    let alice = UserKey::for_test("alice");
    let s1 = test_chat("s1");
    test_turn(&store, &alice, s1, "hi", 1000).await.unwrap();
    assert!(
        store
            .rename_session(&alice, s1, &"é".repeat(200))
            .await
            .unwrap()
    );
    assert!(
        store
            .rename_session(&alice, s1, &"é".repeat(201))
            .await
            .is_err()
    );
}

#[tokio::test]
async fn delete_turn_finds_the_turn_by_its_time() {
    use crate::store::{StoredMessage, TestDb, test_chat, test_turn};

    let db = TestDb::new().await.unwrap();
    let store = db.store().await.unwrap();
    let alice = UserKey::for_test("alice");
    let s1 = test_chat("s1");

    for (at, prompt) in [(1000, "one"), (2000, "two"), (3000, "three")] {
        test_turn(&store, &alice, s1, prompt, at).await.unwrap();
    }

    let contents = |messages: Vec<StoredMessage>| -> Vec<String> {
        messages.iter().map(|m| m.text().to_owned()).collect()
    };

    assert_eq!(
        store
            .delete_turn(&alice, s1, 1500, TurnsToDelete::One)
            .await
            .unwrap(),
        DeletedTurn::NotFound
    );
    assert_eq!(
        store
            .delete_turn(&alice, s1, 2000, TurnsToDelete::One)
            .await
            .unwrap(),
        DeletedTurn::Deleted
    );
    assert_eq!(
        contents(store.chat_messages(s1).await.unwrap()),
        ["one", "re: one", "three", "re: three"]
    );
    assert_eq!(
        store
            .delete_turn(&alice, s1, 1000, TurnsToDelete::Onward)
            .await
            .unwrap(),
        DeletedTurn::Deleted
    );
    assert!(store.chat_messages(s1).await.unwrap().is_empty());
}
