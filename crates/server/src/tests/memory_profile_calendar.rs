use super::*;

#[test]
fn reads_google_outlook_and_jmap_events() {
    let google = json!({"summary": "Gym", "start": {"dateTime": "2025-03-04T18:00:00+01:00"},
            "location": "Basic-Fit", "attendees": [{"email": "me@x.com", "self": true}, {"displayName": "Ana"}]});
    let outlook = json!({"subject": "Standup", "start": {"dateTime": "2025-03-05T09:00:00.0000000"},
            "location": {"displayName": "Office"}, "attendees": [{"emailAddress": {"name": "Bo", "address": "bo@x.com"}}]});
    let jmap = json!({"title": "Dinner", "start": "2025-03-06T20:00:00",
            "locations": {"1": {"name": "Casa"}}, "participants": {"p": {"name": "Cy"}}});
    let cancelled =
        json!({"summary": "Gone", "status": "cancelled", "start": {"date": "2025-03-07"}});
    let events: Vec<Event> = [google, outlook, jmap, cancelled]
        .iter()
        .filter_map(parse_event)
        .collect();
    assert_eq!(events.len(), 3);
    assert_eq!(events[0].people, ["Ana"]);
    assert_eq!(events[1].location, "Office");
    assert_eq!(events[2].day, NaiveDate::from_ymd_opt(2025, 3, 6).unwrap());
    assert_eq!(events[2].location, "Casa");
}
