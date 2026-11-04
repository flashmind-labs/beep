//! The server's live transcription of the microphone: opening one, its
//! words, retrying when it fails, and the last words of a released hold.

use super::{
    Voice,
    session::{MIC_CHUNKS, UNAVAILABLE},
};
use crate::error::DesktopError;
use beep_live::{Input, Phase};
use beep_proto::{ApiError, Transcription};
use serde_bytes::ByteBuf;
use std::{sync::Arc, time::Duration};
use tokio::{sync::mpsc, time::sleep};

/// Failed transcriptions in a row before live mode gives up.
const STT_TRIES: u32 = 3;
/// Wait before opening a transcription again after one failed.
const STT_RETRY: Duration = Duration::from_secs(1);
/// Tries while the server has no transcription free, waiting longer each
/// time, about half a minute in all.
const BUSY_TRIES: u32 = 6;
/// The longest wait between those tries.
const BUSY_WAIT: Duration = Duration::from_secs(8);
/// How long letting go of Option waits for the last words.
const FINISH_WAIT: Duration = Duration::from_secs(2);
const BUSY: &str = "Voice is busy in another window or on another device. Try again in a moment.";

/// How a transcription ended.
enum SttEnd {
    /// It is no longer the one running, or live mode is leaving.
    Stale,
    /// The server said all it heard. `tail` is what the words so far left
    /// out, when the whole text starts with them.
    Done {
        tail: Option<String>,
    },
    /// The stream closed without its text.
    Closed,
    Failed {
        busy: bool,
        message: String,
    },
}

/// Whether the server turned a transcription away for want of a slot.
fn busy(api: Option<&ApiError>) -> bool {
    matches!(api, Some(ApiError::Conflict { .. }))
}

impl Voice {
    pub(super) fn transcribe(self: &Arc<Self>, id: u64) {
        let (stt, sound) = mpsc::channel(MIC_CHUNKS);
        let run = {
            let mut state = self.state();
            let Some(s) = state.live.as_mut().filter(|s| s.id == id) else {
                return;
            };
            s.stt = Some(stt);
            s.stt_run += 1;
            s.finishing = None;
            s.stt_run
        };
        let me = self.clone();
        tauri::async_runtime::spawn(async move { me.run_transcription(id, run, sound).await });
    }

    pub(super) fn end_transcription(&self, id: u64) {
        if let Some(s) = self.state().live.as_mut().filter(|s| s.id == id) {
            // Dropping the sender ends the audio; the run's last words no
            // longer count.
            s.stt = None;
            s.stt_run += 1;
            s.finishing = None;
        }
    }

    /// Whether transcription `run` of session `id` is the one running.
    fn is_run(&self, id: u64, run: u64) -> bool {
        self.state()
            .live
            .as_ref()
            .is_some_and(|s| s.id == id && s.stt_run == run)
    }

    async fn run_transcription(
        self: Arc<Self>,
        id: u64,
        run: u64,
        mut sound: mpsc::Receiver<Vec<u8>>,
    ) {
        let (mut uplink, mut replies) = match self.beep.transcribe().await {
            Ok(opened) => opened,
            Err(e) => {
                let api = match &e {
                    DesktopError::Api(api) => Some(api),
                    _ => None,
                };
                let end = SttEnd::Failed {
                    busy: busy(api),
                    message: e.to_string(),
                };
                return self.stt_ended(id, run, end);
            }
        };
        let up = tauri::async_runtime::spawn(async move {
            let mut sent = 0usize;
            while let Some(pcm) = sound.recv().await {
                sent += pcm.len();
                if let Err(e) = uplink.send(&ByteBuf::from(pcm)).await {
                    log::debug!("live audio stopped going up: {e}");
                    return;
                }
            }
            log::info!("live transcription {run}: {sent} bytes of audio sent");
            if let Err(e) = uplink.finish() {
                log::debug!("ending live audio: {e}");
            }
        });
        let mut heard = String::new();
        let end = loop {
            match replies.next().await {
                Ok(Some(Transcription::Delta { text })) => {
                    if !self.is_run(id, run) {
                        break SttEnd::Stale;
                    }
                    if let Some(s) = self.state().live.as_mut().filter(|s| s.id == id) {
                        s.stt_failures = 0;
                        s.stt_busy = 0;
                    }
                    heard.push_str(&text);
                    self.input(id, Input::Words(text));
                }
                Ok(Some(Transcription::Done { text })) => {
                    let tail = text.strip_prefix(heard.as_str()).map(str::to_owned);
                    break SttEnd::Done { tail };
                }
                Ok(None) => break SttEnd::Closed,
                Ok(Some(Transcription::Unavailable {})) => {
                    self.fail(
                        id,
                        UNAVAILABLE,
                        "the server has no live transcription".into(),
                    );
                    break SttEnd::Stale;
                }
                Err(e) => {
                    break SttEnd::Failed {
                        busy: busy(e.downcast_ref::<ApiError>()),
                        message: format!("{e:#}"),
                    };
                }
            }
        };
        up.abort();
        self.stt_ended(id, run, end);
    }

