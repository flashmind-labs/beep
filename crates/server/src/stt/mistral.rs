//! Live transcription through Mistral's realtime API, by way of flashmind's
//! Mistral provider.

use super::{AUDIO_QUEUE, HEARD_QUEUE, Heard, RealtimeStt, Session, SttError};
use bytes::Bytes;
use flashmind::llm::{
    MistralProvider,
    mistral::{AudioSink, RealtimeTranscriptionRequest, TranscriptionEvent, TranscriptionEvents},
};
use futures::{StreamExt, future::BoxFuture};
use std::{sync::Arc, time::Duration};
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

/// Moves audio to Mistral and its words back until the whole text came.
async fn pump(
    mut chunks: Receiver<Vec<u8>>,
    sink: AudioSink,
    mut events: TranscriptionEvents,
    said: Sender<Result<Heard, SttError>>,
) {
    let mut sink = Some(sink);
    let mut ended_at = None;
    loop {
        let wait = ended_at.map_or(DONE_WAIT, |at: Instant| {
            DONE_WAIT.saturating_sub(at.elapsed())
        });
        tokio::select! {
            chunk = chunks.recv(), if sink.is_some() => match chunk {
                Some(chunk) => {
                    let Some(to) = sink.as_ref() else { continue };
                    if let Err(error) = to.send(Bytes::from(chunk)).await {
                        // Why it ended comes through `events`.
                        log::debug!("live transcription stopped taking audio: {error:#}");
                        sink = None;
                    }
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
