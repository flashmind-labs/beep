use super::*;
use crate::agents::kept_folder::{IMAGES, image_file};
use std::collections::VecDeque;
use std::fs;

fn key(session: &str) -> SessionKey {
    SessionKey {
        machine_id: "build-box".into(),
        agent: "claude".into(),
        session: session.into(),
    }
}

fn outbox() -> (tempfile::TempDir, PathBuf, Outbox) {
    let dir = tempfile::tempdir().expect("a temporary folder");
    let kept = dir.path().join("outbox");
    let outbox = Outbox::load(kept.clone());
    (dir, kept, outbox)
}

fn draft(text: &str) -> Draft {
    Draft {
        how: Kind::Steer,
        text: text.into(),
        images: Vec::new(),
        seen: None,
    }
}

fn image(name: &str) -> PastedImage {
    PastedImage {
        name: name.into(),
        mime: "image/png".into(),
        data: "aGVsbG8=".into(),
    }
}

fn push(outbox: &Outbox, session: &str, text: &str) -> u64 {
    outbox
        .with(|held| held.push(&key(session), draft(text), None))
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
        held.push(&key("s1"), draft("first"), Some(reply));
        held.push(&key("s1"), draft("second"), Some(behind));
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

#[tokio::test]
async fn a_message_behind_one_waiting_for_the_machine_is_answered_at_once_and_shown() {
    let (_dir, _, outbox) = outbox();
    let first = push(&outbox, "s1", "first");
    outbox.with(|held| {
        held.begin(&key("s1"));
        held.wait(first, false)
    });

    let (reply, answer) = oneshot::channel();
    outbox.with(|held| held.push(&key("s1"), draft("second"), Some(reply)));

    assert!(matches!(answer.await, Ok(Answer::Waiting)));
    assert_eq!(texts(&outbox, "s1"), ["first", "second"]);
}

#[test]
fn a_window_that_stopped_waiting_hands_its_message_to_the_outbox() {
    let (_dir, _, outbox) = outbox();
    let (reply, _answer) = oneshot::channel();
    let (id, _) = outbox.with(|held| held.push(&key("s1"), draft("slow"), Some(reply)));
    assert!(texts(&outbox, "s1").is_empty());

    let changed = outbox
        .with(|held| held.hand_over(id))
        .expect("the window waited");
    assert_eq!(changed.rows.len(), 1);
    assert_eq!(texts(&outbox, "s1"), ["slow"]);
}

#[test]
fn a_new_message_waits_behind_one_not_sent_and_one_flusher_runs() {
    let (_dir, _, outbox) = outbox();
    let (_, first) = outbox.with(|held| held.push(&key("s1"), draft("a"), None));
    let (_, second) = outbox.with(|held| held.push(&key("s1"), draft("b"), None));
    let (_, other) = outbox.with(|held| held.push(&key("s2"), draft("c"), None));
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
fn stop_all_drains_what_waits_at_once_and_leaves_the_one_being_sent() {
    let (_dir, _, outbox) = outbox();
    push(&outbox, "s1", "a");
    push(&outbox, "s1", "b");
    push(&outbox, "s1", "c");
    push(&outbox, "s2", "other");
    outbox.with(|held| held.begin(&key("s1")));

    let (taken, changed) = outbox.with(|held| held.drain(&key("s1")));

    let taken: Vec<&str> = taken.iter().map(|m| m.text.as_str()).collect();
    assert_eq!(taken, ["b", "c"]);
    assert_eq!(changed.rows.len(), 1);
    assert_eq!(texts(&outbox, "s2"), ["other"]);
}

#[test]
fn every_message_kept_from_before_a_restart_is_checked_before_it_goes_again() {
    let (_dir, kept, outbox) = outbox();
    push(&outbox, "s1", "first");
    push(&outbox, "s1", "second");
    outbox.with(|held| held.begin(&key("s1")));
    outbox.sync();
    drop(outbox);

    let again = Outbox::load(kept);
    assert_eq!(texts(&again, "s1"), ["first", "second"]);
    assert_eq!(again.read(Held::waiting), [key("s1")]);
    let first = again
        .with(|held| held.begin(&key("s1")))
        .expect("a message to send");
    let second = again
        .with(|held| held.begin(&key("s1")))
        .expect("another message to send");
    // Waiting on disk does not mean nothing went: the file lags the sends.
    assert!(first.unsure);
    assert!(second.unsure);
    assert_eq!(push(&again, "s1", "third"), 2);
}

#[test]
fn images_are_kept_in_their_own_files_and_go_with_their_message() {
    let (_dir, kept, outbox) = outbox();
    let with_image = Draft {
        images: vec![image("shot.png")],
        ..draft("look")
    };
    let (id, _) = outbox.with(|held| held.push(&key("s1"), with_image, None));
    outbox.sync();

    let messages = fs::read_to_string(kept.join(MESSAGES)).expect("the messages' file");
    assert!(messages.contains("shot.png"));
    assert!(!messages.contains("aGVsbG8="));
    let file = kept.join(IMAGES).join(image_file(id, 0));
    assert_eq!(fs::read_to_string(&file).expect("the image"), "aGVsbG8=");

    let again = Outbox::load(kept.clone());
    let (message, _) = again.with(|held| held.take(id)).expect("taken");
    assert_eq!(message.images[0].data, "aGVsbG8=");
    again.sync();
    assert!(!file.exists());
}

fn entry(kind: AgentEntryKind, text: &str) -> AgentEntry {
    AgentEntry {
        kind,
        text: text.into(),
        tool: None,
        sent_at: None,
    }
}

fn user(text: &str) -> AgentEntry {
    entry(AgentEntryKind::User {}, text)
}

fn agent(text: &str) -> AgentEntry {
    entry(AgentEntryKind::Agent {}, text)
}

fn kept(text: &str, images: &[&str], seen: Option<Seen>) -> Message {
    Message {
        id: 0,
        machine_id: "build-box".into(),
        agent: "claude".into(),
        session: "s1".into(),
        how: Kind::Steer,
        text: text.into(),
        images: images.iter().map(|name| image(name).into()).collect(),
        state: Stage::Waiting,
        unsure: true,
        seen,
    }
}

fn log<'a>(queued: &'a [String], earlier: u64, entries: &'a [AgentEntry]) -> Log<'a> {
    Log {
        queued,
        earlier,
        entries,
    }
}

#[test]
fn a_dropped_send_counts_as_taken_only_for_what_came_after_it() {
    let seen = |entries, queued: &[&str]| {
        Some(Seen {
            entries,
            queued: queued.iter().map(|q| q.to_string()).collect(),
        })
    };
    let entries = [user("yes"), agent("done")];

    // An older "yes" before the send is not this one.
    assert!(!holds(
        &log(&[], 0, &entries),
        &kept("yes", &[], seen(2, &[]))
    ));
    // A "yes" after it is.
    let after = [user("yes"), agent("done"), user("yes")];
    assert!(holds(&log(&[], 0, &after), &kept("yes", &[], seen(2, &[]))));
    // The same words inside a longer message are not it.
    let longer = [user("yes please")];
    assert!(!holds(
        &log(&[], 0, &longer),
        &kept("yes", &[], seen(0, &[]))
    ));
    // One more queued copy than the window saw is it; the same count is not.
    let one = ["yes".to_string()];
    assert!(holds(&log(&one, 0, &[]), &kept("yes", &[], seen(0, &[]))));
    assert!(!holds(
        &log(&one, 0, &[]),
        &kept("yes", &[], seen(0, &["yes"]))
    ));
    // Without what the window saw, only the newest user entry counts.
    assert!(holds(
        &log(&[], 0, &[agent("x"), user("yes")]),
        &kept("yes", &[], None)
    ));
    assert!(!holds(
        &log(&[], 0, &[user("yes"), user("no")]),
        &kept("yes", &[], None)
    ));
}

#[test]
fn an_image_only_message_is_found_by_its_image_lines() {
    let message = kept("", &["shot.png"], Some(Seen::default()));
    let queued = ["[Image: shot.png]".to_string()];

    assert!(holds(&log(&queued, 0, &[]), &message));
    assert!(holds(&log(&[], 5, &[user("[image: image-abc]")]), &message));
    assert!(!holds(&log(&[], 5, &[user("hello")]), &message));
}
