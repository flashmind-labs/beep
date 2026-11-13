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
