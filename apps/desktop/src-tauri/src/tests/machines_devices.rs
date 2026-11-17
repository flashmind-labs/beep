use super::*;
use beep_proto::node::device::TouchPhase;

fn frame(n: u8) -> Frame {
    Frame {
        jpeg: vec![n],
        width: 2,
        height: 4,
    }
}

#[test]
fn only_the_newest_frame_waits_while_the_window_draws() {
    let start = Instant::now();
    let mut screen = Screen::new();

    assert_eq!(screen.arrived(frame(1), start), Some(frame(1)));
    assert_eq!(screen.arrived(frame(2), start), None);
    assert_eq!(screen.arrived(frame(3), start), None);
    assert_eq!(screen.drawn(start), Some(frame(3)));
    assert_eq!(screen.drawn(start), None);
    assert_eq!(screen.arrived(frame(4), start), Some(frame(4)));
}

#[test]
fn a_frame_goes_anyway_once_the_window_stalls() {
    let start = Instant::now();
    let mut screen = Screen::new();

    assert!(screen.arrived(frame(1), start).is_some());
    assert_eq!(screen.arrived(frame(2), start + DRAW_STALL), Some(frame(2)));
}

#[test]
fn a_frame_goes_as_its_size_then_the_jpeg() {
    let InvokeResponseBody::Raw(bytes) = ScreenEvent::Frame(frame(9)).body().unwrap() else {
        panic!("a frame goes raw");
    };
    assert_eq!(bytes, [2, 0, 0, 0, 4, 0, 0, 0, 9]);
}

#[tokio::test]
async fn frames_and_acknowledgements_run_while_input_is_blocked() {
    let attached = Attached::default();
    let view = Uuid::new_v4();
    let (input, up) = mpsc::channel(UP_QUEUE);
    let (drawn, draws) = watch::channel(());
    attached.follow(view, "window", DeviceControls { input, drawn });
    let input = attached.get(&view, "device").unwrap().input;
    let text = || Input::Text {
        text: "a".repeat(MAX_TEXT),
    };

    for _ in 0..UP_QUEUE {
        input.try_send(text()).unwrap();
    }

    let link = beep_proto::node::local_link();
    let (tx, rx) = link.caller;
    // Keep the other direction alive without reading it, so input backs up.
    let (mut node_tx, _node_rx) = link.node;
    let (shown, mut frames) = mpsc::unbounded_channel();
    let following = follow_screen(
        tx,
        rx,
        up,
        draws,
        |event| {
            shown.send(event).unwrap();
            Ok(())
        },
        "Connection lost.",
    );
    tokio::pin!(following);
    assert!(futures::poll!(&mut following).is_pending());

    while input.try_send(text()).is_ok() {}
    assert_eq!(input.capacity(), 0);

    node_tx.send(&Reply::Frame(frame(1))).await.unwrap();
    assert!(futures::poll!(&mut following).is_pending());
    assert_eq!(frames.try_recv().unwrap(), ScreenEvent::Frame(frame(1)));

    node_tx.send(&Reply::Frame(frame(2))).await.unwrap();
    assert!(futures::poll!(&mut following).is_pending());
    assert!(frames.try_recv().is_err());

    attached
        .get(&view, "device")
        .unwrap()
        .drawn
        .send_replace(());
    assert!(futures::poll!(&mut following).is_pending());
    assert_eq!(frames.try_recv().unwrap(), ScreenEvent::Frame(frame(2)));
    assert_eq!(input.capacity(), 0);

    // An in-flight input sender must not keep the view alive after detach.
    attached.detach(&view);
    assert!(matches!(
        futures::poll!(&mut following),
        std::task::Poll::Ready(Ok(()))
    ));
}

#[test]
fn touches_stay_on_the_screen() {
    let touch = |x, y| Input::Touch {
        phase: TouchPhase::Move,
        x,
        y,
    };
    assert_eq!(checked(touch(-0.5, 1.5)), [touch(0.0, 1.0)]);
    assert!(checked(touch(f32::NAN, 0.5)).is_empty());
}

#[test]
fn long_text_goes_in_pieces_on_character_boundaries() {
    assert_eq!(pieces("abcdé", 5), ["abcd", "é"]);
    assert_eq!(pieces("", 5), Vec::<&str>::new());
    assert!(
        checked(Input::Text {
            text: String::new()
        })
        .is_empty()
    );
}

#[test]
fn input_reads_from_the_window_json() {
    let input: Input = serde_json::from_str(r#"{"Button":{"button":"Home"}}"#).unwrap();
    assert_eq!(
        input,
        Input::Button {
            button: beep_proto::node::device::Button::Home
        }
    );
}
