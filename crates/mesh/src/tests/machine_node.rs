use super::*;

#[test]
fn the_newest_is_neither_installed_nor_restarted() {
    let done = updated(update::Event::Newest {
        version: "0.1.24".into(),
    })
    .expect("an answer");
    assert_eq!((done.from.as_str(), done.to.as_str()), ("0.1.24", "0.1.24"));
    assert!(!done.restarting);
}

#[test]
fn a_failure_keeps_the_nodes_reason() {
    let e = updated(update::Event::Failed {
        reason: "no build for this machine".into(),
    })
    .expect_err("a failure");
    assert_eq!(e.to_string(), "no build for this machine");
}
