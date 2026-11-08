use super::*;
use crate::{
    account::user::UserKey,
    store::{TestDb, test_chat, test_turn},
};
use chrono::TimeDelta;

fn new(fingerprint: &str) -> NewFinding<'_> {
    NewFinding {
        fingerprint,
        kind: FindingKind::WrongTool,
        summary: "Searched mail for a calendar event.",
        steps: "Ask when the next meeting is.",
        cause: Cause::Prompt,
        cause_detail: "The prompt does not say calendars hold meetings.",
        fix: "Say so in the prompt.",
    }
}

#[tokio::test]
async fn the_same_problem_in_a_users_chats_is_one_finding() {
    let db = TestDb::new().await.unwrap();
    let store = db.store().await.unwrap();
    let (ours, elsewhere) = (test_chat("ours"), test_chat("elsewhere"));
    test_turn(&store, &UserKey::for_test("alice"), ours, "when", 1_000)
        .await
        .unwrap();
    test_turn(
        &store,
        &UserKey::for_test("alice"),
        elsewhere,
        "when",
        1_500,
    )
    .await
    .unwrap();
    let start = DateTime::UNIX_EPOCH + TimeDelta::days(1);
    let seen = |user, chat, first_seq| Sighting {
        user,
        chat,
        first_seq,
        last_seq: first_seq + 1,
    };
    let fingerprint = "wrong_tool:gmail:calendar-in-mail";
    let id = store
        .record_finding(&new(fingerprint), &seen("alice", ours, 1), start)
        .await
        .unwrap();
    let later = start + TimeDelta::hours(1);
    let again = store
        .record_finding(&new(fingerprint), &seen("alice", elsewhere, 7), later)
        .await
        .unwrap();
    assert_eq!(id, again);
    let other = store
        .record_finding(
            &new("loop:web_search:same-query"),
            &seen("alice", ours, 9),
            start,
        )
        .await
        .unwrap();
    assert_ne!(id, other);

    let found = store.findings("alice", None, 10).await.unwrap();
    assert_eq!(found.len(), 2);
    assert_eq!(found[0].id, id, "most recently seen first");
    assert_eq!(found[0].fingerprint, fingerprint);
    assert_eq!(found[0].seen, 2);
    assert_eq!((found[0].first_seen, found[0].last_seen), (start, later));
    assert_eq!(
        found[0].sighting,
        Some(SeenIn {
            chat: elsewhere,
            first_seq: 7,
            last_seq: 8
        })
    );

    let dismissed = store
        .set_finding_status("alice", other, FindingStatus::Dismissed, later)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(dismissed.status, FindingStatus::Dismissed);
    let open = store
        .findings("alice", Some(FindingStatus::Open), 10)
        .await
        .unwrap();
    assert_eq!(open.iter().map(|f| f.id).collect::<Vec<_>>(), [id]);
    assert!(
        store
            .set_finding_status("alice", Uuid::new_v4(), FindingStatus::Resolved, later)
            .await
            .unwrap()
            .is_none()
    );

    // A resolved problem that comes back opens again; a dismissed one does not.
    store
        .set_finding_status("alice", id, FindingStatus::Resolved, later)
        .await
        .unwrap();
    store
        .record_finding(&new(fingerprint), &seen("alice", ours, 20), later)
        .await
        .unwrap();
    store
        .record_finding(
            &new("loop:web_search:same-query"),
            &seen("alice", ours, 30),
            later,
        )
        .await
        .unwrap();
    let all = store.findings("alice", None, 10).await.unwrap();
    let status = |id| all.iter().find(|f| f.id == id).map(|f| (f.status, f.seen));
    assert_eq!(status(id), Some((FindingStatus::Open, 3)));
    assert_eq!(status(other), Some((FindingStatus::Dismissed, 2)));
}

#[tokio::test]
async fn each_user_reads_and_settles_only_their_own_findings() {
    let db = TestDb::new().await.unwrap();
    let store = db.store().await.unwrap();
    let (ours, theirs) = (test_chat("ours"), test_chat("theirs"));
    test_turn(&store, &UserKey::for_test("alice"), ours, "when", 1_000)
        .await
        .unwrap();
    test_turn(&store, &UserKey::for_test("bob"), theirs, "when", 2_000)
        .await
        .unwrap();
    let at = DateTime::UNIX_EPOCH + TimeDelta::days(1);
    let fingerprint = "wrong_tool:gmail:calendar-in-mail";
    let seen = |user, chat| Sighting {
        user,
        chat,
        first_seq: 1,
        last_seq: 2,
    };
    let alices = store
        .record_finding(&new(fingerprint), &seen("alice", ours), at)
        .await
        .unwrap();
    // The same problem in Bob's chat is Bob's own finding.
    let bobs = store
        .record_finding(&new(fingerprint), &seen("bob", theirs), at)
        .await
        .unwrap();
    assert_ne!(alices, bobs);

    let found = store.findings("bob", None, 10).await.unwrap();
    assert_eq!(found.iter().map(|f| f.id).collect::<Vec<_>>(), [bobs]);
    assert_eq!(found[0].seen, 1);
    assert_eq!(found[0].fingerprint, fingerprint);
    assert_eq!(found[0].sighting.as_ref().map(|s| s.chat), Some(theirs));
    assert!(store.findings("carol", None, 10).await.unwrap().is_empty());

    // Bob cannot settle Alice's.
    assert!(
        store
            .set_finding_status("bob", alices, FindingStatus::Dismissed, at)
            .await
            .unwrap()
            .is_none()
    );
    let hers = store.findings("alice", None, 10).await.unwrap();
    assert_eq!(hers.iter().map(|f| f.id).collect::<Vec<_>>(), [alices]);
    assert_eq!(hers[0].status, FindingStatus::Open);
}
