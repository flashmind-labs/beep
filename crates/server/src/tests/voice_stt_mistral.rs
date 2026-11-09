use super::*;
use flashmind::llm::mistral::TranscriptionDone;
use futures::stream;
use std::sync::{
    Mutex,
    atomic::{AtomicBool, Ordering},
};

/// Records what `pump` sends.
#[derive(Clone, Default)]
struct Recorded {
    sent: Arc<Mutex<Vec<Bytes>>>,
    finished: Arc<AtomicBool>,
}

impl AudioOut for Recorded {
    async fn send(&self, pcm: Bytes) -> anyhow::Result<()> {
        self.sent.lock().expect("not poisoned in tests").push(pcm);
        Ok(())
    }

    async fn finish(self) -> anyhow::Result<()> {
        self.finished.store(true, Ordering::SeqCst);
        Ok(())
    }
}

#[tokio::test]
async fn audio_that_ends_before_any_came_is_done_with_no_text() {
    let (audio, chunks) = mpsc::channel(AUDIO_QUEUE);
    let (said, mut heard) = mpsc::channel(HEARD_QUEUE);
    let out = Recorded::default();
    // An empty chunk is no audio either.
    audio.send(Vec::new()).await.expect("pump is reading");
    drop(audio);
    // Mistral would never answer a session that got no audio.
    pump(chunks, out.clone(), stream::pending(), said).await;
    assert!(matches!(heard.recv().await, Some(Ok(Heard::Done(text))) if text.is_empty()));
    assert!(!out.finished.load(Ordering::SeqCst));
    assert!(out.sent.lock().expect("not poisoned in tests").is_empty());
}

/// A provider that stopped taking audio.
struct Refusing;

impl AudioOut for Refusing {
    async fn send(&self, _pcm: Bytes) -> anyhow::Result<()> {
        anyhow::bail!("the socket closed")
    }

    async fn finish(self) -> anyhow::Result<()> {
        Ok(())
    }
}

#[tokio::test(start_paused = true)]
async fn a_provider_that_stops_taking_audio_fails_in_time() {
    let (audio, chunks) = mpsc::channel(AUDIO_QUEUE);
    let (said, mut heard) = mpsc::channel(HEARD_QUEUE);
    audio.send(vec![0; 640]).await.expect("pump is reading");
    // The caller keeps the audio open, and the provider never answers.
    let pumped = pump(chunks, Refusing, stream::pending(), said);
    tokio::time::timeout(DONE_WAIT * 2, pumped)
        .await
        .expect("the pump gives up");
    assert!(matches!(heard.recv().await, Some(Err(SttError::Provider))));
    drop(audio);
}

#[tokio::test]
async fn audio_is_finished_and_its_text_comes_back() {
    let (audio, chunks) = mpsc::channel(AUDIO_QUEUE);
    let (said, mut heard) = mpsc::channel(HEARD_QUEUE);
    let out = Recorded::default();
    audio.send(vec![0; 640]).await.expect("pump is reading");
    drop(audio);
    let done = TranscriptionEvent::Done(TranscriptionDone {
        text: "hello".into(),
        language: None,
    });
    // The text arrives only after the audio was finished.
    let finished = out.finished.clone();
    let events = stream::once(async move {
        while !finished.load(Ordering::SeqCst) {
            tokio::task::yield_now().await;
        }

        Ok(done)
    })
    .chain(stream::pending());
    pump(chunks, out.clone(), Box::pin(events), said).await;
    assert!(matches!(heard.recv().await, Some(Ok(Heard::Done(text))) if text == "hello"));
    assert_eq!(out.sent.lock().expect("not poisoned in tests").len(), 1);
}
