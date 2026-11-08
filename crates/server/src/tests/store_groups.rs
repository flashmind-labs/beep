use super::*;
use crate::store::{TestDb, test_chat};

fn post<'a>(group_id: Uuid, request: &str, content: &'a str) -> NewMessage<'a> {
    NewMessage {
        group_id,
        request_id: test_chat(request),
        content,
    }
}

#[tokio::test]
async fn members_join_by_invite_and_a_reset_breaks_the_old_link() {
    let db = TestDb::new().await.unwrap();
    let store = db.store().await.unwrap();
    let g1 = test_chat("g1");
    let (alice, bob) = (UserKey::for_test("alice"), UserKey::for_test("bob"));
    assert!(store.create_group(&alice, g1, "Trip", "Ali").await.unwrap());
    assert!(!store.create_group(&bob, g1, "Mine", "Bo").await.unwrap());

    assert!(store.group_invite(&bob, g1, false).await.unwrap().is_none());
    let token = store
        .group_invite(&alice, g1, false)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        store.group_invite(&alice, g1, false).await.unwrap(),
        Some(token.clone())
    );
    assert!(store.group(&bob, g1).await.unwrap().is_none());
    assert_eq!(
        store.join_group(&bob, &token, "Bo").await.unwrap(),
        Join::Joined(g1)
    );
    assert_eq!(
        store
            .join_group(&UserKey::for_test("user"), &token, "bo")
            .await
            .unwrap(),
        Join::NameTaken
    );
    let group = store.group(&bob, g1).await.unwrap().unwrap();
    let names: Vec<_> = group.members.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(names, ["Ali", "Bo"]);
    assert!(group.members[0].creator && !group.members[1].creator);

    let fresh = store.group_invite(&bob, g1, true).await.unwrap().unwrap();
    assert_ne!(fresh, token);
    assert!(store.group_by_invite(&token).await.unwrap().is_none());
    assert_eq!(
        store.join_group(&alice, &token, "x").await.unwrap(),
        Join::UnknownInvite
    );
}

#[tokio::test]
async fn messages_post_once_and_only_from_members() {
    let db = TestDb::new().await.unwrap();
    let store = db.store().await.unwrap();
    let g1 = test_chat("g1");
    let (alice, bob) = (UserKey::for_test("alice"), UserKey::for_test("bob"));
    store.create_group(&alice, g1, "Trip", "Ali").await.unwrap();
    assert!(
        store
            .post_to_group(&bob, post(g1, "r0", "let me in"))
            .await
            .unwrap()
            .is_none()
    );
    let first = store
        .post_to_group(&alice, post(g1, "r1", "hi"))
        .await
        .unwrap()
        .unwrap();
    let again = store
        .post_to_group(&alice, post(g1, "r1", "hi"))
        .await
        .unwrap()
        .unwrap();
    assert!(first.new && !again.new);
    assert_eq!(first.message.id, again.message.id);
    store.post_beep(g1, "hello Ali").await.unwrap();
    let messages = store.group_messages(g1, 10).await.unwrap();
    let texts: Vec<_> = messages
        .iter()
        .map(|m| (m.author_name.as_str(), m.content.as_str()))
        .collect();
    assert_eq!(texts, [("Ali", "hi"), ("Beep", "hello Ali")]);
    assert_eq!(messages[1].author_id, None);
    assert_eq!(
        store.group_messages(g1, 1).await.unwrap()[0].content,
        "hello Ali"
    );
}

#[tokio::test]
async fn the_last_member_out_deletes_the_group() {
    let db = TestDb::new().await.unwrap();
    let store = db.store().await.unwrap();
    let g1 = test_chat("g1");
    let (alice, bob) = (UserKey::for_test("alice"), UserKey::for_test("bob"));
    store.create_group(&alice, g1, "Trip", "Ali").await.unwrap();
    let token = store
        .group_invite(&alice, g1, false)
        .await
        .unwrap()
        .unwrap();
    store.join_group(&bob, &token, "Bo").await.unwrap();
    assert_eq!(store.groups(&bob).await.unwrap().len(), 1);
    assert!(store.pin_group(&bob, g1, true).await.unwrap());
    let members = store.group_members(g1).await.unwrap();
    let pinned: Vec<bool> = members.iter().map(|member| member.pinned).collect();
    assert_eq!(pinned, [false, true]);
    assert!(members.iter().all(|member| member.shares_apps));
    assert!(store.share_group_apps(&bob, g1, false).await.unwrap());
    let members = store.group_members(g1).await.unwrap();
    let shared: Vec<bool> = members.iter().map(|member| member.shares_apps).collect();
    assert_eq!(shared, [true, false]);
    assert!(
        !store
            .pin_group(&UserKey::for_test("carol"), g1, true)
            .await
            .unwrap()
    );
    assert!(store.leave_group(&alice, g1).await.unwrap());
    assert!(!store.leave_group(&alice, g1).await.unwrap());
    assert!(store.group_by_invite(&token).await.unwrap().is_some());
    assert!(store.leave_group(&bob, g1).await.unwrap());
    assert!(store.group_by_invite(&token).await.unwrap().is_none());
}
