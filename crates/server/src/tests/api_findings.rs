use super::*;
use crate::{
    memory::watchers::{
        Cause, FindingKind,
        testing::{Harness, Scripted},
    },
    store::{NewFinding, Sighting, test_chat},
};

#[tokio::test]
async fn every_user_reads_and_settles_only_their_own_findings() {
    let h = Harness::new(Scripted::default()).await;
    let chat = test_chat("finding");
    let last = h.turn(&h.alice, chat, "no, I meant tomorrow").await;
    let new = NewFinding {
        fingerprint: "correction:system-prompt:relative-dates",
        kind: FindingKind::Correction,
        summary: "Booked today instead of tomorrow.",
        steps: "1. Ask to book something tomorrow.",
        cause: Cause::Prompt,
        cause_detail: "Relative dates are not resolved.",
        fix: "Resolve relative dates first.",
    };
    let seen = Sighting {
        user: "alice",
        chat,
        first_seq: last - 1,
        last_seq: last,
    };
    let id = h
        .app
        .store
        .record_finding(&new, &seen, Utc::now())
        .await
        .unwrap();

    // Bob gets findings too, only never Alice's.
    assert!(list(&h.app, &h.bob, None).await.unwrap().is_empty());
    assert_eq!(
        update(&h.app, &h.bob, &id.to_string(), FindingStatus::Dismissed {})
            .await
            .unwrap_err(),
        ApiError::NotFound {}
    );
    let bobs_chat = test_chat("bobs-finding");
    let bobs_last = h.turn(&h.bob, bobs_chat, "no, I meant tomorrow").await;
    let bobs_sighting = Sighting {
        user: "bob",
        chat: bobs_chat,
        first_seq: bobs_last - 1,
        last_seq: bobs_last,
    };
    let bobs = h
        .app
        .store
        .record_finding(&new, &bobs_sighting, Utc::now())
        .await
        .unwrap();
    let his = list(&h.app, &h.bob, None).await.unwrap();
    assert_eq!(his.len(), 1);
    assert_eq!(his[0].id, bobs.to_string());
    assert_eq!(his[0].session_id, Some(bobs_chat));

    let listed = list(&h.app, &h.alice, Some(FindingStatus::Open {}))
        .await
        .unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].kind, "correction");
    assert_eq!(listed[0].cause, "prompt");
    assert_eq!(listed[0].session_id, Some(chat));
    assert_eq!(
        (listed[0].first_message, listed[0].last_message),
        (last - 1, last)
    );

    let resolved = update(
        &h.app,
        &h.alice,
        &id.to_string(),
        FindingStatus::Resolved {},
    )
    .await
    .unwrap();
    assert_eq!(resolved.status, FindingStatus::Resolved {});
    assert!(
        list(&h.app, &h.alice, Some(FindingStatus::Open {}))
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        update(&h.app, &h.alice, "not-an-id", FindingStatus::Open {})
            .await
            .unwrap_err(),
        ApiError::NotFound {}
    );
}
