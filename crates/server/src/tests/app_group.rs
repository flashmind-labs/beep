use super::*;

#[test]
fn only_a_whole_mention_calls_beep() {
    assert!(mentions_beep("@beep where should we eat?"));
    assert!(mentions_beep("what do you think @Beep?"));
    assert!(mentions_beep("hey @BEEP"));
    assert!(!mentions_beep("beep me later"));
    assert!(!mentions_beep("@beeper is a phone"));
}
