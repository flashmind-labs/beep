use super::*;

fn at(t: Instant, ms: u64) -> Instant {
    t + Duration::from_millis(ms)
}

fn sends(effects: &[Effect]) -> Option<&str> {
    effects.iter().find_map(|e| match e {
        Effect::Send { text, .. } => Some(text.as_str()),
        _ => None,
    })
}

fn says(live: &mut Live, words: &str, now: Instant) {
    live.input(Input::Words(words.into()), now);
}

#[test]
fn a_pause_sends_in_pause_mode() {
    let t = Instant::now();
    let (mut live, start) = Live::listening(EndMode::Pause, t);
    assert_eq!(start, vec![Effect::Transcribe]);
    says(&mut live, "What's the weather in Girona", at(t, 100));
    assert_eq!(
        live.input(Input::Tick, at(t, 500)),
        vec![Effect::Soon(Some(Duration::from_millis(1100)))]
    );
    // Talking again takes the countdown away.
    says(&mut live, " this weekend", at(t, 700));
    let out = live.input(Input::Tick, at(t, 700 + 1500));
    assert_eq!(
        sends(&out),
        Some("What's the weather in Girona this weekend")
    );
    assert_eq!(live.phase(), Phase::Thinking);
}

#[test]
fn a_pause_never_sends_in_word_mode() {
    let t = Instant::now();
    let (mut live, _) = Live::listening(EndMode::Word, t);
    says(&mut live, "Remind me to call the plumber", at(t, 100));

    for ms in (200..10_000).step_by(100) {
        assert_eq!(sends(&live.input(Input::Tick, at(t, ms))), None);
    }

    says(&mut live, ", send.", at(t, 10_000));
    let out = live.input(Input::Tick, at(t, 10_000) + PHRASE_PAUSE);
    assert_eq!(sends(&out), Some("Remind me to call the plumber"));
}

#[test]
fn send_sends_at_once_in_pause_mode_too() {
    let t = Instant::now();
    let (mut live, _) = Live::listening(EndMode::Pause, t);
    says(&mut live, "Buy milk. Send.", at(t, 100));
    live.input(Input::Sound, at(t, 1000));
    let out = live.input(Input::Tick, at(t, 1000) + PHRASE_PAUSE);
    assert_eq!(sends(&out), Some("Buy milk."));
}

#[test]
fn speaking_done_goes_back_to_listening() {
    let t = Instant::now();
    let (mut live, _) = Live::listening(EndMode::Pause, t);
    says(&mut live, "Hi", at(t, 100));
    live.input(Input::Tick, at(t, 100) + PAUSE_SEND);
    assert_eq!(live.phase(), Phase::Thinking);
    assert_eq!(
        live.input(Input::ReplyReady, at(t, 4000)),
        vec![Effect::Speak, Effect::Transcribe]
    );
    assert_eq!(live.phase(), Phase::Speaking);
    assert_eq!(
        live.input(Input::SpeakDone, at(t, 9000)),
        vec![Effect::Transcribe]
    );
    assert_eq!(live.phase(), Phase::Listening);
}

#[test]
fn a_tap_interrupts_speech() {
    let t = Instant::now();
    let (mut live, _) = Live::speaking(EndMode::Pause, t);
    let out = live.input(Input::Tap, at(t, 500));
    assert_eq!(
        out,
        vec![
            Effect::StopSpeaking,
            Effect::CancelReply,
            Effect::Transcribe,
            Effect::Toast(Toast::Stopped)
        ]
    );
    assert_eq!(live.phase(), Phase::Listening);
}

