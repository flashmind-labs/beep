use super::*;
use crate::{
    protocol::MessageBody,
    store::{NewMessage, TestDb, TurnsToDelete, test_chat, test_turn},
};
use std::collections::HashSet;

/// A page as short labels, oldest first.
fn labels(page: &UpdatePage) -> Vec<String> {
    page.updates
        .iter()
        .map(|update| match update {
            Update::Message { message, .. } => match &message.body {
                MessageBody::User { content, .. } => format!("user:{content}"),
                MessageBody::Assistant { content } => format!("beep:{content}"),
                MessageBody::Member { content, .. } => format!("member:{content}"),
                MessageBody::Beep { content } => format!("beep:{content}"),
                other => format!("{other:?}"),
            },
            Update::Deleted { .. } => "deleted".to_owned(),
            Update::Chat { .. } => "chat".to_owned(),
            Update::ChatDeleted { .. } => "chat_deleted".to_owned(),
        })
        .collect()
}

#[tokio::test]
async fn a_client_gets_what_it_lacks_and_hears_of_deletes() {
    let db = TestDb::new().await.unwrap();
    let store = db.store().await.unwrap();
    let (alice, bob) = (UserKey::for_test("alice"), UserKey::for_test("bob"));
    let s1 = test_chat("s1");
    test_turn(&store, &alice, s1, "one", 1000).await.unwrap();
    test_turn(&store, &alice, s1, "two", 2000).await.unwrap();

    let all = store.updates_page(&alice, 0).await.unwrap();
    assert_eq!(
        labels(&all),
        [
            "chat",
            "user:one",
            "beep:re: one",
            "user:two",
            "beep:re: two"
        ]
    );
    assert!(!all.more && !all.reset);
    let ids: Vec<i64> = all
        .updates
        .iter()
        .map(|update| match update {
            Update::Message { update_id, .. }
            | Update::Deleted { update_id, .. }
            | Update::Chat { update_id, .. }
            | Update::ChatDeleted { update_id, .. } => *update_id,
        })
        .collect();
    assert!(ids.windows(2).all(|pair| pair[0] < pair[1]));
    assert_eq!(all.last, *ids.last().unwrap());

    let none = store.updates_page(&alice, all.last).await.unwrap();
    assert!(none.updates.is_empty());
    assert_eq!(none.last, all.last);
    assert!(
        store
            .updates_page(&bob, 0)
            .await
            .unwrap()
            .updates
            .is_empty()
    );

    store
        .delete_turn(&alice, s1, 1000, TurnsToDelete::One)
        .await
        .unwrap();
    let since = store.updates_page(&alice, all.last).await.unwrap();
    assert_eq!(labels(&since), ["deleted", "deleted"]);
    // A client starting now never sees the deleted messages.
    let fresh = store.updates_page(&alice, 0).await.unwrap();
    assert_eq!(
        labels(&fresh),
        ["chat", "user:two", "beep:re: two", "deleted", "deleted"]
    );
}

#[tokio::test]
async fn group_members_share_one_copy_and_newcomers_get_the_backlog() {
    let db = TestDb::new().await.unwrap();
    let store = db.store().await.unwrap();
    let (alice, bob) = (UserKey::for_test("alice"), UserKey::for_test("bob"));
    let g1 = test_chat("g1");
    store.create_group(&alice, g1, "Trip", "Ali").await.unwrap();
    store
        .post_to_group(
            &alice,
            NewMessage {
                group_id: g1,
                request_id: test_chat("r1"),
                content: "hi",
            },
        )
        .await
        .unwrap();
    store.post_beep(g1, "hello Ali").await.unwrap();
    let before = store.updates_page(&alice, 0).await.unwrap();
    assert_eq!(labels(&before), ["chat", "member:hi", "beep:hello Ali"]);

    let token = store
        .group_invite(&alice, g1, false)
        .await
        .unwrap()
        .unwrap();
    store.join_group(&bob, &token, "Bo").await.unwrap();
    let joined = store.updates_page(&bob, 0).await.unwrap();
    assert_eq!(labels(&joined), ["chat", "member:hi", "beep:hello Ali"]);
    let Some(Update::Chat { chat, .. }) = joined.updates.first() else {
        panic!("no chat first: {joined:?}");
    };
    assert_eq!(chat.members.len(), 2);
    assert_eq!(
        labels(&store.updates_page(&alice, before.last).await.unwrap()),
        ["chat"]
    );

    let seen = store.last_update(alice.as_ref()).await.unwrap();
    assert!(store.leave_group(&bob, g1).await.unwrap());
    assert_eq!(
        labels(&store.updates_page(&bob, 0).await.unwrap()),
        ["chat_deleted"]
    );
    assert_eq!(
        labels(&store.updates_page(&alice, seen).await.unwrap()),
        ["chat"]
    );
}

