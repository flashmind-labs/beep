use super::*;
use std::collections::VecDeque;

fn key(session: &str) -> SessionKey {
    SessionKey {
        machine_id: "build-box".into(),
        agent: "claude".into(),
        session: session.into(),
    }
}

fn outbox() -> (tempfile::TempDir, PathBuf, Outbox) {
    let dir = tempfile::tempdir().expect("a temporary folder");
    let file = dir.path().join("outbox.json");
    let outbox = Outbox::load(file.clone());
    (dir, file, outbox)
}

fn push(outbox: &Outbox, session: &str, text: &str) -> u64 {
    outbox
        .with(|held| held.push(&key(session), Kind::Steer, text.into(), Vec::new(), None))
        .0
}

fn texts(outbox: &Outbox, session: &str) -> Vec<String> {
    outbox.read(|held| {
        held.rows(&key(session))
            .into_iter()
            .map(|row| row.text)
            .collect()
    })
}

/// What the fake machine answers, one per call, and what it was sent.
#[derive(Default)]
struct Fake {
    sends: VecDeque<Result<Vec<String>>>,
    checks: VecDeque<Result<bool>>,
    sent: Vec<String>,
    checked: Vec<String>,
    waits: usize,
    changes: Vec<Changed>,
}

impl Link for Fake {
    async fn send(&mut self, message: &Message) -> Result<Vec<String>> {
        let answer = self.sends.pop_front().unwrap_or(Ok(Vec::new()));

        if answer.is_ok() {
            self.sent.push(message.text.clone());
        }

        answer
    }

    async fn took(&mut self, message: &Message) -> Result<bool> {
        self.checked.push(message.text.clone());
        self.checks.pop_front().unwrap_or(Ok(false))
    }

    async fn reachable(&mut self) {
        self.waits += 1;
    }

    fn changed(&mut self, changed: Changed) {
        self.changes.push(changed);
    }
}

#[tokio::test]
async fn messages_go_in_the_order_typed_after_the_machine_comes_back() {
    let (_dir, _, outbox) = outbox();
    push(&outbox, "s1", "first");
    push(&outbox, "s1", "second");
    let mut link = Fake {
        sends: VecDeque::from([Err(DesktopError::Offline("build-box".into()))]),
        ..Fake::default()
    };

    flush(&outbox, &key("s1"), &mut link).await;

    assert_eq!(link.sent, ["first", "second"]);
    assert_eq!(link.waits, 1);
    assert!(texts(&outbox, "s1").is_empty());
    let sent: Vec<&str> = link
        .changes
        .iter()
        .flat_map(|c| c.sent.iter().map(|row| row.text.as_str()))
        .collect();
    assert_eq!(sent, ["first", "second"]);
}

#[tokio::test]
async fn a_send_the_link_dropped_on_is_read_before_it_goes_again() {
    let (_dir, _, outbox) = outbox();
    push(&outbox, "s1", "first");
    push(&outbox, "s1", "second");
    let mut link = Fake {
        sends: VecDeque::from([Err(DesktopError::Unsure("build-box".into()))]),
        checks: VecDeque::from([Ok(true)]),
        ..Fake::default()
    };

    flush(&outbox, &key("s1"), &mut link).await;

    // The machine had "first": only "second" goes again.
    assert_eq!(link.checked, ["first"]);
    assert_eq!(link.sent, ["second"]);
}

#[tokio::test]
async fn a_refused_message_keeps_its_reason_and_the_next_goes() {
    let (_dir, _, outbox) = outbox();
    push(&outbox, "s1", "first");
    push(&outbox, "s1", "second");
    let mut link = Fake {
        sends: VecDeque::from([Err(DesktopError::BadRequest("refused".into()))]),
        ..Fake::default()
    };

    flush(&outbox, &key("s1"), &mut link).await;

    assert_eq!(link.sent, ["second"]);
    let rows = outbox.read(|held| held.rows(&key("s1")));
    assert_eq!(rows.len(), 1);
    assert_eq!(
        (rows[0].state, rows[0].reason.as_deref()),
        ("failed", Some("refused"))
    );
}

#[tokio::test]
async fn the_window_that_sent_is_answered_by_the_first_try() {
    let (_dir, _, outbox) = outbox();
    let (reply, answer) = oneshot::channel();
    let (behind, later) = oneshot::channel();
    outbox.with(|held| {
        held.push(
            &key("s1"),
            Kind::Steer,
            "first".into(),
            Vec::new(),
            Some(reply),
        );
        held.push(
            &key("s1"),
            Kind::Steer,
            "second".into(),
            Vec::new(),
            Some(behind),
        );
    });
    // Sent windows show their own rows.
    assert!(texts(&outbox, "s1").is_empty());
    let mut link = Fake {
        sends: VecDeque::from([Err(DesktopError::Lost("build-box".into()))]),
        ..Fake::default()
    };

    flush(&outbox, &key("s1"), &mut link).await;

    assert!(matches!(answer.await, Ok(Answer::Waiting)));
    assert!(matches!(later.await, Ok(Answer::Waiting)));
}

#[test]
fn a_new_message_waits_behind_one_not_sent_and_one_flusher_runs() {
    let (_dir, _, outbox) = outbox();
    let (_, first) =
        outbox.with(|held| held.push(&key("s1"), Kind::Steer, "a".into(), Vec::new(), None));
    let (_, second) =
        outbox.with(|held| held.push(&key("s1"), Kind::Steer, "b".into(), Vec::new(), None));
    let (_, other) =
        outbox.with(|held| held.push(&key("s2"), Kind::Steer, "c".into(), Vec::new(), None));
    assert!(first);
    assert!(!second);
    assert!(other);

    let begun = outbox
        .with(|held| held.begin(&key("s1")))
        .expect("a message to send");
    assert_eq!(begun.text, "a");
    // One being sent cannot be taken back; one waiting can.
    assert!(outbox.with(|held| held.take(begun.id)).is_err());
    let b = outbox.read(|held| held.rows(&key("s1"))[1].id);
    let (taken, changed) = outbox.with(|held| held.take(b)).expect("taken");
    assert_eq!(taken.text, "b");
    assert_eq!(changed.rows.len(), 1);
}

#[test]
fn kept_messages_come_back_after_a_restart_and_a_cut_send_is_checked() {
    let (_dir, file, outbox) = outbox();
    push(&outbox, "s1", "first");
    push(&outbox, "s1", "second");
    outbox.with(|held| held.begin(&key("s1")));
    outbox.sync();
    drop(outbox);

    let again = Outbox::load(file);
    assert_eq!(texts(&again, "s1"), ["first", "second"]);
    assert_eq!(again.read(Held::waiting), [key("s1")]);
    let first = again
        .with(|held| held.begin(&key("s1")))
        .expect("a message to send");
    assert!(first.unsure);
    assert_eq!(push(&again, "s1", "third"), 2);
}

#[test]
fn a_dropped_send_counts_as_taken_when_the_session_holds_it() {
    let entry = |kind, text: &str| AgentEntry {
        kind,
        text: text.into(),
        tool: None,
    };
    let user = |text| entry(AgentEntryKind::User {}, text);
    let agent = |text| entry(AgentEntryKind::Agent {}, text);
    let none: [String; 0] = [];

    assert!(!holds(
        &none,
        &[user("run the tests"), agent("done"), user("fix it")],
        "run the tests"
    ));
    assert!(holds(
        &none,
        &[agent("done"), user("run the tests")],
        "run the tests"
    ));
    assert!(holds(&["run the tests".to_string()], &[], "run the tests"));
    assert!(!holds(&none, &[user("x")], ""));
}