#[test]
fn saying_stop_interrupts_speech_but_other_words_do_not() {
    let t = Instant::now();
    let (mut live, _) = Live::speaking(EndMode::Pause, t);
    // Beep's own voice leaking through is ignored.
    says(&mut live, "two emails need a reply", at(t, 100));
    assert!(
        live.input(Input::Tick, at(t, 100) + PHRASE_PAUSE)
            .is_empty()
    );
    assert_eq!(live.phase(), Phase::Speaking);
    says(&mut live, "Stop the server", at(t, 2000));
    assert!(
        live.input(Input::Tick, at(t, 2000) + PHRASE_PAUSE)
            .is_empty()
    );
    says(&mut live, "Stop.", at(t, 4000));
    let out = live.input(Input::Tick, at(t, 4000) + PHRASE_PAUSE);
    assert!(out.contains(&Effect::StopSpeaking));
    assert_eq!(live.phase(), Phase::Listening);
    // What was heard while speaking is not part of the next message.
    assert_eq!(live.text(), "");
}

#[test]
fn tap_or_escape_leaves_and_keeps_a_draft() {
    let t = Instant::now();
    let (mut live, _) = Live::listening(EndMode::Word, t);
    assert_eq!(
        live.input(Input::Tap, at(t, 100)),
        vec![
            Effect::EndTranscription,
            Effect::Leave {
                draft: None,
                why: Leaving::Asked
            }
        ]
    );
    assert!(live.is_over());
    assert!(live.input(Input::Tap, at(t, 200)).is_empty());

    let (mut live, _) = Live::listening(EndMode::Word, t);
    says(&mut live, "Remind me at nine", at(t, 100));
    let out = live.input(Input::Escape, at(t, 300));
    assert!(out.contains(&Effect::Leave {
        draft: Some("Remind me at nine".into()),
        why: Leaving::Asked
    }));
}

#[test]
fn two_quiet_minutes_end_live_mode() {
    let t = Instant::now();
    let (mut live, _) = Live::listening(EndMode::Word, t);
    says(&mut live, "Something half", at(t, 100));
    assert!(live.input(Input::Tick, at(t, 60_000)).is_empty());
    let out = live.input(Input::Tick, at(t, 100) + QUIET_LIMIT);
    assert!(out.contains(&Effect::Leave {
        draft: Some("Something half".into()),
        why: Leaving::Quiet
    }));
}

#[test]
fn holding_sends_on_release_and_another_key_cancels() {
    let t = Instant::now();
    let (mut live, _) = Live::holding(EndMode::Word, t);
    says(&mut live, "What's on tomorrow", at(t, 300));
    // Pauses while holding never send, even in pause mode.
    live.set_mode(EndMode::Pause);
    assert!(live.input(Input::Tick, at(t, 5000)).is_empty());
    let out = live.input(Input::HoldEnd, at(t, 5100));
    assert_eq!(sends(&out), Some("What's on tomorrow"));

    let (mut live, _) = Live::holding(EndMode::Word, t);
    says(&mut live, "Never", at(t, 300));
    let out = live.input(Input::HoldCancel, at(t, 400));
    assert!(out.contains(&Effect::Toast(Toast::Cancelled)));
    assert_eq!(live.phase(), Phase::Listening);
}

#[test]
fn holding_while_speaking_interrupts_and_listens() {
    let t = Instant::now();
    let (mut live, _) = Live::speaking(EndMode::Pause, t);
    let out = live.input(Input::HoldStart, at(t, 100));
    assert_eq!(
        out,
        vec![
            Effect::StopSpeaking,
            Effect::CancelReply,
            Effect::Transcribe
        ]
    );
    assert_eq!(live.phase(), Phase::Holding);
}

#[test]
fn a_tap_while_thinking_cancels_the_reply() {
    let t = Instant::now();
    let (mut live, _) = Live::listening(EndMode::Word, t);
    says(&mut live, "Hi send", at(t, 100));
    live.input(Input::Tick, at(t, 100) + PHRASE_PAUSE);
    assert_eq!(live.phase(), Phase::Thinking);
    assert_eq!(
        live.input(Input::Tap, at(t, 2000)),
        vec![
            Effect::CancelReply,
            Effect::Transcribe,
            Effect::Toast(Toast::Stopped)
        ]
    );
    assert_eq!(live.phase(), Phase::Listening);
    assert!(live.input(Input::ReplyReady, at(t, 3000)).is_empty());
}

