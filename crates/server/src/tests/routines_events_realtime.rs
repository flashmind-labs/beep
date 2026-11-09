use super::*;

#[test]
fn chunked_events_come_out_whole_in_any_order() {
    let payload = json!({"metadata": {"trigger_slug": "X", "user_id": "u"}, "data": {"a": 1}});
    let text = payload.to_string();
    let (head, tail) = text.split_at(text.len() / 2);
    let mut chunks = Chunks::default();
    assert!(
        chunks
            .add(&json!({"id": "e1", "index": 1, "chunk": tail, "final": true}))
            .is_none()
    );
    let whole = chunks
        .add(&json!({"id": "e1", "index": 0, "chunk": head, "final": false}))
        .unwrap();
    assert_eq!(whole, payload);
    assert!(chunks.open.is_empty());
}

#[test]
fn chunked_events_are_bounded() {
    let mut chunks = Chunks::default();

    for n in 0..MAX_CHUNKED {
        let piece = json!({"id": format!("e{n}"), "index": 0, "chunk": "{", "final": false});
        assert!(chunks.add(&piece).is_none());
    }

    let extra = json!({"id": "extra", "index": 0, "chunk": "{}", "final": true});
    assert!(chunks.add(&extra).is_none());
    assert!(!chunks.open.contains_key("extra"));
}

#[test]
fn the_socket_url_takes_only_plain_credentials() {
    let credentials = |cluster: &str| Credentials {
        pusher_key: "abc123".into(),
        project_id: "p".into(),
        pusher_cluster: cluster.into(),
    };
    assert_eq!(
        pusher_url(&credentials("mt1")).unwrap(),
        "wss://ws-mt1.pusher.com/app/abc123?protocol=7&client=beep-server&version=1.0&flash=false"
    );
    assert!(pusher_url(&credentials("evil.example.com/x")).is_err());
}

#[test]
fn the_subscribe_carries_the_signed_channel_data() {
    let auth: ChannelAuth =
        serde_json::from_str(r#"{"auth":"key:sig","channel_data":"{\"user_id\":\"p1\"}"}"#)
            .expect("Composio's auth reply");
    let frame = subscribe_frame("private-p1_triggers", auth);
    assert_eq!(frame["event"], "pusher:subscribe");
    assert_eq!(frame["data"]["auth"], "key:sig");
    assert_eq!(frame["data"]["channel_data"], r#"{"user_id":"p1"}"#);

    let auth: ChannelAuth =
        serde_json::from_str(r#"{"auth":"key:sig"}"#).expect("a plain private auth");
    let frame = subscribe_frame("private-p1_triggers", auth);
    assert!(frame["data"].get("channel_data").is_none());
}
