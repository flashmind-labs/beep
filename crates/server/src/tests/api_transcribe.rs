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
