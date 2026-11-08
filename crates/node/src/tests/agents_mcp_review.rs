use super::*;
use beep_proto::node::agent::{CodeFinding, CodeVerdict, FindingSeverity};
use serde_json::json;

#[test]
fn an_agents_review_reads_typed_and_answers_as_json() {
    let given = json!({
        "verdict": "changes",
        "summary": "One bug.",
        "findings": [{
            "file": "src/limit.rs",
            "line": 42,
            "severity": "high",
            "summary": "Off by one",
            "scenario": "A limit of 0 lets one request through."
        }, {
            "file": "README.md",
            "line": null,
            "severity": "low",
            "summary": "Stale example",
            "scenario": "Copying it fails."
        }]
    });
    let review: SubmittedReview = serde_json::from_value(given).expect("reads");
    assert_eq!(review.verdict, CodeVerdict::Changes);
    assert_eq!(
        review.findings[0],
        CodeFinding {
            file: "src/limit.rs".into(),
            line: Some(42),
            severity: FindingSeverity::High,
            summary: "Off by one".into(),
            scenario: "A limit of 0 lets one request through.".into(),
        }
    );
    assert_eq!(review.findings[1].line, None);
    let bad = json!({ "verdict": "maybe", "summary": "?", "findings": [] });
    assert!(serde_json::from_value::<SubmittedReview>(bad).is_err());

    let run = ReviewRun {
        id: "b4e0d2".into(),
        review: Some(review),
        ..ReviewRun::default()
    };
    let Reply::Done { text, failed } = standing(run) else {
        panic!("not done");
    };
    assert!(!failed);
    let read: serde_json::Value = serde_json::from_str(&text).expect("JSON");
    assert_eq!(read["state"], "done");
    assert_eq!(read["review"]["findings"][0]["severity"], "high");
    let Reply::Done { text, failed } = standing(ReviewRun {
        id: "b4e0d2".into(),
        error: Some("The review was stopped.".into()),
        ..ReviewRun::default()
    }) else {
        panic!("not done");
    };
    assert!(failed);
    assert!(text.contains("\"state\":\"failed\""), "{text}");
}
