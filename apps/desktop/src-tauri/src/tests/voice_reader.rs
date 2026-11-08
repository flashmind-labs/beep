use super::*;
use tokio::sync::oneshot;

/// A reader that says a line once `go` fires, and the receiver that
/// hears it, or an error when the reader was stopped first.
fn gated_reader(reader: &mut Reader) -> (oneshot::Sender<()>, oneshot::Receiver<()>) {
    let (go, gate) = oneshot::channel();
    let (said, heard) = oneshot::channel();
    reader.start(async move {
        if gate.await.is_ok() {
            let _ = said.send(());
        }
    });
    (go, heard)
}

#[tokio::test]
async fn a_hush_right_after_a_reader_starts_stops_it() {
    let mut reader = Reader::default();
    let (go, heard) = gated_reader(&mut reader);
    // The hush comes before the task has had a chance to run.
    reader.stop();
    // The gate may already be gone with the aborted task.
    let _ = go.send(());
    assert!(heard.await.is_err(), "a hushed reader still spoke");
}

#[tokio::test]
async fn a_new_reader_stops_the_one_before() {
    let mut reader = Reader::default();
    let (first_go, first) = gated_reader(&mut reader);
    let (second_go, second) = gated_reader(&mut reader);
    let _ = first_go.send(());
    assert!(first.await.is_err(), "the replaced reader still spoke");
    second_go
        .send(())
        .expect("the running reader waits on its gate");
    assert!(second.await.is_ok());
}
