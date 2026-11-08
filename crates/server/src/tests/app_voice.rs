use super::*;

#[test]
fn a_user_runs_two_live_transcriptions_at_most() {
    let slots = LiveSlots::default();
    let first = slots.take("alice").expect("a first slot");
    let _second = slots.take("alice").expect("a second slot");
    assert!(slots.take("alice").is_none(), "a third waits");
    let _theirs = slots.take("bob").expect("another user has their own");
    drop(first);
    assert!(slots.take("alice").is_some(), "an ended one frees its slot");
}
