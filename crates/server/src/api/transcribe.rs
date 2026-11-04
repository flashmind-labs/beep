//! `Transcribe`: the app streams speech up and the words come back as recognized.

use super::Reply;
use crate::{
    api::router::{Closed, Cx, Sink, Uplink},
    protocol::{ApiError, MAX_TRANSCRIBE_BYTES, TRANSCRIBE_RATE, Transcribe, Transcription},
    voice::stt::{Heard, Session, SttError},
};
use serde_bytes::ByteBuf;
use std::time::Duration;
use tokio::time::timeout;

/// How long the provider gets to finish once the audio ended.
const FINISH_WAIT: Duration = Duration::from_secs(10);
/// How long the client may send nothing before the transcription ends.
const IDLE: Duration = Duration::from_secs(30);
/// The longest a live transcription runs.
const MAX_LENGTH: Duration = Duration::from_secs(6 * 60);

pub(super) async fn transcribe(
    cx: Cx,
    req: Transcribe,
    mut uplink: Uplink<ByteBuf>,
    sink: Sink<Transcription>,
) -> Reply<()> {
    if req.sample_rate != TRANSCRIBE_RATE {
        return Err(ApiError::bad_request(format!(
            "audio must be at {TRANSCRIBE_RATE} Hz"
        )));
    }
    let live = match cx.app.transcribe_live(cx.user.id()).await {
        Ok(live) => live,
        Err(SttError::Unavailable) => {
            sink.send(&Transcription::Unavailable {}).await?;
            return Ok(());
        }
        // A conflict, so the app can wait for a slot quietly.
        Err(SttError::Busy) => {
            return Err(ApiError::Conflict {
                message: "too many live transcriptions at once".into(),
            });
        }
        Err(SttError::Provider) => return Err(ApiError::Internal {}),
    };
    match timeout(MAX_LENGTH, relay(live.session, &mut uplink, &sink, IDLE)).await {
        Ok(relayed) => relayed,
        Err(_) => Err(ApiError::bad_request("the transcription ran too long")),
    }
}

/// Where the audio comes from: the client's uplink, or a list in tests.
pub(super) trait Chunks {
    async fn next_chunk(&mut self) -> Result<Option<ByteBuf>, ApiError>;
}

impl Chunks for Uplink<ByteBuf> {
    async fn next_chunk(&mut self) -> Result<Option<ByteBuf>, ApiError> {
        self.next().await
    }
}

/// Where the words go.
pub(super) trait Words {
    async fn say(&self, heard: Transcription) -> Result<(), Closed>;
}

impl Words for Sink<Transcription> {
    async fn say(&self, heard: Transcription) -> Result<(), Closed> {
        self.send(&heard).await
    }
}

/// Feeds the client's audio to `session` and its words back; a client quiet for `idle` is cut off.
pub(super) async fn relay(
    session: Session,
    chunks: &mut impl Chunks,
    words: &impl Words,
    idle: Duration,
) -> Reply<()> {
    let Session { audio, mut heard } = session;
    let mut audio = Some(audio);
    let mut total = 0usize;
    // The loudest sample, to tell silence from speech nobody understood.
    let mut peak = 0u16;
    loop {
        let event = match audio.as_ref() {
            Some(to) => tokio::select! {
                chunk = timeout(idle, chunks.next_chunk()) => {
                    let Ok(chunk) = chunk else {
                        return Err(ApiError::bad_request("no audio came for too long"));
                    };
                    match chunk? {
                        Some(chunk) => {
                            total += chunk.len();
                            peak = chunk
                                .as_chunks::<2>()
                                .0
                                .iter()
                                .map(|s| i16::from_le_bytes(*s).unsigned_abs())
                                .fold(peak, u16::max);
                            if total > MAX_TRANSCRIBE_BYTES {
                                return Err(ApiError::bad_request("too much audio"));
                            }
                            if to.send(chunk.into_vec()).await.is_err() {
                                // The provider stopped; its reason comes through `heard`.
                                audio = None;
                            }
                        }
                        // Dropping the sender tells the provider the speech ended.
                        None => audio = None,
                    }
                    continue;
                }
                event = heard.recv() => event,
            },
            None => match timeout(FINISH_WAIT, heard.recv()).await {
                Ok(event) => event,
                Err(_) => {
                    log::warn!("live transcription did not finish");
                    return Err(ApiError::Internal {});
                }
            },
        };
        match event {
            Some(Ok(Heard::Delta(text))) => words.say(Transcription::Delta { text }).await?,
            Some(Ok(Heard::Done(text))) => {
                log::info!(
                    "live transcription done: {total} bytes of audio, peak {peak}, {} chars heard",
                    text.chars().count()
                );
                words.say(Transcription::Done { text }).await?;
                return Ok(());
            }
            Some(Err(e)) => {
                log::warn!("live transcription failed: {e}");
                return Err(ApiError::Internal {});
            }
            None => {
                log::warn!("live transcription ended without its text");
                return Err(ApiError::Internal {});
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::voice::stt::{RealtimeStt, fake};
    use std::sync::Mutex;

    struct List(Vec<ByteBuf>);

    impl Chunks for List {
        async fn next_chunk(&mut self) -> Result<Option<ByteBuf>, ApiError> {
            Ok(if self.0.is_empty() {
                None
            } else {
                Some(self.0.remove(0))
            })
        }
    }

    #[derive(Default)]
    struct Said(Mutex<Vec<Transcription>>);

    impl Words for Said {
        async fn say(&self, heard: Transcription) -> Result<(), Closed> {
            self.0.lock().expect("not poisoned in tests").push(heard);
            Ok(())
        }
    }

    #[tokio::test]
    async fn too_much_audio_is_refused() {
        let session = fake::Words.open().await.expect("the fake opens");
        let big = ByteBuf::from(vec![0u8; MAX_TRANSCRIBE_BYTES + 1]);
        let refused = relay(session, &mut List(vec![big]), &Said::default(), IDLE).await;
        assert!(matches!(refused, Err(ApiError::BadRequest { .. })));
    }

    /// A client that never sends again.
    struct Stalled;

    impl Chunks for Stalled {
        async fn next_chunk(&mut self) -> Result<Option<ByteBuf>, ApiError> {
            std::future::pending().await
        }
    }

    #[tokio::test]
    async fn a_client_that_goes_quiet_is_cut_off() {
        let session = fake::Words.open().await.expect("the fake opens");
        let refused = relay(session, &mut Stalled, &Said::default(), Duration::ZERO).await;
        assert!(matches!(refused, Err(ApiError::BadRequest { .. })));
    }
}
