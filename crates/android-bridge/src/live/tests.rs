use super::*;
use beep_proto::{Approval, DeviceQuery};
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
    /// When set, the server cannot voice answers.
    mute: bool,
}

impl Speech for FakeSpeech {
    async fn transcribe(&self, mut audio: mpsc::Receiver<Vec<u8>>, heard: HeardSink) {
        if self.unavailable {
            return heard.hear(Hearing::Unavailable).await;
        }
        heard.hear(Hearing::Open).await;
        // The test may have stopped listening; then nobody speaks.
        let _ = self.transcriptions.send(heard);
        while audio.recv().await.is_some() {}
    }

    async fn say(&self, text: String) -> Result<Vec<u8>, BeepError> {
        if self.mute {
            return Err(BeepError::Internal);
        }
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
        Self::with(holding, unavailable, false)
    }

    fn with(holding: bool, unavailable: bool, mute: bool) -> Self {
        let (tx, events) = unbounded_channel();
        let (sinks, transcriptions) = unbounded_channel();
        let speech = Arc::new(FakeSpeech {
            transcriptions: sinks,
            unavailable,
            mute,
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
        self.send_reply(words).await.1
    }

    /// Like `send`, with the reply number.
    async fn send_reply(&mut self, words: &str) -> (u64, String) {
        self.say(words).await;
        self.session.hold_end();
        self.wait(|e| match e {
            LiveEvent::Send { reply, text, .. } => Some((*reply, text.clone())),
            _ => None,
        })
        .await
    }

    fn text(&self, reply: u64, delta: &str) {
        self.session.reply_event(
            reply,
            RunEvent::TextDelta {
                delta: delta.into(),
            },
        );
    }

    fn finish(&self, reply: u64, status: &str) {
        self.session.reply_event(
            reply,
            RunEvent::RunFinished {
                status: status.into(),
                updated_at: None,
            },
        );
    }

    async fn toast(&mut self, wanted: &str) {
        self.wait(|e| match e {
            LiveEvent::Toast { text } if text == wanted => Some(()),
            _ => None,
        })
        .await;
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
    let (reply, _) = h.send_reply("What's on tomorrow").await;
    h.text(reply, "The dentist at 9:30. Then a call");
    let (first, audio) = h.play().await;
    assert_eq!(audio, "The dentist at 9:30.");
    h.phase(LivePhase::Speaking).await;
    h.text(reply, " with Alex.");
    h.finish(reply, "completed");
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
async fn a_tap_while_speaking_cancels_the_reply_and_the_next_one_is_read() {
    let mut h = Harness::start(true, false);
    let (reply, _) = h.send_reply("Read me the news").await;
    h.session.reply_event(
        reply,
        RunEvent::RunStarted {
            run_id: "run-1".into(),
            session_id: "chat".into(),
        },
    );
    h.text(reply, "Rain on Saturday. ");
    h.play().await;
    h.phase(LivePhase::Speaking).await;
    h.session.interrupt();
    h.wait(|e| (*e == LiveEvent::StopAudio).then_some(())).await;
    let cancelled = h
        .wait(|e| match e {
            LiveEvent::CancelReply { reply, run_id } => Some((*reply, run_id.clone())),
            _ => None,
        })
        .await;
    assert_eq!(cancelled, (reply, Some("run-1".into())));
    h.toast("Stopped").await;
    h.phase(LivePhase::Listening).await;
    // The old run goes on streaming: none of it is read out, now or as
    // the next reply.
    h.text(reply, "Sun on Sunday. ");
    let (next, _) = h.send_reply("what else").await;
    assert_ne!(next, reply);
    h.text(reply, "Snow on Monday. ");
    h.finish(reply, "completed");
    h.text(next, "Nothing else. ");
    let (_, audio) = h.play().await;
    assert_eq!(audio, "Nothing else.");
}

#[tokio::test]
async fn a_tap_while_thinking_cancels_and_listens() {
    let mut h = Harness::start(true, false);
    let (reply, _) = h.send_reply("Hello").await;
    h.phase(LivePhase::Thinking).await;
    h.session.tap();
    let cancelled = h
        .wait(|e| match e {
            LiveEvent::CancelReply { reply, run_id } => Some((*reply, run_id.clone())),
            _ => None,
        })
        .await;
    assert_eq!(cancelled, (reply, None));
    h.phase(LivePhase::Listening).await;
}

#[tokio::test]
async fn saying_stop_while_thinking_cancels() {
    let mut h = Harness::start(true, false);
    let (reply, _) = h.send_reply("Hello").await;
    h.phase(LivePhase::Thinking).await;
    let heard = h.transcription().await;
    heard.hear(Hearing::Words("Stop.".into())).await;
    let cancelled = h
        .wait(|e| match e {
            LiveEvent::CancelReply { reply, .. } => Some(*reply),
            _ => None,
        })
        .await;
    assert_eq!(cancelled, reply);
    h.phase(LivePhase::Listening).await;
}

#[tokio::test]
async fn leaving_mid_reply_cancels_it_but_not_a_finished_one() {
    let mut h = Harness::start(true, false);
    let (reply, _) = h.send_reply("Hello").await;
    h.session.stop();
    let mut cancelled = false;
    let reason = h
        .wait(|e| match e {
            LiveEvent::CancelReply { reply: r, .. } if *r == reply => {
                cancelled = true;
                None
            }
            LiveEvent::Left { reason, .. } => Some(*reason),
            _ => None,
        })
        .await;
    assert!(cancelled);
    assert_eq!(reason, LeaveReason::Asked);

    let mut h = Harness::start(true, false);
    let (reply, _) = h.send_reply("Hello").await;
    h.text(reply, "Hi there. ");
    h.finish(reply, "completed");
    h.play().await;
    h.session.interrupted();
    let left = h
        .wait(|e| match e {
            LiveEvent::CancelReply { .. } => Some(None),
            LiveEvent::Left { reason, .. } => Some(Some(*reason)),
            _ => None,
        })
        .await;
    assert_eq!(left, Some(LeaveReason::Interrupted));
}

#[tokio::test]
async fn an_approval_shows_waits_and_the_resumed_run_is_read() {
    let mut h = Harness::start(true, false);
    let (reply, _) = h.send_reply("Email Ana that I'm late").await;
    h.session.reply_event(
        reply,
        RunEvent::RunStarted {
            run_id: "run-1".into(),
            session_id: "chat".into(),
        },
    );
    h.text(reply, "I'll send it once you say so.");
    let approval = Approval {
        app: "Fastmail".into(),
        action: "send email".into(),
        fields: Vec::new(),
    };
    h.session.reply_event(
        reply,
        RunEvent::DeviceQuery {
            query_id: "q1".into(),
            query: DeviceQuery::Approve {
                approval: approval.clone(),
            },
        },
    );
    h.finish(reply, "waiting");
    let (id, _) = h.play().await;
    h.session.played(id);
    let asked = h
        .wait(|e| match e {
            LiveEvent::Approval {
                reply,
                run_id,
                query_id,
                approval,
            } => Some((*reply, run_id.clone(), query_id.clone(), approval.clone())),
            LiveEvent::Toast { text } => panic!("unexpected toast {text}"),
            _ => None,
        })
        .await;
    assert_eq!(asked, (reply, Some("run-1".into()), "q1".into(), approval));
    h.phase(LivePhase::Waiting).await;
    h.session.approval_answered(reply);
    h.phase(LivePhase::Thinking).await;
    h.text(reply, "Sent.");
    h.finish(reply, "completed");
    let (id, audio) = h.play().await;
    assert_eq!(audio, "Sent.");
    h.session.played(id);
    h.phase(LivePhase::Listening).await;
}

#[tokio::test]
async fn the_resumed_runs_events_alone_resume_after_an_approval() {
    let mut h = Harness::start(true, false);
    let (reply, _) = h.send_reply("Email Ana").await;
    h.session.reply_event(
        reply,
        RunEvent::DeviceQuery {
            query_id: "q1".into(),
            query: DeviceQuery::Approve {
                approval: Approval {
                    app: "Fastmail".into(),
                    action: "send email".into(),
                    fields: Vec::new(),
                },
            },
        },
    );
    h.finish(reply, "waiting");
    h.phase(LivePhase::Waiting).await;
    h.text(reply, "Done. ");
    h.phase(LivePhase::Thinking).await;
    let (_, audio) = h.play().await;
    assert_eq!(audio, "Done.");
}

#[tokio::test]
async fn a_line_the_server_cannot_voice_still_shows() {
    let mut h = Harness::with(true, false, true);
    let (reply, _) = h.send_reply("Hi").await;
    h.text(reply, "Hello there. ");
    h.finish(reply, "completed");
    let (id, text, audio) = h
        .wait(|e| match e {
            LiveEvent::Play {
                id, text, audio, ..
            } => Some((*id, text.clone(), audio.clone())),
            _ => None,
        })
        .await;
    assert_eq!(text, "Hello there.");
    assert!(audio.is_empty());
    let view = h
        .wait(|e| match e {
            LiveEvent::View { view } if view.voice_unavailable => Some(view.clone()),
            _ => None,
        })
        .await;
    assert_eq!(view.line.as_deref(), Some("Hello there."));
    h.session.played(id);
    h.phase(LivePhase::Listening).await;
}

#[tokio::test]
async fn a_failed_reply_says_how() {
    let mut h = Harness::start(true, false);
    let (reply, _) = h.send_reply("Hello").await;
    h.session.reply_failed(reply, ReplyFailure::NotSent);
    h.toast("Could not send").await;
    h.phase(LivePhase::Listening).await;

    let (reply, _) = h.send_reply("Hello again").await;
    h.session.reply_event(
        reply,
        RunEvent::ToolStarted {
            name: "search".into(),
            call_id: "c1".into(),
            description: String::new(),
        },
    );
    h.session.reconnecting(reply);
    let view = h
        .wait(|e| match e {
            LiveEvent::View { view } if view.step.as_deref() == Some("Reconnecting") => {
                Some(view.clone())
            }
            _ => None,
        })
        .await;
    assert_eq!(view.phase, LivePhase::Thinking);
    h.session.reply_failed(reply, ReplyFailure::NotSent);
    h.toast("Beep couldn't finish").await;
    h.phase(LivePhase::Listening).await;

    let (reply, _) = h.send_reply("Once more").await;
    h.session.reply_failed(reply, ReplyFailure::ConnectionLost);
    h.toast("Connection lost").await;
}

#[tokio::test]
async fn the_screen_connects_before_it_listens() {
    let (tx, mut events) = unbounded_channel();
    // Kept, so the fake can hand its sinks over.
    let (sinks, _transcriptions) = unbounded_channel();
    struct Slow(UnboundedSender<HeardSink>);
    impl Speech for Slow {
        async fn transcribe(&self, mut audio: mpsc::Receiver<Vec<u8>>, heard: HeardSink) {
            // The test opens it once it saw the sound kept.
            let _ = self.0.send(heard.clone());
            let first = audio.recv().await;
            assert_eq!(first.as_deref(), Some(&[1u8, 0][..]));
            heard.hear(Hearing::Open).await;
            while audio.recv().await.is_some() {}
        }
        async fn say(&self, text: String) -> Result<Vec<u8>, BeepError> {
            Ok(text.into_bytes())
        }
        async fn filler(&self, _filler: Filler, _said: String) -> Result<Spoken, BeepError> {
            Err(BeepError::Internal)
        }
    }
    let session = LiveSession::start(
        Arc::new(Slow(sinks)),
        Arc::new(Events(tx)),
        LiveStart {
            send_on_pause: true,
            holding: false,
        },
    );
    // Sound pushed at once is kept for the transcription.
    session.push_audio(vec![1, 0]);
    let phases = timeout(Duration::from_secs(5), async {
        let mut seen = Vec::new();
        while let Some(event) = events.recv().await {
            if let LiveEvent::View { view } = event {
                seen.push(view.phase);
                if view.phase == LivePhase::Listening {
                    return seen;
                }
            }
        }
        seen
    });
    let seen = phases.await.expect("it listened in time");
    assert_eq!(seen.first(), Some(&LivePhase::Connecting));
    assert_eq!(seen.last(), Some(&LivePhase::Listening));
}

#[tokio::test]
async fn stopping_keeps_what_was_said_as_a_draft() {
    let mut h = Harness::start(false, false);
    h.say("Remind me at nine").await;
    h.session.stop();
    let left = h
        .wait(|e| match e {
            LiveEvent::Left { draft, reason, .. } => Some((draft.clone(), *reason)),
            _ => None,
        })
        .await;
    assert_eq!(left, (Some("Remind me at nine".into()), LeaveReason::Asked));
}

#[tokio::test]
async fn no_live_transcription_on_the_server_leaves_with_the_reason() {
    let mut h = Harness::start(false, true);
    let left = h
        .wait(|e| match e {
            LiveEvent::Left {
                failure, reason, ..
            } => Some((failure.clone(), *reason)),
            _ => None,
        })
        .await;
    assert_eq!(left.1, LeaveReason::Failed);
    assert!(left.0.is_some_and(|f| f.contains("no live transcription")));
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
    let (reply, _) = h.send_reply("Hi").await;
    h.text(reply, "Hello there. ");
    let (id, _) = h.play().await;
    h.session.played(id + 7);
    h.finish(reply, "completed");
    // Still speaking until the line that plays is done.
    h.session.played(id);
    h.phase(LivePhase::Listening).await;
}

#[tokio::test]
async fn a_tool_step_shows_while_thinking() {
    let mut h = Harness::start(true, false);
    let (reply, _) = h.send_reply("Is build-box out of disk?").await;
    h.session.reply_event(
        reply,
        RunEvent::ToolStarted {
            name: "shell".into(),
            call_id: "c1".into(),
            description: "Running df -h on build-box".into(),
        },
    );
    let view = h
        .wait(|e| match e {
            LiveEvent::View { view } if view.step.is_some() => Some(view.clone()),
            _ => None,
        })
        .await;
    assert_eq!(view.phase, LivePhase::Thinking);
    assert_eq!(view.step.as_deref(), Some("Running df -h on build-box"));
}

#[tokio::test]
async fn a_burst_of_loud_sound_does_not_crowd_out_a_stop() {
    let mut h = Harness::start(false, false);
    // The app flushes what it held while connecting all at once: far more
    // loud chunks than the command queue holds, before the session runs.
    let loud: Vec<u8> = [0xff, 0x7f].repeat(320);
    for _ in 0..200 {
        h.session.push_audio(loud.clone());
    }
    h.session.stop();
    let reason = h
        .wait(|e| match e {
            LiveEvent::Left { reason, .. } => Some(*reason),
            _ => None,
        })
        .await;
    assert_eq!(reason, LeaveReason::Asked);
}
