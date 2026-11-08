use super::*;

#[test]
fn chips_in_another_script_than_the_reply_go() {
    let reply = "The museum opens at nine and is quieter before noon.";
    assert!(same_script("Remind me at 8:30", reply));
    assert!(!same_script("チケットの値段を調べて", reply));
    assert!(!same_script("Louvre の行き方を教えて", reply));
    let japanese = "美術館は九時に開き、昼前は空いています。";
    assert!(same_script("Louvre の行き方を教えて", japanese));
    assert!(!same_script("Find the ticket price", japanese));
    assert!(same_script("9:30", reply), "no letters says nothing");
}

#[test]
fn a_reply_ending_on_a_question_gets_answers() {
    assert_eq!(
        Kind::of("Sure.\n\nWhat time should I remind you? Say \"tomorrow 9am\"."),
        Kind::Answers
    );
    assert_eq!(Kind::of("Want me to book it?"), Kind::Answers);
    assert_eq!(Kind::of("**Lunch or dinner?**"), Kind::Answers);
    assert_eq!(Kind::of("要我帮你预订吗？"), Kind::Answers);
    assert_eq!(Kind::of("هل أحجز الطاولة؟"), Kind::Answers);
    assert_eq!(Kind::of("Which one?\n\nIt opens at nine."), Kind::FollowUps);
    assert_eq!(
        Kind::of("Here it is: https://example.com/search?q=beep"),
        Kind::FollowUps
    );
}
