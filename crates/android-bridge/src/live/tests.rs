use super::*;
use tokio::{
    sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel},
    time::timeout,
};

/// Stands in for the server: each transcription hands its sink to the
/// test, and every line is "spoken" as its own bytes.
struct FakeSpeech {
    transcriptions: UnboundedSender<HeardSink>,
    /// When set, every transcription says the server has none.
    unavailable: bool,
}

impl Speech for FakeSpeech {
    async fn transcribe(&self, mut audio: mpsc::Receiver<Vec<u8>>, heard: HeardSink) {
        if self.unavailable {
            return heard.hear(Hearing::Unavailable).await;
        }
        // The test may have stopped listening; then nobody speaks.
        let _ = self.transcriptions.send(heard);
        while audio.recv().await.is_some() {}
    }

    async fn say(&self, text: String) -> Result<Vec<u8>, BeepError> {
        Ok(text.into_bytes())
    }

    async fn filler(&self, _filler: Filler, _said: String) -> Result<Spoken, BeepError> {
        Ok(Spoken {
            text: "One moment.".into(),
            audio: b"filler".to_vec(),
        })
    }
}

struct Events(UnboundedSender<LiveEvent>);

impl LiveListener for Events {
    fn on_event(&self, event: LiveEvent) {
        // The test may be done reading.
        let _ = self.0.send(event);
    }
}

struct Harness {
    session: Arc<LiveSession>,
    events: UnboundedReceiver<LiveEvent>,
    transcriptions: UnboundedReceiver<HeardSink>,
}

impl Harness {
    fn start(holding: bool, unavailable: bool) -> Self {
        let (tx, events) = unbounded_channel();
        let (sinks, transcriptions) = unbounded_channel();
        let speech = Arc::new(FakeSpeech {
            transcriptions: sinks,
            unavailable,
        });
        let start = LiveStart {
            send_on_pause: true,
            holding,
        };
        let session = LiveSession::start(speech, Arc::new(Events(tx)), start);
        Self {
            session,
            events,
            transcriptions,
        }
    }

    /// The next event `pick` takes, skipping the others.
    async fn wait<T>(&mut self, mut pick: impl FnMut(&LiveEvent) -> Option<T>) -> T {
        let found = timeout(Duration::from_secs(5), async {
            loop {
                let event = self.events.recv().await.expect("the session is running");
                if let Some(found) = pick(&event) {
                    return found;
                }
            }
        });
        found.await.expect("the event came in time")
    }

    async fn phase(&mut self, phase: LivePhase) -> LiveView {
        self.wait(|e| match e {
            LiveEvent::View { view } if view.phase == phase => Some(view.clone()),
            _ => None,
        })
        .await
    }

    async fn transcription(&mut self) -> HeardSink {
        timeout(Duration::from_secs(5), self.transcriptions.recv())
            .await
            .expect("a transcription opened in time")
            .expect("the session is running")
    }

    /// Says `words` to the newest transcription and waits until the screen
    /// shows them. Words to a transcription that already ended are dropped,
    /// so they go again to any that opens meanwhile.
    async fn say(&mut self, words: &str) {
        let mut heard = self.transcription().await;
        while let Ok(newer) = self.transcriptions.try_recv() {
            heard = newer;
        }
        heard.hear(Hearing::Words(words.into())).await;
        let shown = timeout(Duration::from_secs(5), async {
            loop {
                tokio::select! {
                    event = self.events.recv() => match event.expect("the session is running") {
                        LiveEvent::View { view } if view.words == words => return,
                        _ => {}
                    },
                    Some(newer) = self.transcriptions.recv() => {
                        newer.hear(Hearing::Words(words.into())).await;
                    }
                }
            }
        });
        shown.await.expect("the words showed in time");
    }

    /// Holds, says `words` and lets go; returns what the session sent.
    async fn send(&mut self, words: &str) -> String {
        self.say(words).await;
        self.session.hold_end();
        self.wait(|e| match e {
            LiveEvent::Send { text, .. } => Some(text.clone()),
            _ => None,
        })
        .await
    }

    async fn play(&mut self) -> (u64, String) {
        self.wait(|e| match e {
            LiveEvent::Play {
                id, audio, filler, ..
            } if !filler => Some((*id, String::from_utf8_lossy(audio).into_owned())),
            _ => None,
        })
        .await
    }
}

#[tokio::test]
async fn letting_go_of_a_hold_sends_what_was_said() {
    let mut h = Harness::start(true, false);
    assert_eq!(h.send("What's on tomorrow").await, "What's on tomorrow");
    let view = h.phase(LivePhase::Thinking).await;
    assert_eq!(view.words, "");
}

