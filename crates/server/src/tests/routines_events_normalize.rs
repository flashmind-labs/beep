use super::*;

fn gmail_payload(id: &str, labels: &[&str]) -> Value {
    json!({
        "id": "log_1",
        "timestamp": "2026-10-20T10:00:00Z",
        "type": "composio.trigger.message",
        "metadata": {
            "log_id": "log_1",
            "trigger_slug": "GMAIL_NEW_GMAIL_MESSAGE",
            "trigger_id": "ti_1",
            "connected_account_id": "ca_1",
            "auth_config_id": "ac_1",
            "user_id": "outside-1"
        },
        "data": {
            "message_id": id,
            "thread_id": "t1",
            "sender": "Ana <ana@example.com>",
            "subject": "Lease renewal",
            "preview": {"body": "Can you sign\n by Friday?"},
            "label_ids": labels,
            "message_timestamp": "2026-10-20T09:59:00Z"
        }
    })
}

#[test]
fn reads_every_composio_envelope() {
    let v3 = TriggerMessage::try_from(&gmail_payload("m1", &["INBOX"])).unwrap();
    assert_eq!(v3.user, "outside-1");
    assert_eq!(v3.slug, GMAIL_NEW_MESSAGE);
    assert_eq!(v3.data["message_id"], "m1");

    let v2 = TriggerMessage::try_from(&json!({
        "type": "gmail_new_gmail_message",
        "timestamp": "2026-10-20T10:00:00Z",
        "log_id": "log_2",
        "data": {"user_id": "outside-2", "message_id": "m2"}
    }))
    .unwrap();
    assert_eq!(v2.user, "outside-2");
    assert_eq!(v2.slug, GMAIL_NEW_MESSAGE);

    let legacy = TriggerMessage::try_from(&json!({
        "appName": "gmail",
        "payload": {"message_id": "m3"},
        "metadata": {
            "nanoId": "ti_3",
            "triggerName": "GMAIL_NEW_GMAIL_MESSAGE",
            "connection": {"clientUniqueUserId": "outside-3"}
        }
    }))
    .unwrap();
    assert_eq!(legacy.user, "outside-3");
    assert_eq!(legacy.data["message_id"], "m3");

    assert!(TriggerMessage::try_from(&json!({"hello": "world"})).is_err());
}

#[test]
fn a_gmail_trigger_and_a_poll_of_the_same_email_share_a_key() {
    let message = TriggerMessage::try_from(&gmail_payload("m1", &["INBOX"])).unwrap();
    let triggered = AppEvent::try_from(&message).unwrap();
    assert_eq!(triggered.kind, EventKind::NewMail);
    assert_eq!(
        triggered.summary,
        "From Ana <ana@example.com>: Lease renewal"
    );
    assert_eq!(triggered.detail.as_deref(), Some("Can you sign by Friday?"));
    assert_eq!(
        triggered.ids,
        json!({"message_id": "m1", "thread_id": "t1"})
    );
    assert_eq!(triggered.occurred.to_rfc3339(), "2026-10-20T09:59:00+00:00");

    let email = Email {
        id: "m1".into(),
        received: Utc::now(),
        from: "Ana <ana@example.com>".into(),
        to: "user@host".into(),
        subject: "Lease renewal".into(),
        sent: false,
        bulk: false,
        thread: Some("t1".into()),
        body: "Can you sign by Friday?".into(),
    };
    let polled = from_email(Mailbox::Gmail, &email).unwrap();
    assert_eq!(polled.key, triggered.key);
    assert_eq!(polled.source, Source::Poll);
    assert!(
        from_email(
            Mailbox::Gmail,
            &Email {
                sent: true,
                ..email.clone()
            }
        )
        .is_none()
    );
    assert!(
        from_email(
            Mailbox::Gmail,
            &Email {
                bulk: true,
                ..email
            }
        )
        .is_none()
    );
}

#[test]
fn outlook_mail_reads_both_field_styles_and_skips_other() {
    let message = |data: Value| TriggerMessage {
        user: "outside-1".into(),
        slug: OUTLOOK_NEW_MESSAGE.into(),
        data,
        at: Utc::now(),
    };
    let snake = AppEvent::try_from(&message(json!({
            "event_type": "created",
            "id": "AAMk1",
            "outlook_message": {
                "subject": "Invoice",
                "from_address": {"email_address": {"name": "Billing", "address": "billing@example.com"}},
                "body_preview": "Due soon",
                "conversation_id": "c1",
                "inference_classification": "focused"
            }
        })))
        .unwrap();
    assert_eq!(snake.summary, "From Billing: Invoice");
    assert_eq!(snake.ids["thread_id"], "c1");
    assert_eq!(snake.key.as_ref(), "outlook:AAMk1");

    let camel = AppEvent::try_from(&message(json!({
        "id": "AAMk2",
        "message": {"subject": "Hi", "from": {"emailAddress": {"address": "a@example.com"}}}
    })))
    .unwrap();
    assert_eq!(camel.summary, "From a@example.com: Hi");

    assert!(
        AppEvent::try_from(&message(json!({
            "id": "AAMk3",
            "outlook_message": {"subject": "Sale", "inference_classification": "other"}
        })))
        .is_err()
    );
}

fn notification(reason: &str, kind: &str, title: &str, updated: &str) -> GitHubNotification {
    serde_json::from_value(json!({
        "id": "123",
        "reason": reason,
        "unread": true,
        "updated_at": updated,
        "repository": {"full_name": "org/repo"},
        "subject": {
            "title": title,
            "type": kind,
            "url": "https://api.github.com/repos/org/repo/pulls/42"
        }
    }))
    .expect("a GitHub notification")
}

#[test]
fn github_notifications_are_sorted_by_reason() {
    let review = AppEvent::try_from(&notification(
        "review_requested",
        "PullRequest",
        "Add caching",
        "2026-10-20T10:00:00Z",
    ))
    .unwrap();
    assert_eq!(review.kind, EventKind::ReviewRequested);
    assert_eq!(
        review.summary,
        "org/repo: review requested on PR #42 \"Add caching\""
    );
    assert_eq!(review.ids["number"], 42);
    assert_eq!(review.ids["url"], "https://github.com/org/repo/pull/42");

    let cases = [
        ("mention", EventKind::Mention),
        ("team_mention", EventKind::Mention),
        ("assign", EventKind::Assigned),
        ("author", EventKind::ThreadActivity),
        ("subscribed", EventKind::PullRequest),
        ("state_change", EventKind::GithubOther),
    ];
    for (reason, kind) in cases {
        let event = AppEvent::try_from(&notification(
            reason,
            "PullRequest",
            "t",
            "2026-10-20T10:00:00Z",
        ))
        .unwrap();
        assert_eq!(event.kind, kind, "{reason}");
    }

    // New activity on the same thread is a new event.
    let later = AppEvent::try_from(&notification(
        "review_requested",
        "PullRequest",
        "Add caching",
        "2026-10-20T11:00:00Z",
    ))
    .unwrap();
    assert_ne!(later.key, review.key);

    let failed = AppEvent::try_from(&notification(
        "ci_activity",
        "CheckSuite",
        "CI workflow run failed for main branch",
        "2026-10-20T10:00:00Z",
    ))
    .unwrap();
    assert_eq!(failed.kind, EventKind::CiFailed);
    assert!(
        AppEvent::try_from(&notification(
            "ci_activity",
            "CheckSuite",
            "CI workflow run succeeded for main branch",
            "2026-10-20T10:00:00Z",
        ))
        .is_err()
    );
}