    /// Transcription `run` ended: a released hold takes its last words and
    /// sends; otherwise live mode must not go deaf, so it opens another.
    fn stt_ended(self: &Arc<Self>, id: u64, run: u64, end: SttEnd) {
        let finishing = self
            .state()
            .live
            .as_ref()
            .is_some_and(|s| s.id == id && s.finishing == Some(run));
        if finishing {
            if let SttEnd::Done { tail: Some(tail) } = &end
                && !tail.trim().is_empty()
            {
                self.input(id, Input::Words(tail.clone()));
            }
            if let SttEnd::Failed { message, .. } = &end {
                log::warn!("live transcription, finishing: {message}");
            }
            return self.finish_hold(id, run);
        }
        let listening = self
            .state()
            .live
            .as_ref()
            .is_some_and(|s| s.id == id && s.stt_run == run && s.stt.is_some());
        if !listening {
            return;
        }
        match end {
            SttEnd::Stale => {}
            // The server ended it, such as at its length limit.
            SttEnd::Done { .. } => self.transcribe(id),
            SttEnd::Closed => self.stt_failed(id, run, false, "the transcription ended".into()),
            SttEnd::Failed { busy, message } => self.stt_failed(id, run, busy, message),
        }
    }

    /// Opens the transcription again, unless it keeps failing. While the
    /// server has none free it waits longer each time, saying nothing.
    fn stt_failed(self: &Arc<Self>, id: u64, run: u64, busy: bool, message: String) {
        log::warn!("live transcription: {message}");
        let tries = {
            let mut state = self.state();
            let Some(s) = state
                .live
                .as_mut()
                .filter(|s| s.id == id && s.stt_run == run && s.stt.is_some())
            else {
                return;
            };
            let tries = if busy {
                &mut s.stt_busy
            } else {
                &mut s.stt_failures
            };
            *tries += 1;
            *tries
        };
        let wait = if busy {
            if tries > BUSY_TRIES {
                return self.fail(id, BUSY, message);
            }
            STT_RETRY
                .saturating_mul(1u32 << (tries - 1).min(4))
                .min(BUSY_WAIT)
        } else {
            if tries >= STT_TRIES {
                return self.fail(id, UNAVAILABLE, message);
            }
            STT_RETRY
        };
        let me = self.clone();
        tauri::async_runtime::spawn(async move {
            sleep(wait).await;
            if me.is_run(id, run) {
                me.transcribe(id);
            }
        });
    }

    /// Option came up after a hold. The words said last may still be on
    /// their way, so the audio ends and the hold waits for the server's last
    /// words, up to `FINISH_WAIT`, before it sends.
    pub(super) fn release(self: &Arc<Self>) {
        let finishing = {
            let mut state = self.state();
            let Some(s) = state.live.as_mut() else {
                return;
            };
            if s.live.phase() != Phase::Holding || s.finishing.is_some() {
                None
            } else if s.stt.take().is_some() {
                // Dropping the sender ends the audio going up.
                s.finishing = Some(s.stt_run);
                Some((s.id, s.stt_run))
            } else {
                None
            }
        };
        let Some((id, run)) = finishing else {
            return self.input_now(Input::HoldEnd);
        };
        self.show(id);
        let me = self.clone();
        tauri::async_runtime::spawn(async move {
            sleep(FINISH_WAIT).await;
            me.finish_hold(id, run);
        });
    }

    /// The hold waiting on transcription `run` ends now.
    fn finish_hold(self: &Arc<Self>, id: u64, run: u64) {
        let finishing = self
            .state()
            .live
            .as_mut()
            .filter(|s| s.id == id && s.finishing == Some(run))
            .map(|s| s.finishing = None)
            .is_some();
        if finishing {
            self.input(id, Input::HoldEnd);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_busy_server_is_told_apart() {
        assert!(busy(Some(&ApiError::Conflict {
            message: "live transcriptions busy".into()
        })));
        assert!(!busy(Some(&ApiError::Internal {})));
        assert!(!busy(None));
    }
}