#[tokio::test]
async fn the_reply_is_read_out_sentence_by_sentence_then_it_listens() {
    let mut h = Harness::start(true, false);
    h.send("What's on tomorrow").await;
    h.session.reply_event(RunEvent::TextDelta {
        delta: "The dentist at 9:30. Then a call".into(),
    });
    let (first, audio) = h.play().await;
    assert_eq!(audio, "The dentist at 9:30.");
    h.phase(LivePhase::Speaking).await;
    h.session.reply_event(RunEvent::TextDelta {
        delta: " with Alex.".into(),
    });
    h.session.reply_event(RunEvent::RunFinished {
        status: "completed".into(),
        updated_at: None,
    });
    h.session.played(first);
    let (second, audio) = h.play().await;
    assert_eq!(audio, "Then a call with Alex.");
    h.session.played(second);
    let view = h.phase(LivePhase::Listening).await;
    assert_eq!(view.line.as_deref(), Some("Then a call with Alex."));
    assert_eq!(view.prev.as_deref(), Some("The dentist at 9:30."));
    // Listening again opened a new transcription.
    h.say("thanks").await;
}

#[tokio::test]
async fn a_tap_while_speaking_stops_the_audio_and_listens() {
    let mut h = Harness::start(true, false);
    h.send("Read me the news").await;
    h.session.reply_event(RunEvent::TextDelta {
        delta: "Rain on Saturday. ".into(),
    });
    h.play().await;
    h.phase(LivePhase::Speaking).await;
    h.session.interrupt();
    h.wait(|e| (*e == LiveEvent::StopAudio).then_some(())).await;
    h.wait(|e| match e {
        LiveEvent::Toast { text } if text == "Stopped" => Some(()),
        _ => None,
    })
    .await;
    h.phase(LivePhase::Listening).await;
    // The rest of the reply is no longer read out.
    h.session.reply_event(RunEvent::TextDelta {
        delta: "Sun on Sunday. ".into(),
    });
    h.session.reply_event(RunEvent::RunFinished {
        status: "completed".into(),
        updated_at: None,
    });
    h.say("what else").await;
}

#[tokio::test]
async fn a_failed_reply_says_so_and_listens() {
    let mut h = Harness::start(true, false);
    h.send("Hello").await;
    h.session.reply_failed();
    h.wait(|e| match e {
        LiveEvent::Toast { text } if text == "Could not send" => Some(()),
        _ => None,
    })
    .await;
    h.phase(LivePhase::Listening).await;
}

#[tokio::test]
async fn stopping_keeps_what_was_said_as_a_draft() {
    let mut h = Harness::start(false, false);
    h.say("Remind me at nine").await;
    h.session.stop();
    let left = h
        .wait(|e| match e {
            LiveEvent::Left { draft, quiet, .. } => Some((draft.clone(), *quiet)),
            _ => None,
        })
        .await;
    assert_eq!(left, (Some("Remind me at nine".into()), false));
}

#[tokio::test]
async fn no_live_transcription_on_the_server_leaves_with_the_reason() {
    let mut h = Harness::start(false, true);
    let failure = h
        .wait(|e| match e {
            LiveEvent::Left { failure, .. } => Some(failure.clone()),
            _ => None,
        })
        .await;
    assert!(failure.is_some_and(|f| f.contains("no live transcription")));
}

#[tokio::test]
async fn dropping_the_session_leaves_live_mode() {
    let Harness {
        session,
        mut events,
        ..
    } = Harness::start(false, false);
    drop(session);
    let left = timeout(Duration::from_secs(5), async {
        while let Some(event) = events.recv().await {
            if matches!(event, LiveEvent::Left { .. }) {
                return true;
            }
        }
        false
    });
    assert!(left.await.expect("it left in time"));
}

#[tokio::test]
async fn played_for_an_old_line_does_nothing() {
    let mut h = Harness::start(true, false);
    h.send("Hi").await;
    h.session.reply_event(RunEvent::TextDelta {
        delta: "Hello there. ".into(),
    });
    let (id, _) = h.play().await;
    h.session.played(id + 7);
    h.session.reply_event(RunEvent::RunFinished {
        status: "completed".into(),
        updated_at: None,
    });
    // Still speaking until the line that plays is done.
    h.session.played(id);
    h.phase(LivePhase::Listening).await;
}

#[tokio::test]
async fn a_tool_step_shows_while_thinking() {
    let mut h = Harness::start(true, false);
    h.send("Is build-box out of disk?").await;
    h.session.reply_event(RunEvent::ToolStarted {
        name: "shell".into(),
        call_id: "c1".into(),
        description: "Running df -h on build-box".into(),
    });
    let view = h
        .wait(|e| match e {
            LiveEvent::View { view } if view.step.is_some() => Some(view.clone()),
            _ => None,
        })
        .await;
    assert_eq!(view.phase, LivePhase::Thinking);
    assert_eq!(view.step.as_deref(), Some("Running df -h on build-box"));
}
