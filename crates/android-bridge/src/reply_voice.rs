//! A chat reply read out as it streams, outside live mode: what the chat's
//! speak button starts while a reply loads.
//!
//! The app hands over the reply's text so far with `update` each time it
//! grows, and `finish` once it ended. Finished sentences are voiced one
//! ahead of the one playing, with the same `Say` call live mode reads
//! replies with. The app plays each `ReplyVoiceEvent::Play` in turn and
//! says when it finished with `played`. Dropping the object stops it.

use crate::BeepError;
use beep_live::Reading;
use std::{collections::VecDeque, future::Future, sync::Arc};
use tokio::sync::{mpsc, watch};

/// Sentences voiced ahead of the one playing.
const AHEAD: usize = 1;

/// What the app is told while a reply is read out.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum ReplyVoiceEvent {
    /// Play `audio`, an MP3, then call `played(id)`. Empty `audio` means
    /// the server could not voice the sentence: say `text` with the
    /// device's own voice, or skip it, then call `played(id)`.
    Play {
        id: u64,
        text: String,
        audio: Vec<u8>,
    },
    /// Every sentence played: the reading is over.
    Done,
}

/// Gets what a reading has to say. Called from a background thread.
#[uniffi::export(with_foreign)]
pub trait ReplyVoiceListener: Send + Sync {
    fn on_event(&self, event: ReplyVoiceEvent);
}

/// The server call a reading makes, so tests can stand in for it.
pub(crate) trait Voice: Send + Sync + 'static {
    /// `text` spoken as it is, as an MP3.
    fn say(&self, text: String) -> impl Future<Output = Result<Vec<u8>, BeepError>> + Send;
}

/// The reply as the app last handed it over.
#[derive(Debug, Default)]
struct Feed {
    text: String,
    finished: bool,
}

#[derive(uniffi::Object)]
pub struct ReplyVoice {
    feed: watch::Sender<Feed>,
    played: mpsc::Sender<u64>,
}

impl ReplyVoice {
    /// Starts a reading on the tokio runtime this is called from.
    pub(crate) fn start<V: Voice>(
        voice: Arc<V>,
        listener: Arc<dyn ReplyVoiceListener>,
    ) -> Arc<Self> {
        let (feed, fed) = watch::channel(Feed::default());
        // One sentence plays at a time, so one `played` is ever pending.
        let (played, plays) = mpsc::channel(4);
        tokio::spawn(read(voice, listener, fed, plays));
        Arc::new(Self { feed, played })
    }
}

#[uniffi::export]
impl ReplyVoice {
    /// The reply's text so far. Only what follows the text handed over
    /// before is read; a text that does not continue it is ignored.
    pub fn update(&self, text: String) {
        self.feed.send_if_modified(|feed| {
            let grows =
                !feed.finished && text.len() > feed.text.len() && text.starts_with(&feed.text);
            if grows {
                feed.text = text;
            }
            grows
        });
    }

    /// The reply ended: what is left of it is read, then `Done` comes.
    pub fn finish(&self) {
        self.feed
            .send_if_modified(|feed| !std::mem::replace(&mut feed.finished, true));
    }

    /// The `Play` with `id` finished playing, or could not play.
    pub fn played(&self, id: u64) {
        if self.played.try_send(id).is_err() {
            log::warn!("reply voice did not take played({id})");
        }
    }
}

/// Reads the reply fed through `fed` until it is finished and every
/// sentence played, or until the `ReplyVoice` is dropped.
async fn read<V: Voice>(
    voice: Arc<V>,
    listener: Arc<dyn ReplyVoiceListener>,
    mut fed: watch::Receiver<Feed>,
    mut plays: mpsc::Receiver<u64>,
) {
    let mut reading = Reading::default();
    let mut read_to = 0;
    let mut finished = false;
    let mut sentences: VecDeque<String> = VecDeque::new();
    // Voiced sentences waiting to play, in order.
    let mut ready: VecDeque<(String, Vec<u8>)> = VecDeque::new();
    let mut voicing = None;
    let mut playing = None;
    let mut next_id = 0;
    loop {
        if playing.is_none()
            && let Some((text, audio)) = ready.pop_front()
        {
            next_id += 1;
            playing = Some(next_id);
            listener.on_event(ReplyVoiceEvent::Play {
                id: next_id,
                text,
                audio,
            });
        }
        if voicing.is_none()
            && ready.len() < AHEAD + usize::from(playing.is_none())
            && let Some(text) = sentences.pop_front()
        {
            let voice = voice.clone();
            voicing = Some(Box::pin(async move {
                let audio = voice.say(text.clone()).await;
                (text, audio)
            }));
        }
        if finished
            && sentences.is_empty()
            && voicing.is_none()
            && ready.is_empty()
            && playing.is_none()
        {
            listener.on_event(ReplyVoiceEvent::Done);
            return;
        }
        tokio::select! {
            changed = fed.changed(), if !finished => {
                if changed.is_err() {
                    return;
                }
                let feed = fed.borrow_and_update();
                sentences.extend(reading.push(&feed.text[read_to..]));
                read_to = feed.text.len();
                if feed.finished {
                    finished = true;
                    sentences.extend(reading.finish());
                }
            }
            (text, audio) = async { voicing.as_mut().expect("guarded by the branch condition").await }, if voicing.is_some() => {
                voicing = None;
                let audio = audio.unwrap_or_else(|error| {
                    log::warn!("reply voice could not voice a sentence: {error}");
                    Vec::new()
                });
                ready.push_back((text, audio));
            }
            id = plays.recv() => match id {
                Some(id) if playing == Some(id) => playing = None,
                Some(_) => {}
                None => return,
            },
        }
    }
}

#[cfg(test)]
mod tests;
