use super::*;

fn turn(words: &str) -> Turn {
    let mut t = Turn::default();
    t.hear(words);
    t
}

#[test]
fn words_come_in_as_deltas() {
    let mut t = turn("Remind me");
    t.hear(" to call");
    assert_eq!(t.text(), "Remind me to call");
    assert!(t.is_open());
}

#[test]
fn a_phrase_that_is_a_command_is_taken_out() {
    let mut t = turn("Book a table for four.");
    assert_eq!(t.close(), Closed::Kept);
    t.hear(" Stop.");
    assert_eq!(t.close(), Closed::Command(Command::Stop));
    assert_eq!(t.text(), "Book a table for four.");
}

#[test]
fn a_command_after_a_sentence_without_a_pause_still_counts() {
    let mut t = turn("Text Sam that I'm late. Stop.");
    assert_eq!(t.close(), Closed::Command(Command::Stop));
    assert_eq!(t.text(), "Text Sam that I'm late.");
}

#[test]
fn a_command_word_inside_a_sentence_is_kept() {
    let mut t = turn("Stop the server on build-box");
    assert_eq!(t.close(), Closed::Kept);
    assert_eq!(t.text(), "Stop the server on build-box");
}

#[test]
fn a_send_at_the_end_is_not_part_of_the_text() {
    let mut t = turn("Buy milk, send.");
    assert!(t.ends_in_send());
    assert_eq!(t.text(), "Buy milk");
    assert_eq!(t.close(), Closed::Command(Command::Send));
    assert_eq!(t.text(), "Buy milk");
}

#[test]
fn nothing_new_closes_nothing() {
    let mut t = Turn::default();
    assert_eq!(t.close(), Closed::Nothing);
    t.hear("   ");
    assert_eq!(t.close(), Closed::Nothing);
}
