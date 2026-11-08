use super::notes::parse_notes;
use super::*;
use crate::apps::feeds::mail::Mailbox;

#[test]
fn only_the_last_read_to_end_condenses() {
    let claim = |source| Claim::take_id("release-last-test".into(), source).unwrap();
    let gmail = claim(Source::Mail(Mailbox::Gmail));
    let calendar = claim(Source::Calendar(Calendar::Google));
    assert!(!gmail.release_last());
    assert!(calendar.release_last());
    assert!(!running().iter().any(|(id, _)| id == "release-last-test"));
}

#[test]
fn a_cut_off_answer_keeps_its_whole_notes() {
    let notes = parse_notes(
            "{\"notes\": [{\"note\": \"Flew to Tokyo.\"}, {\"note\": \"Speaks Spanish.\"}, {\"date\": \"2024-",
        )
        .unwrap();
    assert_eq!(notes.len(), 2);
    assert_eq!(notes[1].text, "Speaks Spanish.");
    assert!(parse_notes("{\"notes\": [{\"note\": \"Fle").is_err());
}
