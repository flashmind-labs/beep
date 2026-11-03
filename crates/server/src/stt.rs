//! Live transcription; `RealtimeStt` is the provider seam, `Unavailable` without one.

use futures::future::BoxFuture;
use thiserror::Error;
use tokio::sync::mpsc::{Receiver, Sender};

mod mistral;

pub use mistral::MistralStt;

/// Audio chunks waiting for the provider, a few seconds' worth.
pub const AUDIO_QUEUE: usize = 64;
/// Words waiting for the client.
pub const HEARD_QUEUE: usize = 64;

/// What a live transcription heard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Heard {
    /// More words, to add to what came before.
    Delta(String),
    /// The audio ended; the whole text.
    Done(String),
}

#[derive(Debug, Error)]
pub enum SttError {
    #[error("live transcription is not set up")]
    Unavailable,
    #[error("live transcription is busy")]
    Busy,
    #[error("the transcription provider failed")]
    Provider,
}

/// One live transcription: send 16 kHz mono 16-bit LE PCM into `audio`, drop it to end.
pub struct Session {
    pub audio: Sender<Vec<u8>>,
    pub heard: Receiver<Result<Heard, SttError>>,
}

/// Something that transcribes speech live.
pub trait RealtimeStt: Send + Sync {
    fn open(&self) -> BoxFuture<'_, Result<Session, SttError>>;
}

/// No live transcription: the server has no provider for it.
pub struct Unavailable;

impl RealtimeStt for Unavailable {
    fn open(&self) -> BoxFuture<'_, Result<Session, SttError>> {
        Box::pin(async { Err(SttError::Unavailable) })
    }
}

#[cfg(test)]
pub mod fake {
    //! A test transcriber that hears each chunk as "word".

    use super::{AUDIO_QUEUE, HEARD_QUEUE, Heard, RealtimeStt, Session, SttError};
    use futures::future::BoxFuture;
    use tokio::sync::mpsc;

    pub struct Words;

    impl RealtimeStt for Words {
        fn open(&self) -> BoxFuture<'_, Result<Session, SttError>> {
            Box::pin(async {
                let (audio, mut chunks) = mpsc::channel::<Vec<u8>>(AUDIO_QUEUE);
                let (said, heard) = mpsc::channel(HEARD_QUEUE);
                tokio::spawn(async move {
                    let mut all = Vec::new();
                    while let Some(chunk) = chunks.recv().await {
                        if chunk.is_empty() {
                            continue;
                        }
                        let word = if all.is_empty() { "word" } else { " word" };
                        all.push("word");
                        if said.send(Ok(Heard::Delta(word.into()))).await.is_err() {
                            return;
                        }
                    }
                    // Nobody may be left to read the end; then it is not needed.
                    let _ = said.send(Ok(Heard::Done(all.join(" ")))).await;
                });
                Ok(Session { audio, heard })
            })
        }
    }
}
