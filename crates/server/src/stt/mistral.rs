//! Live transcription through Mistral's realtime API, by way of flashmind's
//! Mistral provider.

use super::{AUDIO_QUEUE, HEARD_QUEUE, Heard, RealtimeStt, Session, SttError};
use bytes::Bytes;
use flashmind::llm::{
    MistralProvider,
    mistral::{AudioSink, RealtimeTranscriptionRequest, TranscriptionEvent},
};
use futures::{Stream, StreamExt, future::BoxFuture};
use std::{future::Future, sync::Arc, time::Duration};
use tokio::{
    sync::mpsc::{self, Receiver, Sender},
    time::{Instant, sleep, timeout},
};

/// How long Mistral may take to open a session.
const OPEN_TIMEOUT: Duration = Duration::from_secs(10);
/// How long Mistral gets to send the whole text once the audio ended. The
/// provider has no watchdog of its own after `finish`.
const DONE_WAIT: Duration = Duration::from_secs(10);

pub struct MistralStt {
    provider: Arc<MistralProvider>,
    model: String,
}

impl MistralStt {
    pub fn new(provider: Arc<MistralProvider>, model: String) -> Self {
        Self { provider, model }
    }
}

impl RealtimeStt for MistralStt {
    fn open(&self) -> BoxFuture<'_, Result<Session, SttError>> {
        Box::pin(async move {
            let request = RealtimeTranscriptionRequest::new(self.model.clone());
            let opened = timeout(OPEN_TIMEOUT, self.provider.realtime_transcription(request)).await;
            let session = match opened {
                Ok(Ok(session)) => session,
                Ok(Err(error)) => {
                    log::warn!("live transcription did not open: {error:#}");
                    return Err(SttError::Provider);
                }
                Err(_) => {
                    log::warn!("live transcription took too long to open");
                    return Err(SttError::Provider);
                }
            };
            let (sink, events) = session.split();
            let (audio, chunks) = mpsc::channel(AUDIO_QUEUE);
            let (said, heard) = mpsc::channel(HEARD_QUEUE);
            // Owned by the session: it ends when the text is done, the
            // provider fails, or the caller drops `heard`.
            tokio::spawn(pump(chunks, sink, events, said));
            Ok(Session { audio, heard })
        })
    }
}

/// Where `pump` sends the audio: Mistral's `AudioSink`, or a fake in tests.
trait AudioOut {
    fn send(&self, pcm: Bytes) -> impl Future<Output = anyhow::Result<()>> + Send;
    fn finish(self) -> impl Future<Output = anyhow::Result<()>> + Send;
}

impl AudioOut for AudioSink {
    fn send(&self, pcm: Bytes) -> impl Future<Output = anyhow::Result<()>> + Send {
        AudioSink::send(self, pcm)
    }

    fn finish(self) -> impl Future<Output = anyhow::Result<()>> + Send {
        AudioSink::finish(self)
    }
}

/// Moves audio to Mistral and its words back until the whole text came.
/// Audio that ends before any of it was sent was silence: Mistral refuses to
/// finish an empty session, so it is done with no text.
async fn pump(
    mut chunks: Receiver<Vec<u8>>,
    sink: impl AudioOut,
    mut events: impl Stream<Item = anyhow::Result<TranscriptionEvent>> + Unpin,
    said: Sender<Result<Heard, SttError>>,
) {
    let mut sink = Some(sink);
    let mut sent_any = false;
    let mut ended_at = None;
    loop {
        let wait = ended_at.map_or(DONE_WAIT, |at: Instant| {
            DONE_WAIT.saturating_sub(at.elapsed())
        });
        tokio::select! {
            chunk = chunks.recv(), if sink.is_some() => match chunk {
                Some(chunk) => {
                    let Some(to) = sink.as_ref() else { continue };
                    if chunk.is_empty() {
                        continue;
                    }
                    sent_any = true;
                    if let Err(error) = to.send(Bytes::from(chunk)).await {
                        // Why it ended comes through `events`.
                        log::debug!("live transcription stopped taking audio: {error:#}");
                        sink = None;
                    }
                }
                None if !sent_any => {
                    // Nobody may be left to read it; then it is not needed.
                    let _ = said.send(Ok(Heard::Done(String::new()))).await;
                    return;
                }
                None => {
                    if let Some(to) = sink.take()
                        && let Err(error) = to.finish().await
                    {
                        log::debug!("live transcription ended before its audio: {error:#}");
                    }
                    ended_at = Some(Instant::now());
                }
            },
            event = events.next() => {
                let heard = match event {
                    Some(Ok(TranscriptionEvent::TextDelta(text))) => Ok(Heard::Delta(text)),
                    Some(Ok(TranscriptionEvent::Done(done))) => {
                        // Nobody may be left to read it; then it is not needed.
                        let _ = said.send(Ok(Heard::Done(done.text))).await;
                        return;
                    }
                    Some(Ok(_)) => continue,
                    Some(Err(error)) => {
                        log::warn!("live transcription failed: {error:#}");
                        Err(SttError::Provider)
                    }
                    None => {
                        log::warn!("live transcription ended without its text");
                        Err(SttError::Provider)
                    }
                };
                let failed = heard.is_err();
                if said.send(heard).await.is_err() || failed {
                    return;
                }
            },
            () = sleep(wait), if ended_at.is_some() => {
                log::warn!("live transcription did not finish");
                // Gone already when the caller stopped waiting too.
                let _ = said.send(Err(SttError::Provider)).await;
                return;
            },
            () = said.closed() => return,
        }
    }
}

#[cfg(test)]
mod tests {
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
}
