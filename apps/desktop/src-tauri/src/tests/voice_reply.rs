use super::*;

#[test]
fn a_reply_that_failed_or_never_started_is_not_ok() {
    let done = |status: &str| Outcome {
        started: true,
        status: Some(status.into()),
        ..Outcome::default()
    };
    assert!(done("completed").ok());
    assert!(done("cancelled").ok());
    assert!(done("waiting").ok() && done("waiting").waits());
    assert!(!done("timed_out").ok());
    let errored = Outcome {
        failed: Some("internal error".into()),
        ..done("completed")
    };
    assert!(!errored.ok());
    // A stream that ended with neither a status nor an error.
    assert!(!Outcome::default().ok());
}

#[test]
fn a_reply_is_repeated_sentence_by_sentence() {
    assert_eq!(
        sentences("Two emails need a reply. Alex asks about Thursday."),
        vec!["Two emails need a reply.", "Alex asks about Thursday."]
    );
    assert!(sentences("  ").is_empty());
}