#[test]
fn a_reply_with_nothing_to_say_goes_back_to_listening() {
    let t = Instant::now();
    let (mut live, _) = Live::listening(EndMode::Word, t);
    live.input(Input::Words("what time is it send".into()), t);
    let out = live.input(Input::Tick, at(t, 1300));
    assert_eq!(sends(&out), Some("what time is it"));
    assert_eq!(live.phase(), Phase::Thinking);
    assert_eq!(
        live.input(Input::SpeakDone, at(t, 4000)),
        vec![Effect::Transcribe]
    );
    assert_eq!(live.phase(), Phase::Listening);
}

/// Sends "Hi" in word mode, leaving `live` thinking.
fn thinking(t: Instant) -> Live {
    let (mut live, _) = Live::listening(EndMode::Word, t);
    says(&mut live, "Hi send", at(t, 100));
    let out = live.input(Input::Tick, at(t, 100) + PHRASE_PAUSE);
    assert_eq!(sends(&out), Some("Hi"));
    // Still listening while Beep thinks, for "stop".
    assert!(out.contains(&Effect::Transcribe));
    assert!(!out.contains(&Effect::EndTranscription));
    live
}

#[test]
fn saying_stop_while_thinking_cancels_the_reply() {
    let t = Instant::now();
    let mut live = thinking(t);
    // Other words while thinking are not a new message.
    says(&mut live, "and the weather", at(t, 2000));
    assert!(
        live.input(Input::Tick, at(t, 2000) + PHRASE_PAUSE)
            .is_empty()
    );
    assert_eq!(live.phase(), Phase::Thinking);
    says(&mut live, "Stop.", at(t, 5000));
    let out = live.input(Input::Tick, at(t, 5000) + PHRASE_PAUSE);
    assert_eq!(
        out,
        vec![
            Effect::CancelReply,
            Effect::Transcribe,
            Effect::Toast(Toast::Stopped)
        ]
    );
    assert_eq!(live.phase(), Phase::Listening);
    assert_eq!(live.text(), "");
}

#[test]
fn saying_stop_while_speaking_cancels_the_reply() {
    let t = Instant::now();
    let mut live = thinking(t);
    live.input(Input::ReplyReady, at(t, 3000));
    says(&mut live, "Stop", at(t, 4000));
    let out = live.input(Input::Tick, at(t, 4000) + PHRASE_PAUSE);
    assert_eq!(
        out,
        vec![
            Effect::StopSpeaking,
            Effect::CancelReply,
            Effect::Transcribe,
            Effect::Toast(Toast::Stopped)
        ]
    );
}

#[test]
fn leaving_mid_reply_cancels_it_but_not_after() {
    let t = Instant::now();
    let mut live = thinking(t);
    live.input(Input::ReplyReady, at(t, 3000));
    let out = live.input(Input::Escape, at(t, 3500));
    assert_eq!(
        out,
        vec![
            Effect::EndTranscription,
            Effect::StopSpeaking,
            Effect::CancelReply,
            Effect::Leave {
                draft: None,
                why: Leaving::Asked
            }
        ]
    );

    let (mut live, _) = Live::listening(EndMode::Word, t);
    let out = live.input(Input::End(Leaving::Interrupted), at(t, 100));
    assert!(!out.contains(&Effect::CancelReply));
    assert!(out.contains(&Effect::Leave {
        draft: None,
        why: Leaving::Interrupted
    }));
}

#[test]
fn holding_while_thinking_cancels_and_listens() {
    let t = Instant::now();
    let mut live = thinking(t);
    let out = live.input(Input::HoldStart, at(t, 2000));
    assert_eq!(out, vec![Effect::CancelReply, Effect::Transcribe]);
    assert_eq!(live.phase(), Phase::Holding);
}

