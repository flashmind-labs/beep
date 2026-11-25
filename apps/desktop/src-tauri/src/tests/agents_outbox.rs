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
    let outbox = Outbox::load(kept.clone(), dir.path().join("cache"));
    (dir, kept, outbox)
}

fn draft(text: &str) -> Draft {
    Draft {
        how: Kind::Steer,
        text: text.into(),
        images: Vec::new(),
        files: Vec::new(),
        root: String::new(),
        mentions: Vec::new(),
        resume: None,
        seen: None,
        reading: None,
    }
}

/// A message to a session that ended.
fn resuming(text: &str) -> Draft {
    Draft {
        resume: Some(Resume::default()),
        ..draft(text)
    }
}

/// A message with files to upload first.
fn with_files(text: &str, names: &[&str]) -> Draft {
    Draft {
        root: "/srv/project".into(),
        files: names.iter().map(|name| image(name).into()).collect(),
        mentions: vec!["src/lib.rs".into()],
        ..draft(text)
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
    outbox.with(|held| held.push(&key(session), draft(text))).0
}

fn texts(outbox: &Outbox, session: &str) -> Vec<String> {
    outbox.read(|held| {
        held.rows(&key(session))
            .into_iter()
            .map(|row| row.text)
            .collect()
    })
}

/// The rows of a session that are still the outbox's to send: not the sent ones, which stay until
/// the session lists them.
fn unsent(outbox: &Outbox, session: &str) -> Vec<Row> {
    outbox.read(|held| {
        held.rows(&key(session))
            .into_iter()
            .filter(|row| row.state != "sent")
            .collect()
    })
}

/// What the fake machine answers, one per call, and what it was sent.
#[derive(Default)]
struct Fake {
    sends: VecDeque<Result<Vec<String>>>,
    uploads: VecDeque<Result<()>>,
    resumes: VecDeque<Result<()>>,
    /// How many times the session was resumed, and the sessions messages went to.
    resumed: usize,
    went_to: Vec<SessionId>,
    configured: Vec<String>,
    orders: VecDeque<Result<()>>,
    checks: VecDeque<Result<bool>>,
    sent: Vec<String>,
    ordered: Vec<OrderKind>,
    /// The sessions the orders went to.
    ordered_on: Vec<SessionId>,
    events: Vec<String>,
    checked: Vec<String>,
    waits: usize,
    changes: Vec<Changed>,
}

impl Link for Fake {
    async fn resume(&mut self, _: &Message) -> Result<Resumed> {
        self.resumes.pop_front().unwrap_or(Ok(()))?;
        self.resumed += 1;
        self.events.push("resume".into());
        Ok(Resumed {
            session: "s2".into(),
            row: None,
        })
    }

    async fn configure(&mut self, message: &Message, pick: &Pick) -> Result<()> {
        self.configured.push(format!("{:?}", pick.effort));
        self.events.push(format!("configure:{}", message.session));
        Ok(())
    }

    async fn upload(&mut self, message: &Message) -> Result<Vec<String>> {
        self.uploads.pop_front().unwrap_or(Ok(()))?;
        let names: Vec<String> = message.files.iter().map(|f| f.name.clone()).collect();
        self.events.push(format!("up:{}", names.join(",")));
        Ok(names.iter().map(|name| format!("/cache/{name}")).collect())
    }

    async fn send(&mut self, message: &Message) -> Result<Vec<String>> {
        let answer = self.sends.pop_front().unwrap_or(Ok(Vec::new()));

        if answer.is_ok() {
            self.went_to.push(message.session.clone());
            self.sent.push(message.text.clone());
            self.events.push(format!("msg:{}", message.text));
        }

        answer
    }

    async fn order(&mut self, order: &Order) -> Result<()> {
        let answer = self.orders.pop_front().unwrap_or(Ok(()));

        if answer.is_ok() {
            self.ordered.push(order.kind.clone());
            self.ordered_on.push(order.key.session.clone());
            self.events.push(format!("ord:{:?}", order.kind));
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
    assert!(unsent(&outbox, "s1").is_empty());
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
    let rows = unsent(&outbox, "s1");
    assert_eq!(rows.len(), 1);
    assert_eq!(
        (rows[0].state, rows[0].reason.as_deref()),
        ("failed", Some("refused"))
    );
}

#[tokio::test]
async fn a_message_is_shown_at_once_and_its_image_keys_come_with_the_sent_row() {
    let (_dir, _, outbox) = outbox();
    push(&outbox, "s1", "first");
    // No window waits on a first try: the row shows from the start.
    assert_eq!(texts(&outbox, "s1"), ["first"]);
    let mut link = Fake {
        sends: VecDeque::from([Ok(vec!["k1".into()])]),
        ..Fake::default()
    };

    flush(&outbox, &key("s1"), &mut link).await;

    let sent: Vec<&Row> = link.changes.iter().flat_map(|c| &c.sent).collect();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].keys, ["k1"]);
}

#[tokio::test]
async fn a_sent_message_stays_as_a_sent_row_until_a_push_of_its_session_lists_it() {
    let (_dir, _, outbox) = outbox();
    push(&outbox, "s1", "first");
    let mut link = Fake::default();

    flush(&outbox, &key("s1"), &mut link).await;

    let rows = outbox.read(|held| held.rows(&key("s1")));
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].state, "sent");
    assert_eq!(link.changes.last().map(|c| c.rows.len()), Some(1));

    // A push from before the machine took it, and one of another session, list nothing.
    assert!(
        outbox
            .with(|held| held.settled(&key("s1"), &log(&[], 0, &[user("older")])))
            .is_none()
    );
    let queued = ["first".to_string()];
    assert!(
        outbox
            .with(|held| held.settled(&key("s2"), &log(&queued, 0, &[])))
            .is_none()
    );
    assert_eq!(texts(&outbox, "s1"), ["first"]);

    // The session's queue lists it: the row goes, in a change the windows hear.
    let changed = outbox
        .with(|held| held.settled(&key("s1"), &log(&queued, 0, &[])))
        .expect("the row goes");
    assert!(changed.rows.is_empty());
    assert!(texts(&outbox, "s1").is_empty());
}

