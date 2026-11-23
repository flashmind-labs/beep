use super::*;
use crate::lenient;

#[test]
fn requests_read_by_position() {
    let named = rmp_serde::to_vec(&Request::Answer {
        tool: "claude".into(),
        text: "abc".into(),
    })
    .expect("encodes");
    #[derive(Serialize)]
    enum Tuple {
        Answer(&'static str, &'static str),
    }
    let positional = rmp_serde::to_vec(&Tuple::Answer("claude", "abc")).expect("encodes");
    assert_eq!(named, positional);
}

#[test]
fn a_waiting_state_round_trips() {
    let state = State::Waiting {
        url: "https://auth.openai.com/codex/device".into(),
        code: Some("ABCD-12345".into()),
        asks_code: false,
    };
    let bytes = rmp_serde::to_vec(&state).expect("encodes");
    let read: State = lenient::from_slice(&bytes).expect("decodes");
    assert_eq!(read, state);
    assert!(read.running());
}

#[test]
fn tools_go_by_name() {
    assert_eq!("opencode".parse::<Tool>().ok(), Some(Tool::Opencode));
    assert_eq!(Tool::Claude.to_string(), "claude");
    assert_eq!("agy".parse::<Tool>().ok(), Some(Tool::Agy));
    assert!("example".parse::<Tool>().is_err());
}