#[test]
fn a_reply_failing_while_speaking_goes_back_to_listening() {
    let t = Instant::now();
    let mut live = thinking(t);
    live.input(Input::ReplyReady, at(t, 3000));
    assert_eq!(
        live.input(Input::ReplyFailed, at(t, 4000)),
        vec![Effect::Transcribe]
    );
    assert_eq!(live.phase(), Phase::Listening);
    // The message is in the chat already.
    assert_eq!(live.text(), "");
}

#[test]
fn a_message_not_sent_comes_back_without_sending_again() {
    let t = Instant::now();
    let (mut live, _) = Live::listening(EndMode::Pause, t);
    says(&mut live, "Book the dentist", at(t, 100));
    let out = live.input(Input::Tick, at(t, 100) + PAUSE_SEND);
    assert_eq!(sends(&out), Some("Book the dentist"));
    assert_eq!(
        live.input(Input::NotSent, at(t, 3000)),
        vec![Effect::Transcribe, Effect::Show]
    );
    assert_eq!(live.text(), "Book the dentist");
    // A pause alone does not send it again.
    assert_eq!(sends(&live.input(Input::Tick, at(t, 9000))), None);
    says(&mut live, " for Friday", at(t, 9000));
    let out = live.input(Input::Tick, at(t, 9000) + PAUSE_SEND);
    assert_eq!(sends(&out), Some("Book the dentist for Friday"));
}

#[test]
fn cancel_stops_the_reply_and_puts_the_message_back() {
    let t = Instant::now();
    let mut live = thinking(t);
    live.input(Input::ReplyReady, at(t, 3000));
    assert_eq!(
        live.input(Input::Cancel, at(t, 3500)),
        vec![
            Effect::StopSpeaking,
            Effect::CancelReply,
            Effect::Transcribe,
            Effect::Toast(Toast::Cancelled),
            Effect::Show,
        ]
    );
    assert_eq!(live.phase(), Phase::Listening);
    assert_eq!(live.text(), "Hi");
    assert!(!live.is_over());

    // With no reply going on, Cancel leaves like Escape.
    let (mut live, _) = Live::listening(EndMode::Word, t);
    says(&mut live, "Half a thought", at(t, 100));
    assert!(
        live.input(Input::Cancel, at(t, 200))
            .contains(&Effect::Leave {
                draft: Some("Half a thought".into()),
                why: Leaving::Asked
            })
    );
}

#[test]
fn stop_while_talking_sends() {
    let t = Instant::now();
    let (mut live, _) = Live::listening(EndMode::Word, t);
    says(&mut live, "Book a table for four. Stop.", at(t, 100));
    let out = live.input(Input::Tick, at(t, 100) + PHRASE_PAUSE);
    assert_eq!(sends(&out), Some("Book a table for four."));

    // With nothing said, it sends nothing.
    let (mut live, _) = Live::listening(EndMode::Word, t);
    says(&mut live, "Stop.", at(t, 100));
    let out = live.input(Input::Tick, at(t, 100) + PHRASE_PAUSE);
    assert_eq!(sends(&out), None);
    assert_eq!(live.phase(), Phase::Listening);
}

#[test]
fn letting_go_after_stop_sends() {
    let t = Instant::now();
    let (mut live, _) = Live::listening(EndMode::Word, t);
    live.input(Input::HoldStart, at(t, 100));
    says(&mut live, "Book a table. Stop.", at(t, 200));
    let out = live.input(Input::HoldEnd, at(t, 300));
    assert_eq!(sends(&out), Some("Book a table."));
}

#[test]
fn an_empty_hold_sends_nothing_now_or_later() {
    let t = Instant::now();
    let (mut live, _) = Live::listening(EndMode::Pause, t);
    live.input(Input::HoldStart, at(t, 100));
    let out = live.input(Input::HoldEnd, at(t, 200));
    // A fresh transcription: late words of the hold do not count.
    assert_eq!(out, vec![Effect::Transcribe, Effect::Show]);
    assert_eq!(live.phase(), Phase::Listening);
}