#[tokio::test]
async fn two_messages_sent_without_what_the_window_saw_both_settle_from_the_last_push() {
    let (_dir, _, outbox) = outbox();
    // A follow's push, before either message.
    outbox.with(|held| held.settled(&key("s1"), &log(&[], 0, &[user("older")])));
    push(&outbox, "s1", "one");
    push(&outbox, "s1", "two");
    let mut link = Fake::default();

    flush(&outbox, &key("s1"), &mut link).await;

    outbox
        .with(|held| {
            held.settled(
                &key("s1"),
                &log(&[], 0, &[user("older"), user("one"), user("two")]),
            )
        })
        .expect("the rows go");
    assert!(texts(&outbox, "s1").is_empty());
}

#[tokio::test]
async fn a_sent_message_a_push_listed_in_the_log_goes_too_and_stop_all_takes_back_the_rest() {
    let (_dir, _, outbox) = outbox();
    push(&outbox, "s1", "one");
    push(&outbox, "s1", "two");
    push(&outbox, "s1", "three");
    let mut link = Fake::default();

    flush(&outbox, &key("s1"), &mut link).await;

    assert_eq!(texts(&outbox, "s1"), ["one", "two", "three"]);
    // Delivered already: it is in the log, not in the queue.
    outbox
        .with(|held| held.settled(&key("s1"), &log(&[], 0, &[user("two")])))
        .expect("the row goes");
    assert_eq!(texts(&outbox, "s1"), ["one", "three"]);

    let (taken, changed) = outbox.with(|held| held.drain(&key("s1")));
    let taken: Vec<&str> = taken.iter().map(|m| m.text.as_str()).collect();
    assert_eq!(taken, ["one", "three"]);
    assert!(changed.rows.is_empty());
}

#[tokio::test]
async fn sent_rows_go_when_the_machine_drops_its_steers() {
    let (_dir, _, outbox) = outbox();
    push(&outbox, "s1", "one");
    push(&outbox, "s2", "other");
    let mut link = Fake::default();
    flush(&outbox, &key("s1"), &mut link).await;
    flush(&outbox, &key("s2"), &mut link).await;

    let changed = outbox
        .with(|held| held.dropped(&key("s1")))
        .expect("the row goes");

    assert!(changed.rows.is_empty());
    assert_eq!(texts(&outbox, "s2"), ["other"]);
    assert!(outbox.with(|held| held.dropped(&key("s1"))).is_none());
}

#[tokio::test]
async fn a_plain_stop_takes_the_sent_prompts_off_the_list_but_not_the_steers() {
    let (_dir, _, outbox) = outbox();
    let s1 = key("s1");
    outbox.with(|held| {
        held.push(
            &s1,
            Draft {
                how: Kind::Prompt,
                ..draft("queued prompt")
            },
        );
        held.push(&s1, draft("steer"));
    });
    let mut link = Fake::default();
    flush(&outbox, &s1, &mut link).await;
    assert_eq!(texts(&outbox, "s1"), ["queued prompt", "steer"]);

    outbox.with(|held| held.push_order(&s1, OrderKind::Cancel, Instant::now(), None));
    flush(&outbox, &s1, &mut link).await;

    assert_eq!(link.ordered, [OrderKind::Cancel]);
    assert_eq!(texts(&outbox, "s1"), ["steer"]);
}

#[tokio::test]
async fn a_message_the_session_already_showed_leaves_no_sent_row_behind() {
    let (_dir, _, outbox) = outbox();
    let id = push(&outbox, "s1", "one");
    outbox.with(|held| held.begin(&key("s1")));

    outbox.with(|held| held.found(id)).expect("it goes");

    assert!(texts(&outbox, "s1").is_empty());
}

#[test]
fn a_user_entry_the_node_stored_without_the_pictures_is_still_the_message() {
    let message = kept("look", &["image.jpg"], Some(Seen::default()));

    assert!(holds(&log(&[], 0, &[user("look")]), &message));
    assert!(!holds(&log(&[], 0, &[user("looks")]), &message));
}

fn reading(text: &str, token: &str) -> Draft {
    Draft {
        reading: Some(token.into()),
        ..draft(text)
    }
}

fn read(text: &str, names: &[&str]) -> Read {
    Read {
        text: text.into(),
        images: Vec::new(),
        files: names.iter().map(|name| image(name)).collect(),
        mentions: Vec::new(),
        root: "/srv/project".into(),
    }
}

#[tokio::test]
async fn a_message_whose_files_are_read_is_kept_at_once_and_holds_back_those_behind_it() {
    let (_dir, _, outbox) = outbox();
    outbox.with(|held| held.push(&key("s1"), reading("with files", "t1")));
    push(&outbox, "s1", "plain");
    assert_eq!(texts(&outbox, "s1"), ["with files", "plain"]);
    let rows = outbox.read(|held| held.rows(&key("s1")));
    assert_eq!(rows[0].state, "reading");
    let mut link = Fake::default();

    flush(&outbox, &key("s1"), &mut link).await;

    assert!(
        link.sent.is_empty(),
        "nothing goes before the files are read"
    );

    let (changed, _, flusher) = outbox
        .with(|held| held.attach("t1", read("with files", &["a.txt"])))
        .expect("attached");
    assert!(flusher);
    assert_eq!(changed.rows[0].state, "waiting");
    flush(&outbox, &key("s1"), &mut link).await;

    assert_eq!(
        link.events,
        ["up:a.txt", "msg:with files\n\n@/cache/a.txt", "msg:plain"]
    );
}

#[test]
fn a_message_taken_while_its_files_are_read_cannot_be_completed_and_a_cancel_removes_it() {
    let (_dir, _, outbox) = outbox();
    let (id, _) = outbox.with(|held| held.push(&key("s1"), reading("a", "t1")));
    outbox.with(|held| held.push(&key("s1"), reading("b", "t2")));
    outbox.with(|held| held.take(id)).expect("taken");

    assert!(
        outbox
            .with(|held| held.attach("t1", read("a", &[])))
            .is_none()
    );
    assert!(outbox.with(|held| held.cancel("t2")).is_some());
    assert!(texts(&outbox, "s1").is_empty());
}

#[test]
fn a_message_still_reading_when_beep_quit_fails_with_its_words_kept() {
    let (dir, kept, outbox) = outbox();
    outbox.with(|held| held.push(&key("s1"), reading("my words", "t1")));
    outbox.sync();
    drop(outbox);

    let again = Outbox::load(kept, dir.path().join("cache"));

    let rows = again.read(|held| held.rows(&key("s1")));
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].state, "failed");
    assert_eq!(rows[0].text, "my words");
}

