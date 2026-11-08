use super::*;

fn call(name: &str, success: bool) -> Call {
    Call {
        name: name.into(),
        success,
    }
}

#[test]
fn a_shown_map_or_a_schedule_settles_its_check() {
    let calls = [call("open_maps", true), call("schedule_once", false)];
    let draft = Draft {
        asked: "Where should I go?",
        reply: "Head to the Gucci flagship.",
        calls: &calls,
        scheduled: &[],
        can_map: true,
        can_schedule: true,
    };
    let settled: Vec<_> = CHECKS
        .iter()
        .filter(|check| (check.settled)(&draft))
        .map(|check| check.key)
        .collect();
    assert_eq!(settled, ["map"]);
}

#[test]
fn a_routine_is_not_asked_to_schedule() {
    let draft = Draft {
        asked: "Check whether .bitcoin is on sale.",
        reply: "Not on sale yet, I will keep checking.",
        calls: &[],
        scheduled: &[],
        can_map: true,
        can_schedule: false,
    };
    let promised = CHECKS.iter().find(|check| check.key == "promised");
    assert!((promised.expect("promised check").settled)(&draft));
}