#[test]
fn hold_to_talk_listens_only_while_held() {
    let t = Instant::now();
    let (mut live, start) = Live::hold_to_talk(EndMode::Pause, t);
    assert_eq!(start, vec![Effect::Transcribe]);
    assert!(live.hold_only());
    says(&mut live, "What's on tomorrow", at(t, 300));
    let out = live.input(Input::HoldEnd, at(t, 600));
    assert_eq!(sends(&out), Some("What's on tomorrow"));
    // "stop" still works while Beep thinks and speaks.
    assert!(out.contains(&Effect::Transcribe));
    live.input(Input::ReplyReady, at(t, 2000));
    assert_eq!(
        live.input(Input::SpeakDone, at(t, 5000)),
        vec![Effect::EndTranscription]
    );
    assert_eq!(live.phase(), Phase::Listening);
    assert_eq!(
        live.input(Input::HoldStart, at(t, 8000)),
        vec![Effect::Transcribe]
    );
    assert_eq!(live.phase(), Phase::Holding);
    assert_eq!(
        live.input(Input::HoldEnd, at(t, 8300)),
        vec![Effect::EndTranscription, Effect::Show]
    );
    // A tap between holds leaves.
    assert!(
        live.input(Input::Tap, at(t, 9000))
            .contains(&Effect::Leave {
                draft: None,
                why: Leaving::Asked
            })
    );
}

#[test]
fn a_tap_mid_reply_in_hold_to_talk_stops_listening() {
    let t = Instant::now();
    let (mut live, _) = Live::hold_to_talk(EndMode::Pause, t);
    says(&mut live, "Hi", at(t, 300));
    live.input(Input::HoldEnd, at(t, 600));
    assert_eq!(
        live.input(Input::Tap, at(t, 1000)),
        vec![
            Effect::CancelReply,
            Effect::EndTranscription,
            Effect::Toast(Toast::Stopped)
        ]
    );
}

#[test]
fn an_approval_waits_without_the_quiet_limit_then_goes_on() {
    let t = Instant::now();
    let mut live = thinking(t);
    live.input(Input::ReplyReady, at(t, 3000));
    assert_eq!(
        live.input(Input::Approval, at(t, 4000)),
        vec![Effect::Transcribe]
    );
    assert_eq!(live.phase(), Phase::Waiting);

    for s in (5..=600).step_by(5) {
        assert!(live.input(Input::Tick, at(t, s * 1000)).is_empty());
    }

    assert_eq!(live.phase(), Phase::Waiting);
    assert_eq!(
        live.input(Input::Answered, at(t, 601_000)),
        vec![Effect::Transcribe]
    );
    assert_eq!(live.phase(), Phase::Thinking);
    live.input(Input::SpeakDone, at(t, 605_000));
    assert_eq!(live.phase(), Phase::Listening);
    // Answered, the quiet limit counts again.
    let out = live.input(Input::Tick, at(t, 605_000) + QUIET_LIMIT);
    assert!(out.contains(&Effect::Leave {
        draft: None,
        why: Leaving::Quiet
    }));
}

#[test]
fn while_waiting_a_new_message_drops_the_approval() {
    let t = Instant::now();
    let mut live = thinking(t);
    live.input(Input::Approval, at(t, 3000));
    assert_eq!(live.phase(), Phase::Waiting);
    says(&mut live, "Actually do something else send", at(t, 9000));
    let out = live.input(Input::Tick, at(t, 9000) + PHRASE_PAUSE);
    assert_eq!(sends(&out), Some("Actually do something else"));
    assert!(live.input(Input::Answered, at(t, 12_000)).is_empty());
    live.input(Input::SpeakDone, at(t, 15_000));
    assert_eq!(live.phase(), Phase::Listening);
}
