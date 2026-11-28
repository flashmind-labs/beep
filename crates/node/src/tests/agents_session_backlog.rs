use super::*;

fn prompt(text: &str) -> Prompt {
    Prompt::from(text.to_owned())
}

#[test]
fn a_stop_not_from_the_user_takes_back_the_steer_in_flight() {
    let mut backlog = Backlog::default();
    backlog.push(prompt("queued"), false);
    backlog.push(prompt("steer"), true);
    backlog.begin_steer(prompt("sent"));

    backlog.cancel(false);

    assert_eq!(backlog.waiting(), 0);
    assert!(backlog.take_steering().unwrap().dropped);
}

#[test]
fn a_stop_from_the_user_keeps_the_steers() {
    let mut backlog = Backlog::default();
    backlog.push(prompt("queued"), false);
    backlog.push(prompt("steer"), true);
    backlog.begin_steer(prompt("sent"));

    backlog.cancel(true);

    assert_eq!(backlog.messages(), ["sent", "steer"]);
    assert!(!backlog.take_steering().unwrap().dropped);
}

#[test]
fn what_waits_is_listed_steering_first_then_steers_then_queue() {
    let mut backlog = Backlog::default();
    backlog.push(prompt("queued"), false);
    backlog.push(prompt("steer"), true);
    backlog.begin_steer(prompt("sent"));

    assert_eq!(backlog.messages(), ["sent", "steer", "queued"]);
}
