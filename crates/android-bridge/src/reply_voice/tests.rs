use super::*;
use std::sync::Mutex;
use tokio::{
    sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel},
    time::{Duration, timeout},
};

/// Stands in for the server: every sentence is "spoken" as its own bytes,
/// and "mute" ones cannot be voiced.
struct FakeVoice;

impl Voice for FakeVoice {
    async fn say(&self, text: String) -> Result<Vec<u8>, BeepError> {
        if text.contains("mute") {
            return Err(BeepError::Internal);
        }
        Ok(text.into_bytes())
    }
}

struct Events(Mutex<UnboundedSender<ReplyVoiceEvent>>);

impl ReplyVoiceListener for Events {
    fn on_event(&self, event: ReplyVoiceEvent) {
        let _ = self.0.lock().expect("not poisoned").send(event);
    }
}

fn start() -> (Arc<ReplyVoice>, UnboundedReceiver<ReplyVoiceEvent>) {
    let (sender, events) = unbounded_channel();
    let voice = ReplyVoice::start(Arc::new(FakeVoice), Arc::new(Events(Mutex::new(sender))));
    (voice, events)
}

async fn next(events: &mut UnboundedReceiver<ReplyVoiceEvent>) -> ReplyVoiceEvent {
    timeout(Duration::from_secs(5), events.recv())
        .await
        .expect("an event in time")
        .expect("the reading is still on")
}

/// Plays the next sentence to its end and returns its text and audio.
async fn play(
    voice: &ReplyVoice,
    events: &mut UnboundedReceiver<ReplyVoiceEvent>,
) -> (String, Vec<u8>) {
    match next(events).await {
        ReplyVoiceEvent::Play { id, text, audio } => {
            voice.played(id);
            (text, audio)
        }
        ReplyVoiceEvent::Done => panic!("done before the sentence played"),
    }
}

#[tokio::test]
async fn reads_sentences_as_the_reply_grows_then_the_rest_when_it_ends() {
    let (voice, mut events) = start();
    voice.update("The plumber comes".to_owned());
    voice.update("The plumber comes Thursday. Bring".to_owned());
    assert_eq!(
        play(&voice, &mut events).await.0,
        "The plumber comes Thursday."
    );
    voice.update("The plumber comes Thursday. Bring the key".to_owned());
    voice.finish();
    assert_eq!(play(&voice, &mut events).await.0, "Bring the key");
    assert_eq!(next(&mut events).await, ReplyVoiceEvent::Done);
}

#[tokio::test]
async fn plays_one_sentence_at_a_time_in_order() {
    let (voice, mut events) = start();
    voice.update("One. Two. Three. ".to_owned());
    voice.finish();
    let ReplyVoiceEvent::Play { id, text, .. } = next(&mut events).await else {
        panic!("a sentence first");
    };
    assert_eq!(text, "One.");
    // Nothing else plays until the first one finished.
    assert!(
        timeout(Duration::from_millis(100), events.recv())
            .await
            .is_err()
    );
    voice.played(id);
    assert_eq!(play(&voice, &mut events).await.0, "Two.");
    assert_eq!(play(&voice, &mut events).await.0, "Three.");
    assert_eq!(next(&mut events).await, ReplyVoiceEvent::Done);
}

#[tokio::test]
async fn a_sentence_the_server_cannot_voice_comes_without_audio() {
    let (voice, mut events) = start();
    voice.update("Say this mute line. Then this one. ".to_owned());
    voice.finish();
    assert_eq!(
        play(&voice, &mut events).await,
        ("Say this mute line.".to_owned(), Vec::new())
    );
    assert_eq!(
        play(&voice, &mut events).await.1,
        b"Then this one.".to_vec()
    );
    assert_eq!(next(&mut events).await, ReplyVoiceEvent::Done);
}

#[tokio::test]
async fn text_that_does_not_continue_the_reply_is_ignored() {
    let (voice, mut events) = start();
    voice.update("First part. ".to_owned());
    voice.update("Something else. ".to_owned());
    voice.finish();
    assert_eq!(play(&voice, &mut events).await.0, "First part.");
    assert_eq!(next(&mut events).await, ReplyVoiceEvent::Done);
}

#[tokio::test]
async fn dropping_it_stops_the_reading() {
    let (voice, mut events) = start();
    voice.update("One. Two. ".to_owned());
    assert!(matches!(
        next(&mut events).await,
        ReplyVoiceEvent::Play { .. }
    ));
    drop(voice);
    assert!(
        timeout(Duration::from_secs(5), events.recv())
            .await
            .expect("the reading ends")
            .is_none()
    );
}
