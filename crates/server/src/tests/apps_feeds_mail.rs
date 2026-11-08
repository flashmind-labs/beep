use super::*;

#[test]
fn parses_gmail_page() {
    let data = json!({
        "messages": [
            {
                "messageId": "m1",
                "messageTimestamp": "2026-03-01T10:00:00Z",
                "sender": "Sam <sam@example.com>",
                "to": "me@example.com",
                "subject": "Dinner",
                "messageText": "See you Friday",
                "labelIds": ["INBOX"]
            },
            {
                "messageId": "m2",
                "messageTimestamp": "2026-02-01T10:00:00Z",
                "subject": "Re: trip",
                "messageText": "Booked",
                "labelIds": ["SENT"]
            },
            {"messageId": "m3"}
        ],
        "nextPageToken": "next"
    });
    let page = parse_gmail(&data).unwrap();
    assert_eq!(page.emails.len(), 2);
    assert!(!page.emails[0].sent && page.emails[1].sent);
    assert_eq!(page.emails[0].body, "See you Friday");
    assert_eq!(page.next.as_deref(), Some("next"));
}

#[test]
fn parses_outlook_page() {
    let data = json!({
        "value": [
            {
                "id": "o1",
                "receivedDateTime": "2026-03-01T10:00:00Z",
                "from": {"emailAddress": {"name": "Sam", "address": "sam@example.com"}},
                "toRecipients": [
                    {"emailAddress": {"name": "me@example.com", "address": "me@example.com"}},
                    {"emailAddress": {"name": "Alex", "address": "alex@example.com"}}
                ],
                "subject": "Dinner",
                "body": {"contentType": "html", "content": "<html><body><p>See you <b>Friday</b></p></body></html>"},
                "parentFolderId": "inbox-id",
                "isDraft": false
            },
            {
                "id": "o2",
                "receivedDateTime": "2026-03-02T10:00:00Z",
                "subject": "Re: trip",
                "body": {"contentType": "text", "content": "Booked"},
                "parentFolderId": "sent-id"
            },
            {"id": "o3", "receivedDateTime": "2026-03-03T10:00:00Z", "isDraft": true},
            {"id": "o4"}
        ],
        "next_page_token": "next"
    });
    let page = parse_outlook(&data, Some("sent-id")).unwrap();
    assert_eq!(page.emails.len(), 2);
    let (first, second) = (&page.emails[0], &page.emails[1]);
    assert_eq!(first.from, "Sam <sam@example.com>");
    assert_eq!(first.to, "me@example.com, Alex <alex@example.com>");
    assert_eq!(first.body.trim(), "See you Friday");
    assert!(!first.sent && second.sent);
    assert_eq!(second.body, "Booked");
    assert_eq!(page.next.as_deref(), Some("next"));
}
