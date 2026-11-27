use super::*;
use crate::agents::Parts;
use std::time::Duration;
use tokio::time::timeout;

#[tokio::test]
async fn an_update_can_wait_for_idle() {
    let dir = tempfile::tempdir().expect("a temporary folder");
    let agents = Arc::new(Agents::new(Parts::default()));
    let listening = {
        let (dir, reopen) = (dir.path().to_path_buf(), agents.reopen.clone());
        tokio_util::task::AbortOnDropHandle::new(tokio::spawn(
            async move { listen(&dir, reopen).await },
        ))
    };
    // Bound before the update asks: it waits for the socket.
    let drained = loop {
        match drain(dir.path(), Duration::from_secs(20)).await {
            Drained::NoNode if !listening.is_finished() => tokio::task::yield_now().await,
            other => break other,
        }
    };
    let Drained::Idle(held) = drained else {
        panic!("a node without sessions is idle at once");
    };
    drop(held);
}

#[tokio::test]
async fn a_host_marks_sessions_reopened_and_notes_what_is_open() {
    let dir = tempfile::tempdir().expect("a temporary folder");
    let agents = Arc::new(Agents::new(Parts::default()));
    let keeping = {
        let (dir, agents) = (dir.path().to_path_buf(), agents.clone());
        tokio_util::task::AbortOnDropHandle::new(tokio::spawn(async move {
            reopen_and_keep(&dir, &agents).await;
        }))
    };
    // No note and no settings: moves stop waiting at once.
    timeout(Duration::from_secs(20), agents.reopen.until_reopened())
        .await
        .expect("reopened without waiting for settings");
    keeping.abort();
    let _ = keeping.await;
    std::fs::remove_file(dir.path().join(FILE)).ok();
    note_on_quit(dir.path(), &agents).expect("the note is written");
    let note = Reopen::read(dir.path());
    assert!(dir.path().join(FILE).exists());
    assert!(note.sessions.is_empty() && note.terminals.is_empty());
}
