use super::*;

fn decide(name: &str, args: Value) -> Result<Triage, String> {
    let decision = decisions()
        .into_iter()
        .find(|decision| decision.name == name)
        .ok_or("no such decision")?;
    (decision.parse)(&args)
}

fn inform(urgency: Urgency) -> Triage {
    Triage::Inform {
        summary: "Ana asked for a review.".to_owned(),
        why: "A person is waiting.".to_owned(),
        urgency,
        refs: vec![1],
    }
}

#[test]
fn decisions_read_their_arguments() {
    assert_eq!(
        decide(
            "inform_beep",
            json!({"summary": " Ana asked for a review. ", "why": "A person is waiting.", "urgency": "now", "refs": [1, "#2", "[#3]", "x"]})
        ),
        Ok(Triage::Inform {
            summary: "Ana asked for a review.".to_owned(),
            why: "A person is waiting.".to_owned(),
            urgency: Urgency::Now,
            refs: vec![1, 2, 3],
        })
    );
    assert_eq!(
        decide("ignore", json!({"reason": "Newsletters."})),
        Ok(Triage::Ignore {
            reason: "Newsletters.".to_owned()
        })
    );
    assert!(
        decide(
            "inform_beep",
            json!({"summary": "x", "why": "y", "urgency": "soon", "refs": []})
        )
        .is_err()
    );
    assert!(decide("ignore", json!({"reason": "  "})).is_err());
}

#[test]
fn only_an_inform_now_with_a_wake_left_wakes() {
    assert_eq!(route(&inform(Urgency::Now), true), Route::Wake);
    assert_eq!(route(&inform(Urgency::Now), false), Route::Defer);
    assert_eq!(route(&inform(Urgency::Later), true), Route::Defer);
    let ignore = Triage::Ignore {
        reason: "Spam.".to_owned(),
    };
    assert_eq!(route(&ignore, true), Route::Drop);
}