#[tokio::test]
async fn a_resume_with_no_words_resumes_and_applies_the_pick_and_sends_nothing() {
    let (_dir, _, outbox) = outbox();
    outbox.with(|held| held.push(&key("s1"), resuming("")));
    let mut link = Fake::default();

    flush(&outbox, &key("s1"), &mut link).await;

    assert_eq!(link.resumed, 1);
    assert!(link.sent.is_empty());
    assert!(outbox.read(|held| held.rows(&key("s2"))).is_empty());
    assert!(texts(&outbox, "s1").is_empty());
    assert!(outbox.read(|held| held.rows(&key("s1"))).is_empty());
}

#[tokio::test]
async fn a_refused_first_try_stays_as_a_failed_row() {
    let (_dir, _, outbox) = outbox();
    push(&outbox, "s1", "first");
    let mut link = Fake {
        sends: VecDeque::from([Err(DesktopError::BadRequest("refused".into()))]),
        ..Fake::default()
    };

    flush(&outbox, &key("s1"), &mut link).await;

    let rows = outbox.read(|held| held.rows(&key("s1")));
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].state, "failed");
}

#[test]
fn a_new_message_waits_behind_one_not_sent_and_one_flusher_runs() {
    let (_dir, _, outbox) = outbox();
    let (_, first) = outbox.with(|held| held.push(&key("s1"), draft("a")));
    let (_, second) = outbox.with(|held| held.push(&key("s1"), draft("b")));
    let (_, other) = outbox.with(|held| held.push(&key("s2"), draft("c")));
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
    let (dir, kept, outbox) = outbox();
    push(&outbox, "s1", "first");
    push(&outbox, "s1", "second");
    outbox.with(|held| held.begin(&key("s1")));
    outbox.sync();
    drop(outbox);

    let again = Outbox::load(kept, dir.path().join("cache"));
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
fn images_are_cached_and_go_with_their_message() {
    let (dir, kept, outbox) = outbox();
    let with_image = Draft {
        images: vec![image("shot.png")],
        ..draft("look")
    };
    let (id, _) = outbox.with(|held| held.push(&key("s1"), with_image));
    outbox.sync();

    let messages = fs::read_to_string(kept.join(MESSAGES)).expect("the messages' file");
    assert!(messages.contains("shot.png"));
    assert!(!messages.contains("aGVsbG8="));
    let cache = dir.path().join("cache");
    let file = cache.join(IMAGES).join(image_file(id, 0));
    assert_eq!(fs::read_to_string(&file).expect("the image"), "aGVsbG8=");
    assert!(!kept.join(IMAGES).exists());

    let again = Outbox::load(kept, cache);
    let (message, _) = again.with(|held| held.take(id)).expect("taken");
    assert_eq!(&*message.images[0].data, "aGVsbG8=");
    again.sync();
    assert!(!file.exists());
}

#[test]
fn an_image_queued_before_the_cache_move_can_still_be_loaded() {
    let (dir, kept, outbox) = outbox();
    let with_image = Draft {
        images: vec![image("shot.png")],
        ..draft("look")
    };
    let (id, _) = outbox.with(|held| held.push(&key("s1"), with_image));
    outbox.sync();

    let cache = dir.path().join("cache");
    let legacy = kept.join(IMAGES);
    fs::rename(cache.join(IMAGES), &legacy).unwrap();
    let again = Outbox::load(kept, cache);
    let (message, _) = again.with(|held| held.take(id)).expect("taken");
    assert_eq!(&*message.images[0].data, "aGVsbG8=");
    again.sync();
    assert!(!legacy.join(image_file(id, 0)).exists());
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
        files: Vec::new(),
        root: String::new(),
        mentions: Vec::new(),
        resume: None,
        resumed_from: None,
        state: Stage::Waiting,
        unsure: true,
        seen,
        reading: None,
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

#[test]
fn a_message_the_node_cut_is_still_found() {
    let long = "x".repeat(20 * 1024);
    let cut = user(&long[..16 * 1024]);
    let message = kept(&long, &[], None);

    assert!(holds(&log(&[], 0, &[cut]), &message));
    assert!(!holds(
        &log(&[], 0, &[user(&"y".repeat(16 * 1024))]),
        &message
    ));
}

#[tokio::test]
async fn coalesced_orders_precede_later_messages_across_a_degraded_link() {
    let (_dir, _, outbox) = outbox();
    let s1 = key("s1");
    let now = Instant::now();
    let (r1, a1) = oneshot::channel();
    let (r2, a2) = oneshot::channel();

    outbox.with(|held| {
        held.push_order(
            &s1,
            OrderKind::Configure {
                model: Some(ModelId::from("opus")),
                effort: None,
                fast: None,
            },
            now,
            Some(r1),
        );
        held.push_order(
            &s1,
            OrderKind::Configure {
                model: None,
                effort: Some(Effort::High),
                fast: Some(true),
            },
            now,
            Some(r2),
        );
        held.push(&s1, draft("run tests"));
    });

    // Two Configure orders before the prompt coalesce, superseding the first waiter immediately.
    assert_eq!(a1.await, Ok(OrderAnswer::Queued));

    let mut link = Fake {
        orders: VecDeque::from([Err(DesktopError::Lost("build-box".into()))]),
        ..Fake::default()
    };
    flush(&outbox, &s1, &mut link).await;

    assert_eq!(a2.await, Ok(OrderAnswer::Queued));
    assert_eq!(link.waits, 1);
    assert_eq!(
        link.ordered,
        [OrderKind::Configure {
            model: Some(ModelId::from("opus")),
            effort: Some(Effort::High),
            fast: Some(true),
        }]
    );
    assert_eq!(link.sent, ["run tests"]);
    assert_eq!(link.events.len(), 2);
    assert!(link.events[0].starts_with("ord:Configure"));
    assert_eq!(link.events[1], "msg:run tests");
}

#[tokio::test]
async fn close_preserves_drafts_when_refused_and_drains_once_accepted() {
    let (_dir, _, outbox) = outbox();
    let s1 = key("s1");
    let now = Instant::now();

    push(&outbox, "s1", "pending prompt");
    outbox.with(|held| {
        held.push_order(&s1, OrderKind::Cancel, now, None);
        let (_, changed, _) = held.push_order(&s1, OrderKind::Close, now, None);
        assert!(
            changed.is_none(),
            "drafts stay persisted until close succeeds"
        );
    });
    assert_eq!(texts(&outbox, "s1"), ["pending prompt"]);

    // `Close` runs ahead of the queued prompt, and if the node refuses `Close`, the prompt stays waiting.
    let step = outbox.with(|held| held.begin_step(&s1, now));
    let Some(Step::Order(close_order)) = step else {
        panic!("expected Close order first, got {step:?}");
    };
    assert_eq!(close_order.kind, OrderKind::Close);
    outbox.with(|held| {
        held.order_refused(close_order.id, "cannot close".into());
    });
    assert_eq!(texts(&outbox, "s1"), ["pending prompt"]);

    // Once Close succeeds, the queued prompt is drained without being sent.
    outbox.with(|held| {
        held.push_order(&s1, OrderKind::Close, now, None);
    });
    let mut ok_link = Fake {
        orders: VecDeque::from([
            Ok(()),
            Err(DesktopError::Api(ApiError::Conflict {
                message: "already answered".into(),
            })),
        ]),
        ..Fake::default()
    };
    let (reply, answer) = oneshot::channel();
    let s2 = key("s2");
    outbox.with(|held| {
        held.push_order(
            &s2,
            OrderKind::Answer {
                question: 7,
                reply: "allow".into(),
            },
            now,
            Some(reply),
        );
    });

    flush(&outbox, &s1, &mut ok_link).await;
    flush(&outbox, &s2, &mut ok_link).await;

    assert!(texts(&outbox, "s1").is_empty());
    assert!(ok_link.sent.is_empty(), "drained prompt must not be sent");
    assert_eq!(ok_link.ordered, [OrderKind::Close]);
    assert_eq!(answer.await, Ok(OrderAnswer::Sent));
}

#[tokio::test]
async fn configure_stays_ordered_around_queued_prompts_and_stops_do_not_replay() {
    let (_dir, _, outbox) = outbox();
    let s1 = key("s1");
    let now = Instant::now();

    outbox.with(|held| {
        held.push_order(
            &s1,
            OrderKind::Configure {
                model: Some("model-a".into()),
                effort: None,
                fast: None,
            },
            now,
            None,
        );
    });
    push(&outbox, "s1", "prompt for model a");
    outbox.with(|held| {
        held.push_order(
            &s1,
            OrderKind::Configure {
                model: Some("model-b".into()),
                effort: None,
                fast: None,
            },
            now,
            None,
        );
        held.push_order(
            &s1,
            OrderKind::Configure {
                model: Some("model-c".into()),
                effort: None,
                fast: None,
            },
            now,
            None,
        );
    });

    let mut link = Fake {
        orders: VecDeque::from([Ok(()), Ok(())]),
        sends: VecDeque::from([Ok(Vec::new())]),
        ..Fake::default()
    };
    flush(&outbox, &s1, &mut link).await;

    assert_eq!(
        link.events,
        [
            "ord:Configure { model: Some(ModelId(\"model-a\")), effort: None, fast: None }",
            "msg:prompt for model a",
            "ord:Configure { model: Some(ModelId(\"model-c\")), effort: None, fast: None }",
        ]
    );

    // A dropped ack on Cancel is treated as completed rather than replayed on a later turn.
    let s2 = key("s2");
    outbox.with(|held| {
        held.push_order(&s2, OrderKind::Cancel, now, None);
    });
    let mut unsure_link = Fake {
        orders: VecDeque::from([Err(DesktopError::Unsure("node".into()))]),
        ..Fake::default()
    };
    flush(&outbox, &s2, &mut unsure_link).await;
    assert_eq!(unsure_link.waits, 0, "Cancel must not wait to retry");
    assert!(outbox.with(|held| held.begin_step(&s2, now).is_none()));
}

#[tokio::test]
async fn files_upload_before_their_message_goes_and_a_second_message_waits_behind() {
    let (_dir, _, outbox) = outbox();
    outbox.with(|held| held.push(&key("s1"), with_files("Read these", &["a.txt"])));
    push(&outbox, "s1", "Then this");
    // Its row says what is being done, from the first change.
    assert_eq!(
        outbox.read(|held| held.rows(&key("s1")))[0].state,
        "waiting"
    );
    let mut link = Fake::default();

    flush(&outbox, &key("s1"), &mut link).await;

    assert_eq!(
        link.events,
        [
            "up:a.txt",
            "msg:Read these\n\n@src/lib.rs @/cache/a.txt",
            "msg:Then this"
        ]
    );
    let first = &link.changes[0].rows[0];
    assert_eq!(
        (first.state, first.text.as_str()),
        ("uploading", "Read these")
    );
    assert!(unsent(&outbox, "s1").is_empty());
}

#[tokio::test]
async fn every_change_takes_a_newer_revision_and_a_list_says_which_it_is_as_of() {
    let (_dir, _, outbox) = outbox();
    outbox.with(|held| held.push(&key("s1"), with_files("Read these", &["a.txt"])));
    push(&outbox, "s1", "Then this");
    let mut link = Fake::default();

    flush(&outbox, &key("s1"), &mut link).await;

    let revisions: Vec<u64> = link.changes.iter().map(|c| c.revision).collect();
    assert!(revisions.len() > 2, "{revisions:?}");
    assert!(
        revisions.windows(2).all(|pair| pair[0] < pair[1]),
        "{revisions:?}"
    );
    // A read taken now is as new as the last change, so an older reply is never drawn over it.
    let listed = outbox.read(|held| held.listed(&key("s1")));
    assert_eq!(Some(listed.revision), revisions.last().copied());
}

/// A link whose upload holds until released, and which says when it began.
struct Slow {
    inner: Fake,
    started: Option<oneshot::Sender<()>>,
    release: Option<oneshot::Receiver<()>>,
    /// Holds the resume the same way, and says when it began.
    resume_started: Option<oneshot::Sender<()>>,
    resume_release: Option<oneshot::Receiver<()>>,
    /// The same for applying the pick.
    configure_started: Option<oneshot::Sender<()>>,
    configure_release: Option<oneshot::Receiver<()>>,
    lanes: Vec<SessionKey>,
}

impl Link for Slow {
    async fn send(&mut self, message: &Message) -> Result<Vec<String>> {
        self.inner.send(message).await
    }

    async fn resume(&mut self, message: &Message) -> Result<Resumed> {
        if let Some(started) = self.resume_started.take() {
            let _ = started.send(());
        }

        if let Some(release) = self.resume_release.take() {
            let _ = release.await;
        }

        self.inner.resume(message).await
    }

    async fn configure(&mut self, message: &Message, pick: &Pick) -> Result<()> {
        if let Some(started) = self.configure_started.take() {
            let _ = started.send(());
        }

        if let Some(release) = self.configure_release.take() {
            let _ = release.await;
        }

        self.inner.configure(message, pick).await
    }

    async fn upload(&mut self, message: &Message) -> Result<Vec<String>> {
        if let Some(started) = self.started.take() {
            let _ = started.send(());
        }

        if let Some(release) = self.release.take() {
            let _ = release.await;
        }

        self.inner.upload(message).await
    }

    async fn order(&mut self, order: &Order) -> Result<()> {
        self.inner.order(order).await
    }

    async fn took(&mut self, message: &Message) -> Result<bool> {
        self.inner.took(message).await
    }

    async fn reachable(&mut self) {
        self.inner.reachable().await;
    }

    fn changed(&mut self, changed: Changed) {
        self.inner.changed(changed);
    }

    fn urgent(&mut self, key: &SessionKey) {
        self.lanes.push(key.clone());
    }
}

fn held_up(release: Option<oneshot::Receiver<()>>) -> (Slow, oneshot::Receiver<()>) {
    let (started, began) = oneshot::channel();
    let slow = Slow {
        inner: Fake::default(),
        started: Some(started),
        release,
        resume_started: None,
        resume_release: None,
        configure_started: None,
        configure_release: None,
        lanes: Vec::new(),
    };
    (slow, began)
}

#[tokio::test]
async fn a_stop_goes_out_while_a_file_uploads_and_the_message_keeps_its_place() {
    let (_dir, _, outbox) = outbox();
    let s1 = key("s1");
    outbox.with(|held| held.push(&s1, with_files("Read these", &["a.txt"])));
    push(&outbox, "s1", "Then this");
    let (release, held_until) = oneshot::channel();
    let (mut slow, began) = held_up(Some(held_until));
    let mut free = Fake::default();

    let flusher = flush(&outbox, &s1, &mut slow);
    let stop = async {
        began.await.expect("the upload began");
        let (_, _, flusher) =
            outbox.with(|held| held.push_order(&s1, OrderKind::Cancel, Instant::now(), None));
        assert!(!flusher, "the session's flusher is busy uploading");
        assert!(outbox.with(|held| held.start_urgent(&s1)));
        flush_urgent(&outbox, &s1, &mut free).await;

        assert_eq!(free.ordered, [OrderKind::Cancel]);
        assert!(free.sent.is_empty());
        release.send(()).expect("the upload waits");
    };
    tokio::join!(flusher, stop);

    // The message went after its upload, then the one behind it.
    assert_eq!(
        slow.inner.events,
        [
            "up:a.txt",
            "msg:Read these\n\n@src/lib.rs @/cache/a.txt",
            "msg:Then this"
        ]
    );
    assert!(slow.inner.ordered.is_empty());
}

#[tokio::test]
async fn an_order_kept_behind_a_message_is_given_an_urgent_lane_when_that_message_uploads() {
    let (_dir, _, outbox) = outbox();
    let s1 = key("s1");
    outbox.with(|held| {
        held.push(&s1, with_files("Read these", &["a.txt"]));
        held.push_order(&s1, OrderKind::StopAll, Instant::now(), None);
    });
    let (mut slow, _) = held_up(None);

    flush(&outbox, &s1, &mut slow).await;

    assert_eq!(slow.lanes, [s1]);
    assert_eq!(slow.inner.ordered, [OrderKind::StopAll]);
}

#[tokio::test]
async fn a_close_pressed_while_the_session_resumes_waits_and_closes_the_one_that_resumed() {
    let (_dir, _, outbox) = outbox();
    let s1 = key("s1");
    outbox.with(|held| held.push(&s1, resuming("first")));
    let (release, held_until) = oneshot::channel();
    let (mut slow, _) = held_up(None);
    let (began, resume_began) = oneshot::channel();
    slow.resume_started = Some(began);
    slow.resume_release = Some(held_until);

    let flusher = flush(&outbox, &s1, &mut slow);
    let close = async {
        resume_began.await.expect("the resume began");
        outbox.with(|held| held.push_order(&s1, OrderKind::Close, Instant::now(), None));
        // The session has no final id yet: no lane may send it to the ended one.
        assert!(!outbox.with(|held| held.start_urgent(&s1)));
        assert!(
            outbox
                .with(|held| held.begin_urgent(&s1, Instant::now()))
                .is_none()
        );
        release.send(()).expect("the resume waits");
    };
    tokio::join!(flusher, close);

    assert_eq!(slow.inner.ordered, [OrderKind::Close]);
    assert_eq!(slow.inner.ordered_on, ["s2"]);
    assert_eq!(slow.inner.went_to, ["s2"]);
}

/// A message to an ended session with a pick, whose session gets `order` while it resumes. The
/// order waits for the new session, goes out on the urgent lane while the pick is applied, and
/// the message must then never be sent.
async fn an_order_while_the_session_resumes_and_a_pick_applies(order: OrderKind) {
    let (_dir, _, outbox) = outbox();
    let s1 = key("s1");
    let pick = Pick {
        model: None,
        effort: Some(Effort::High),
        fast: None,
    };
    let message = Draft {
        resume: Some(Resume {
            pick: Some(pick),
            ..Resume::default()
        }),
        ..draft("first")
    };
    outbox.with(|held| held.push(&s1, message));
    let (release_resume, resume_held) = oneshot::channel();
    let (release_pick, pick_held) = oneshot::channel();
    let (mut slow, _) = held_up(None);
    let (resume_began, resume_started) = oneshot::channel();
    let (pick_began, pick_started) = oneshot::channel();
    slow.resume_started = Some(resume_began);
    slow.resume_release = Some(resume_held);
    slow.configure_started = Some(pick_began);
    slow.configure_release = Some(pick_held);
    let mut free = Fake::default();

    let flusher = flush(&outbox, &s1, &mut slow);
    let pressed = async {
        resume_started.await.expect("the resume began");
        outbox.with(|held| held.push_order(&s1, order.clone(), Instant::now(), None));
        release_resume.send(()).expect("the resume waits");
        pick_started.await.expect("the pick is being applied");
        // The lane runs now, on the new session, while the message still waits on its pick.
        flush_urgent(&outbox, &key("s2"), &mut free).await;
        release_pick.send(()).expect("the pick waits");
    };
    tokio::join!(flusher, pressed);

    assert_eq!(free.ordered, [order]);
    assert_eq!(free.ordered_on, ["s2"]);
    assert!(slow.inner.sent.is_empty(), "the message must not be sent");
    assert!(texts(&outbox, "s2").is_empty(), "no failed row is left");
}

#[tokio::test]
async fn a_close_while_the_session_resumes_takes_the_message_with_it_whatever_it_still_has_to_do() {
    an_order_while_the_session_resumes_and_a_pick_applies(OrderKind::Close).await;
}

#[tokio::test]
async fn a_stop_while_the_session_resumes_takes_back_what_was_sent_to_it() {
    an_order_while_the_session_resumes_and_a_pick_applies(OrderKind::Cancel).await;
}

#[tokio::test]
async fn a_close_during_an_upload_takes_the_uploading_message_with_the_session() {
    let (_dir, _, outbox) = outbox();
    let s1 = key("s1");
    outbox.with(|held| held.push(&s1, with_files("Read these", &["a.txt"])));
    push(&outbox, "s1", "Then this");
    let (release, held_until) = oneshot::channel();
    let (mut slow, began) = held_up(Some(held_until));
    let mut free = Fake::default();

    let flusher = flush(&outbox, &s1, &mut slow);
    let close = async {
        began.await.expect("the upload began");
        outbox.with(|held| held.push_order(&s1, OrderKind::Close, Instant::now(), None));
        assert!(outbox.with(|held| held.start_urgent(&s1)));
        flush_urgent(&outbox, &s1, &mut free).await;
        release.send(()).expect("the upload waits");
    };
    tokio::join!(flusher, close);

    assert_eq!(free.ordered, [OrderKind::Close]);
    // Nothing goes to the closed session, and no failed message is left on it.
    assert!(slow.inner.sent.is_empty());
    assert!(texts(&outbox, "s1").is_empty());
}

#[test]
fn an_order_expires_in_the_urgent_lane_too_and_its_time_starts_when_its_session_resumed() {
    let (_dir, _, outbox) = outbox();
    let s1 = key("s1");
    let start = Instant::now();
    outbox.with(|held| held.push(&s1, with_files("Read these", &["a.txt"])));
    outbox.with(|held| held.begin_step(&s1, start));
    outbox.with(|held| held.push_order(&s1, OrderKind::Cancel, start, None));

    // Held for longer than its time, it is not sent late.
    assert!(
        outbox
            .with(|held| held.begin_urgent(&s1, start + ORDER_TTL * 2))
            .is_none()
    );

    // One that waited for a resume has its whole time once it could go.
    let s2 = key("s2");
    outbox.with(|held| held.push(&s2, resuming("first")));
    outbox.with(|held| held.begin_step(&s2, start));
    outbox.with(|held| held.push_order(&s2, OrderKind::Cancel, start, None));
    outbox.with(|held| held.resumed(2, "s3".into()));
    assert!(
        outbox
            .with(|held| held.begin_urgent(&key("s3"), start + ORDER_TTL))
            .is_some()
    );
}

#[test]
fn the_redirect_to_a_resumed_session_survives_a_restart_while_its_messages_are_kept() {
    let (dir, kept, outbox) = outbox();
    outbox.with(|held| held.push(&key("s1"), resuming("first")));
    outbox.with(|held| held.begin_step(&key("s1"), Instant::now()));
    outbox.with(|held| held.resumed(0, "s2".into()));
    outbox.sync();

    let again = Outbox::load(kept, dir.path().join("cache"));

    assert_eq!(again.read(|held| held.resolve(&key("s1"))), key("s2"));
    assert_eq!(again.read(|held| held.listed(&key("s1"))).rows.len(), 1);
}

#[test]
fn only_stops_closes_and_answers_skip_a_resume_or_an_upload() {
    let (_dir, _, outbox) = outbox();
    let s1 = key("s1");
    let configure = OrderKind::Mode {
        mode: "plan".into(),
    };
    outbox.with(|held| held.push(&s1, with_files("first", &["a.txt"])));
    outbox.with(|held| held.begin_step(&s1, Instant::now()));
    outbox.with(|held| held.push_order(&s1, configure, Instant::now(), None));
    assert!(!outbox.with(|held| held.start_urgent(&s1)));

    outbox.with(|held| held.push_order(&s1, OrderKind::Cancel, Instant::now(), None));
    assert!(outbox.with(|held| held.start_urgent(&s1)));
    let order = outbox
        .with(|held| held.begin_urgent(&s1, Instant::now()))
        .expect("the cancel");
    assert_eq!(order.kind, OrderKind::Cancel);
    assert!(
        outbox
            .with(|held| held.begin_urgent(&s1, Instant::now()))
            .is_none()
    );
}

#[tokio::test]
async fn a_refused_upload_leaves_a_failed_row_with_its_files_and_sends_nothing_of_it() {
    let (_dir, _, outbox) = outbox();
    let id = outbox
        .with(|held| held.push(&key("s1"), with_files("Read these", &["a.txt"])))
        .0;
    push(&outbox, "s1", "Then this");
    let mut link = Fake {
        uploads: VecDeque::from([Err(DesktopError::BadRequest("no space left".into()))]),
        ..Fake::default()
    };

    flush(&outbox, &key("s1"), &mut link).await;

    assert_eq!(link.events, ["msg:Then this"]);
    let rows = outbox.read(|held| held.rows(&key("s1")));
    assert_eq!(
        (rows[0].state, rows[0].reason.as_deref()),
        ("failed", Some("no space left"))
    );
    // Edit gives the text and the files back, unchanged.
    let (message, _) = outbox.with(|held| held.take(id)).expect("taken");
    let taken = message.taken();
    assert_eq!(taken.text, "Read these");
    assert_eq!(taken.files[0].name, "a.txt");
    assert_eq!(taken.mentions, ["src/lib.rs"]);
}

#[tokio::test]
async fn a_dropped_link_during_an_upload_waits_and_the_message_goes_once() {
    let (_dir, _, outbox) = outbox();
    outbox.with(|held| held.push(&key("s1"), with_files("Read these", &["a.txt"])));
    let mut link = Fake {
        uploads: VecDeque::from([Err(DesktopError::Offline("build-box".into()))]),
        ..Fake::default()
    };

    flush(&outbox, &key("s1"), &mut link).await;

    assert_eq!(link.waits, 1);
    assert_eq!(
        link.events,
        ["up:a.txt", "msg:Read these\n\n@src/lib.rs @/cache/a.txt"]
    );
    assert!(link.checked.is_empty());
}

#[test]
fn files_not_uploaded_yet_survive_a_restart_and_are_not_read_as_sent() {
    let (dir, kept, outbox) = outbox();
    let id = outbox
        .with(|held| held.push(&key("s1"), with_files("Read these", &["a.txt"])))
        .0;
    outbox.with(|held| held.begin(&key("s1")));
    outbox.sync();

    let again = Outbox::load(kept, dir.path().join("cache"));
    let (message, _) = again.with(|held| held.take(id)).expect("taken");

    assert_eq!(&*message.files[0].data, "aGVsbG8=");
    assert!(!message.unsure);
    assert_eq!(message.state, Stage::Waiting);
}

#[test]
fn a_file_lost_from_the_cache_fails_its_message_instead_of_sending_it_without() {
    let (dir, kept, outbox) = outbox();
    let id = outbox
        .with(|held| held.push(&key("s1"), with_files("Read these", &["a.txt"])))
        .0;
    outbox.sync();
    fs::remove_file(
        dir.path()
            .join("cache")
            .join(IMAGES)
            .join(image_file(files_id(id), 0)),
    )
    .unwrap();

    let again = Outbox::load(kept, dir.path().join("cache"));
    let rows = again.read(|held| held.rows(&key("s1")));

    assert_eq!(rows[0].state, "failed");
    assert!(again.read(|held| held.waiting()).is_empty());
}

#[test]
fn mentions_follow_the_typed_words_and_pictures_alone_get_a_request() {
    let paths = vec!["a b.png".to_string(), "c.png".to_string()];
    assert_eq!(
        with_mentions("Look", &paths, 0, false),
        "Look\n\n@\"a b.png\" @c.png"
    );
    assert_eq!(
        with_mentions(" ", &paths[1..], 0, true),
        "Check this image.\n\n@c.png"
    );
    assert_eq!(
        with_mentions("", &paths, 0, true).lines().next(),
        Some("Check these images.")
    );
    assert_eq!(with_mentions("", &[], 0, false), "");
}

#[tokio::test]
async fn an_ended_session_resumes_once_then_the_messages_go_on_in_the_new_one_in_order() {
    let (_dir, _, outbox) = outbox();
    outbox.with(|held| held.push(&key("s1"), resuming("first")));
    // The window still shows the session as ended: this one asks to resume too.
    outbox.with(|held| held.push(&key("s1"), resuming("second")));
    let rows = outbox.read(|held| held.rows(&key("s1")));
    assert_eq!(rows.len(), 2);
    assert!(outbox.read(|held| held.messages[1].resume.is_none()));
    let mut link = Fake::default();

    flush(&outbox, &key("s1"), &mut link).await;

    assert_eq!(link.events, ["resume", "msg:first", "msg:second"]);
    assert_eq!(link.resumed, 1);
    assert_eq!(link.went_to, ["s2", "s2"]);
    // Its first row said what was being done, and the window was sent to the new session.
    assert_eq!(link.changes[0].rows[0].state, "resuming");
    assert!(unsent(&outbox, "s2").is_empty());
}

#[tokio::test]
async fn a_send_to_the_old_session_after_it_resumed_is_sent_on_and_never_resumes_again() {
    let (_dir, _, outbox) = outbox();
    outbox.with(|held| held.push(&key("s1"), resuming("first")));
    let mut link = Fake::default();
    flush(&outbox, &key("s1"), &mut link).await;

    // A window that has not opened the new session yet sends once more to the old one.
    let (_, flusher) = outbox.with(|held| held.push(&key("s1"), resuming("late")));
    assert!(flusher);
    assert_eq!(outbox.read(|held| held.resolve(&key("s1"))), key("s2"));
    assert!(outbox.read(|held| held.messages[0].resume.is_none()));
    flush(&outbox, &key("s2"), &mut link).await;

    assert_eq!(link.resumed, 1);
    assert_eq!(link.went_to, ["s2", "s2"]);
}

#[test]
fn a_read_or_a_take_back_for_the_ended_session_reaches_the_one_that_resumed() {
    let (_dir, _, outbox) = outbox();
    outbox.with(|held| held.push(&key("s1"), resuming("first")));
    outbox.with(|held| held.begin_step(&key("s1"), Instant::now()));
    outbox.with(|held| held.resumed(0, "s2".into()));

    // The window still on the ended id lists what waits on the new one.
    let listed = outbox.read(|held| held.listed(&key("s1")));
    assert_eq!(listed.rows.len(), 1);
    assert_eq!(listed.session, "s2");
    // The one being sent is left to go; once it waits again it comes back.
    let drain = || {
        outbox
            .with(|held| held.drain(&held.resolve(&key("s1"))))
            .0
            .len()
    };
    assert_eq!(drain(), 0);
    outbox.with(|held| held.wait(0, false));
    assert_eq!(drain(), 1);
}

#[tokio::test]
async fn a_refused_resume_leaves_a_failed_row_and_sends_nothing() {
    let (_dir, _, outbox) = outbox();
    let id = outbox
        .with(|held| held.push(&key("s1"), resuming("first")))
        .0;
    let mut link = Fake {
        resumes: VecDeque::from([Err(DesktopError::BadRequest("Could not resume".into()))]),
        ..Fake::default()
    };

    flush(&outbox, &key("s1"), &mut link).await;

    assert!(link.sent.is_empty());
    let rows = outbox.read(|held| held.rows(&key("s1")));
    assert_eq!(
        (rows[0].state, rows[0].reason.as_deref()),
        ("failed", Some("Could not resume"))
    );
    // Edit gives the words back; sending again asks to resume again.
    outbox.with(|held| held.take(id)).expect("taken");
    outbox.with(|held| held.push(&key("s1"), resuming("again")));
    assert!(outbox.read(|held| held.messages[0].resume.is_some()));
}

#[tokio::test]
async fn a_dropped_link_before_the_resume_waits_and_the_session_resumes_once() {
    let (_dir, _, outbox) = outbox();
    outbox.with(|held| held.push(&key("s1"), resuming("first")));
    let mut link = Fake {
        resumes: VecDeque::from([Err(DesktopError::Offline("build-box".into()))]),
        ..Fake::default()
    };

    flush(&outbox, &key("s1"), &mut link).await;

    assert_eq!(link.waits, 1);
    assert_eq!(link.resumed, 1);
    assert_eq!(link.events, ["resume", "msg:first"]);
}

#[tokio::test]
async fn the_picks_apply_to_the_resumed_session_before_the_message_and_its_files_after() {
    let (_dir, _, outbox) = outbox();
    let pick = Pick {
        model: None,
        effort: Some(Effort::High),
        fast: None,
    };
    let mut draft = with_files("Read", &["a.txt"]);
    draft.resume = Some(Resume {
        pick: Some(pick),
        ..Resume::default()
    });
    outbox.with(|held| held.push(&key("s1"), draft));
    let mut link = Fake::default();

    flush(&outbox, &key("s1"), &mut link).await;

    assert_eq!(
        link.events,
        [
            "resume",
            "configure:s2",
            "up:a.txt",
            "msg:Read\n\n@src/lib.rs @/cache/a.txt"
        ]
    );
    // The window that sent it hears the pick that was applied, not the one it holds by then.
    let resumed: Vec<&Changed> = link.changes.iter().filter(|c| c.pick.is_some()).collect();
    assert_eq!(resumed.len(), 1);
    assert_eq!(
        resumed[0].pick.as_ref().and_then(|p| p.effort),
        Some(Effort::High)
    );
}

#[test]
fn a_resume_cut_short_by_a_restart_fails_its_message_and_those_behind_instead_of_resuming_again() {
    let (dir, kept, outbox) = outbox();
    outbox.with(|held| held.push(&key("s1"), resuming("first")));
    push(&outbox, "s1", "behind");
    push(&outbox, "s9", "elsewhere");
    outbox.with(|held| held.begin(&key("s1")));
    outbox.sync();

    let again = Outbox::load(kept, dir.path().join("cache"));
    let rows = again.read(|held| held.rows(&key("s1")));

    assert_eq!(rows.len(), 2);
    assert!(rows.iter().all(|row| row.state == "failed"));
    assert!(
        rows[0]
            .reason
            .as_deref()
            .is_some_and(|r| r.contains("may have resumed"))
    );
    // Nothing of that session goes; another session's message still does.
    assert_eq!(again.read(|held| held.waiting()), [key("s9")]);
}

#[tokio::test]
async fn a_message_behind_a_refused_resume_resumes_in_its_place_once() {
    let (_dir, _, outbox) = outbox();
    outbox.with(|held| held.push(&key("s1"), resuming("first")));
    outbox.with(|held| held.push(&key("s1"), resuming("second")));
    let mut link = Fake {
        resumes: VecDeque::from([Err(DesktopError::BadRequest("Could not resume".into()))]),
        ..Fake::default()
    };

    flush(&outbox, &key("s1"), &mut link).await;

    assert_eq!(link.events, ["resume", "msg:second"]);
    assert_eq!(link.resumed, 1);
    assert_eq!(link.went_to, ["s2"]);
    // The refused one goes on with the session, failed, for the user to Edit.
    let rows = unsent(&outbox, "s2");
    assert_eq!((rows.len(), rows[0].state), (1, "failed"));
}

#[tokio::test]
async fn a_message_behind_one_taken_back_before_it_resumed_carries_the_resume() {
    let (_dir, _, outbox) = outbox();
    let first = outbox
        .with(|held| held.push(&key("s1"), resuming("first")))
        .0;
    outbox.with(|held| held.push(&key("s1"), resuming("second")));
    outbox.with(|held| held.take(first)).expect("taken");
    assert!(outbox.read(|held| held.messages[0].resume.is_some()));
    let mut link = Fake::default();

    flush(&outbox, &key("s1"), &mut link).await;

    assert_eq!(link.events, ["resume", "msg:second"]);
    assert_eq!(link.went_to, ["s2"]);
}

#[tokio::test]
async fn a_resume_whose_answer_never_came_fails_every_message_waiting_for_that_session() {
    let (_dir, _, outbox) = outbox();
    outbox.with(|held| held.push(&key("s1"), resuming("first")));
    outbox.with(|held| held.push(&key("s1"), resuming("second")));
    let mut link = Fake {
        resumes: VecDeque::from([Err(DesktopError::TimedOut("build-box".into()))]),
        ..Fake::default()
    };

    flush(&outbox, &key("s1"), &mut link).await;

    // It may have resumed: nothing is resumed again, nothing goes to the ended session.
    assert_eq!(link.resumed, 0);
    assert!(link.sent.is_empty());
    let rows = outbox.read(|held| held.rows(&key("s1")));
    assert!(rows.iter().all(|row| row.state == "failed"));
    assert_eq!(rows.len(), 2);
}

#[test]
fn a_file_over_the_limit_is_named_and_a_file_within_it_is_not() {
    let small: KeptImage = image("a.txt").into();
    let big: KeptImage = PastedImage {
        data: "QUJD".repeat(4),
        ..image("big.bin")
    }
    .into();
    // "aGVsbG8=" is 5 bytes, the other 12.
    assert_eq!(base64_size(&small.data), 5);
    assert_eq!(base64_size(&big.data), 12);
    assert_eq!(too_big(std::slice::from_ref(&small), 5), None);
    let reason = too_big(&[small, big], 11).expect("refused");
    assert!(reason.starts_with("big.bin is larger than"));
}
