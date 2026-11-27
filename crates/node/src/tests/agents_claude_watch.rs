use super::*;
use std::{io::Write, path::Path};
use tokio::time::timeout;

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

#[tokio::test(start_paused = true)]
async fn a_transcript_is_read_when_the_stores_ring_not_on_a_timer() {
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("projects/p");
    fs::create_dir_all(&project).unwrap();
    let path = project.join(format!("{ID}.jsonl"));
    fs::write(&path, "{\"type\":\"permission-mode\"}\n").unwrap();
    let occupancy = Occupancy::over(crate::logs::claude::Store::at(temp.path()), Default::default());
    let events = crate::events::Hub::default();
    let rings = events.subscribe();
        let link = beep_proto::node::local_link();
    let (node_tx, node_rx) = link.node;
    let (_tx, mut rx) = link.caller;
    let serving = tokio::spawn(async move {
        serve(node_tx, node_rx, &occupancy, vec![ID.into()], rings).await
    });
    let SessionsReply::Watching { unknown } = rx.recv().await.unwrap().unwrap() else {
        panic!("expected the watch to start");
    };
    assert!(unknown.is_empty());

    append(
        &path,
        "{\"type\":\"assistant\",\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"Done.\"}]}}\n\
         {\"type\":\"system\",\"subtype\":\"turn_duration\"}\n",
    );
    let quiet = timeout(HOOK_GRACE * 4, rx.recv::<SessionsReply>()).await;
    assert!(quiet.is_err(), "nothing rang, nothing was read");

    events.changed(beep_proto::node::events::Topic::new("agent"));
    let SessionsReply::Activity { activity } = rx.recv().await.unwrap().unwrap() else {
        panic!("expected the turn end");
    };
    assert_eq!(activity.kind, ActivityKind::TurnDone);
    assert_eq!(activity.message, "Done.");
    serving.abort();
}