#[tokio::test]
async fn a_client_behind_dropped_tombstones_starts_over() {
    let db = TestDb::new().await.unwrap();
    let store = db.store().await.unwrap();
    let alice = UserKey::for_test("alice");
    let s1 = test_chat("s1");
    test_turn(&store, &alice, s1, "one", 1000).await.unwrap();
    test_turn(&store, &alice, s1, "two", 2000).await.unwrap();
    store
        .delete_turn(&alice, s1, 1000, TurnsToDelete::One)
        .await
        .unwrap();
    let last = store.last_update(alice.as_ref()).await.unwrap();
    assert_eq!(store.prune_updates().await.unwrap(), 0);

    sqlx::query("UPDATE updates SET updated_at = now() - interval '40 days' WHERE op = 'deleted'")
        .execute(store.pool())
        .await
        .unwrap();
    assert_eq!(store.prune_updates().await.unwrap(), 1);
    let behind = store.updates_page(&alice, 1).await.unwrap();
    assert!(behind.reset);
    assert_eq!(labels(&behind), ["chat", "user:two", "beep:re: two"]);
    let current = store.updates_page(&alice, last).await.unwrap();
    assert!(!current.reset && current.updates.is_empty());
    // A copy from another database is ahead of this log.
    let ahead = store.updates_page(&alice, last + 1).await.unwrap();
    assert!(ahead.reset);
    assert_eq!(labels(&ahead), labels(&behind));
    assert!(!store.updates_page(&alice, 0).await.unwrap().reset);
}

#[tokio::test]
async fn a_rebuild_continues_across_pages_below_the_reset_threshold() {
    for (count, bytes) in [(PAGE_UPDATES + 1, 1), (5, PAGE_BYTES / 2)] {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let alice = UserKey::for_test("alice");
        let chat = test_chat("paged");
        store
            .create_group(&alice, chat, "Trip", "Ali")
            .await
            .unwrap();
        let before = store.last_update(alice.as_ref()).await.unwrap();
        sqlx::query(
            "WITH added AS (
                     INSERT INTO messages (chat_id, role, content)
                     SELECT $1, 'beep', repeat('x', $2) FROM generate_series(1, $3::bigint)
                     RETURNING id
                 ), logged AS (
                     INSERT INTO updates (user_id, update_id, chat_id, message_id, op)
                     SELECT $4, $5 + row_number() OVER (ORDER BY id), $1, id, 'message'
                     FROM added RETURNING update_id
                 )
                 UPDATE sync_counters SET last = (SELECT max(update_id) FROM logged),
                     pruned_below = (SELECT max(update_id) + 1 FROM logged)
                 WHERE user_id = $4",
        )
        .bind(chat)
        .bind(bytes as i32)
        .bind(count)
        .bind(alice.as_ref())
        .bind(before)
        .execute(store.pool())
        .await
        .unwrap();
        let newest = store.last_update(alice.as_ref()).await.unwrap();

        // Both a new device and a stale cache must finish the rebuild.
        for start in [0, before] {
            let mut cursor = start;
            let mut seen = HashSet::new();
            let mut finished = false;

            for page_number in 0..count + 2 {
                let page = store.updates_page(&alice, cursor).await.unwrap();
                assert_eq!(page.reset, start > 0 && page_number == 0);
                assert!(page.last.abs() > cursor.abs() || page_number == 0);

                for update in &page.updates {
                    let id = match update {
                        Update::Chat { update_id, .. } | Update::Message { update_id, .. } => {
                            *update_id
                        }
                        other => panic!("unexpected update: {other:?}"),
                    };
                    assert!(seen.insert(id), "a page repeated an update");
                }
                cursor = page.last;

                if !page.more {
                    finished = true;
                    break;
                }

                assert!(cursor < 0, "rebuild cursor must bypass the reset threshold");
            }

            assert!(finished, "the rebuild must finish");
            assert_eq!(seen.len() as i64, newest);
            assert_eq!(cursor, newest);
            let tail = store.updates_page(&alice, cursor).await.unwrap();
            assert!(!tail.reset && !tail.more && tail.updates.is_empty());
        }
    }
}

#[tokio::test]
async fn an_empty_rebuild_advances_past_the_reset_threshold() {
    let db = TestDb::new().await.unwrap();
    let store = db.store().await.unwrap();
    let alice = UserKey::for_test("alice");
    sqlx::query("INSERT INTO sync_counters (user_id, last, pruned_below) VALUES ($1, 10, 11)")
        .bind(alice.as_ref())
        .execute(store.pool())
        .await
        .unwrap();

    for cursor in [0, 1, -1] {
        let page = store.updates_page(&alice, cursor).await.unwrap();
        assert_eq!(page.reset, cursor == 1);
        assert!(!page.more && page.updates.is_empty());
        assert_eq!(page.last, 10);
        let tail = store.updates_page(&alice, page.last).await.unwrap();
        assert!(!tail.reset && !tail.more && tail.updates.is_empty());
    }
}

#[tokio::test]
async fn a_write_wakes_the_users_watchers() {
    let db = TestDb::new().await.unwrap();
    let store = db.store().await.unwrap();
    let alice = UserKey::for_test("alice");
    let mut watch = store.watch_updates(alice.as_ref()).await.unwrap();
    assert_eq!(*watch.borrow_and_update(), 0);
    test_turn(&store, &alice, test_chat("s1"), "one", 1000)
        .await
        .unwrap();
    assert!(watch.has_changed().unwrap());
    assert_eq!(
        *watch.borrow_and_update(),
        store.last_update(alice.as_ref()).await.unwrap()
    );
}
