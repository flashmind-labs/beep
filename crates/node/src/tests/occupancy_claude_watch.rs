use super::*;
use crate::occupancy::claude::hooks::{HookEvent, send};
use beep_proto::node::agent::ActivityKind;

const ID: &str = "0f3c2a9e-1111-4222-8333-944455556666";

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
