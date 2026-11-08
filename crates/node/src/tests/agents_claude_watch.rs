use super::*;
use crate::agents::claude::hooks::{HookEvent, send};
use std::io::Write;

const ID: &str = "0f3c2a9e-1111-4222-8333-944455556666";

fn append(path: &Path, text: &str) {
    let mut file = fs::OpenOptions::new().append(true).open(path).unwrap();
    file.write_all(text.as_bytes()).unwrap();
}

#[test]
fn a_turn_end_in_the_transcript_is_sent_one_check_later() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();
    let path = dir.join(format!("{ID}.jsonl"));
    fs::write(&path, "{\"type\":\"permission-mode\"}\n").unwrap();
    let mut tracker = Tracker::new(ID.into(), path.clone());
    assert!(tracker.poll().is_empty());
    append(
        &path,
        "{\"type\":\"assistant\",\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"Done.\"}]}}\n\
         {\"type\":\"system\",\"subtype\":\"turn_duration\"}\n{\"type\":\"assis",
    );
    assert!(tracker.poll().is_empty(), "held for one check");
    let due = tracker.poll();
    assert_eq!(due.len(), 1);
    assert_eq!(due[0].kind, ActivityKind::TurnDone);
    assert_eq!(due[0].message, "Done.");
    assert!(!due[0].from_hook);
    // The half-written line is read once it ends; no turn end in it.
    append(&path, "tant\",\"message\":{\"content\":[]}}\n");
    assert!(tracker.poll().is_empty());
    assert!(tracker.poll().is_empty());

    // Once a hook spoke, the transcript is left alone.
    append(
        &path,
        "{\"type\":\"system\",\"subtype\":\"turn_duration\"}\n",
    );
    tracker.poll();
    tracker.hook();
    assert!(tracker.poll().is_empty());
}

#[tokio::test]
async fn a_hook_reaches_the_hub_over_the_socket() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();
    let socket = dir.join("hooks.sock");
    let hub = Hub::default();
    let mut events = hub.0.subscribe();
    let listening = {
        let socket = socket.clone();
        async move { listen(&socket, &hub, &Live::default()).await }
    };
    let sent = async {
        // The listener binds before its first await, so a failed
        // connect only means it has not run yet.
        for _ in 0..50 {
            let message = HookMessage {
                event: HookEvent::Notification,
                session_id: ID.into(),
                message: Some("Claude needs your permission to use Bash".into()),
                notification_type: Some("permission_prompt".into()),
                tool: None,
                detail: None,
                file: None,
                todos: None,
                error: None,
            };
            if send(&socket, &message).await.is_ok() {
                return events.recv().await.unwrap();
            }
            tokio::task::yield_now().await;
        }
        panic!("the socket never opened");
    };
    let activity = tokio::select! {
        activity = sent => activity,
        result = listening => panic!("the listener stopped: {result:?}"),
    };
    assert_eq!(activity.session, ID);
    assert_eq!(activity.kind, ActivityKind::NeedsInput);
    assert!(activity.from_hook);
}
