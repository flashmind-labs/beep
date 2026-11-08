use super::*;
use crate::routines::events::EventKind;
use chrono::TimeZone;
use serde_json::json;

fn row(id: i64, app: EventApp, summary: &str) -> EventRow {
    EventRow {
        id,
        app,
        kind: EventKind::NewMail,
        summary: summary.to_owned(),
        detail: Some("Can you sign by Friday?".to_owned()),
        ids: json!({"message_id": "m1"}),
        occurred: Utc.with_ymd_and_hms(2026, 10, 20, 9, 30, 0).unwrap(),
        state: EventState::Pending,
    }
}

#[test]
fn the_message_lists_each_event_with_its_id_and_ids() {
    let rows = [row(7, EventApp::Gmail, "From Ana: Lease renewal")];
    let text = describe(&rows, Rule::Accounts, &[EventApp::Gmail]);
    assert_eq!(
        text,
        "New events from the user's connected accounts (1):\n\n\
             [#7] Gmail, new mail, 2026-10-20 09:30 UTC: From Ana: Lease renewal\n\
             Can you sign by Friday?\n\
             ids: {\"message_id\":\"m1\"}\n\n\
             The user set Gmail to read only: offer nothing that would act there."
    );
}

#[test]
fn only_batches_that_waited_are_due() {
    let hub = Hub::default();
    let start = Instant::now();
    hub.mark_pending("alice", start);
    hub.mark_pending("bob", start + Duration::from_secs(50));
    // A later event does not push a waiting batch back.
    hub.mark_pending("alice", start + Duration::from_secs(80));
    let due = hub.due(start + Duration::from_secs(90), Duration::from_secs(90));
    assert_eq!(due, ["alice"]);
    assert!(
        hub.due(start + Duration::from_secs(91), Duration::from_secs(90))
            .is_empty()
    );
    assert_eq!(
        hub.due(start + Duration::from_secs(140), Duration::from_secs(90)),
        ["bob"]
    );
}
