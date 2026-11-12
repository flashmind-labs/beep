use super::*;
use std::time::UNIX_EPOCH;

#[test]
fn events_read_as_the_window_wants_them() {
    let output = TermEvent::from(TerminalEvent::Output {
        at: 7,
        data: ByteBuf::from(b"hi".to_vec()),
    });
    assert_eq!(
        output,
        TermEvent::Output {
            at: 7,
            data: b"hi".to_vec()
        }
    );
    // Output reaches the window as raw bytes: the offset, then the data.
    let InvokeResponseBody::Raw(raw) = output.body().unwrap() else {
        panic!("output went as JSON");
    };
    assert_eq!(raw, [&7u64.to_le_bytes()[..], b"hi"].concat());
    let InvokeResponseBody::Json(json) = TermEvent::Attached.body().unwrap() else {
        panic!("a notice went as bytes");
    };
    assert_eq!(json, r#"{"kind":"attached"}"#);
    let ended = TermEvent::from(TerminalEvent::Ended {
        state: TerminalState::Exited { code: Some(1) },
    });
    assert_eq!(
        ended,
        TermEvent::Ended {
            state: "exited",
            code: Some(1)
        }
    );
}

#[test]
fn long_input_goes_in_pieces() {
    let pieces = keys(&vec![b'x'; MAX_TERMINAL_INPUT * 2 + 1]);
    assert_eq!(pieces.len(), 3);
}

#[test]
fn an_older_attach_ending_leaves_a_newer_one() {
    let attached = Attached::default();
    let viewer = Viewer {
        terminal: Uuid::nil(),
        view: Uuid::nil(),
    };
    let (first, _) = mpsc::channel(1);
    let old = attached.follow(viewer, "center", first);
    let (second, _) = mpsc::channel(1);
    attached.follow(viewer, "center", second);
    attached.unfollow(viewer, old);
    assert!(attached.input(viewer).is_ok());
}

// Two views of one terminal used to share one slot, so each attach
// finished the other's stream and they reattached in turn forever.
#[test]
fn two_views_of_a_terminal_keep_their_own_streams() {
    let attached = Attached::default();
    let terminal = Uuid::from_u128(1);
    let pane = Viewer {
        terminal,
        view: Uuid::from_u128(2),
    };
    let page = Viewer {
        terminal,
        view: Uuid::from_u128(3),
    };
    let (pane_tx, pane_rx) = mpsc::channel(1);
    attached.follow(pane, "center", pane_tx);
    let (page_tx, _page_rx) = mpsc::channel(1);
    attached.follow(page, "center", page_tx);
    assert!(
        !pane_rx.is_closed(),
        "the page attaching kept the pane's stream"
    );
    attached.detach(page);
    assert!(
        attached.input(pane).is_ok(),
        "the page detaching kept the pane"
    );
    assert!(!pane_rx.is_closed());
}

#[test]
fn closing_a_window_ends_only_its_views() {
    let attached = Attached::default();
    let terminal = Uuid::from_u128(1);
    let here = Viewer {
        terminal,
        view: Uuid::from_u128(2),
    };
    let there = Viewer {
        terminal,
        view: Uuid::from_u128(3),
    };
    let (here_tx, here_rx) = mpsc::channel(1);
    attached.follow(here, "center", here_tx);
    let (there_tx, there_rx) = mpsc::channel(1);
    attached.follow(there, "project-61", there_tx);

    attached.clear("center");
    assert!(here_rx.is_closed(), "its stream finishes");
    assert!(!there_rx.is_closed());
    assert!(attached.input(there).is_ok());
}

#[test]
fn an_exited_terminal_keeps_its_code() {
    let row = TerminalRow::from(Terminal {
        id: Uuid::nil(),
        project: "acme/widget".into(),
        machine_id: "m1".into(),
        machine_name: "build-box".into(),
        name: "tests".into(),
        command: "just test".into(),
        cwd: "/srv/project".into(),
        state: TerminalState::Exited { code: Some(2) },
        started_at: UNIX_EPOCH,
    });
    assert_eq!(row.project, "acme/widget");
    assert_eq!(row.state, "exited");
    assert_eq!(row.code, Some(2));
}

fn output(event: Option<&TermEvent>) -> (u64, usize) {
    match event {
        Some(TermEvent::Output { at, data }) => (*at, data.len()),
        other => panic!("not output: {other:?}"),
    }
}

#[test]
fn output_after_a_quiet_spell_goes_at_once() {
    let mut batch = OutputBatch::default();
    let start = Instant::now();

    let sent = batch.add(0, b"$ ", start);
    assert_eq!(output(sent.first()), (0, 2));
    assert_eq!(batch.due(), None);

    // A keystroke's echo, well after.
    let sent = batch.add(2, b"l", start + BATCH_WAIT * 10);
    assert_eq!(output(sent.first()), (2, 1));
}

#[test]
fn a_flood_goes_in_few_large_messages() {
    let mut batch = OutputBatch::default();
    let start = Instant::now();
    let chunk = vec![b'x'; 8 * 1024];
    let mut messages = Vec::new();
    let mut at = 0;

    // 8 KB pieces as the node reads them, faster than a batch waits.
    for i in 0..256 {
        let now = start + Duration::from_micros(i * 10);
        messages.extend(batch.add(at, &chunk, now));
        at += chunk.len() as u64;
    }
    messages.extend(batch.take(start + BATCH_WAIT));

    // The first piece at once, then pieces of BATCH_BYTES, all of it in order.
    assert_eq!(output(messages.first()), (0, chunk.len()));
    assert!(messages.len() <= 2 + (256 * chunk.len()) / BATCH_BYTES);
    let mut next = 0;
    for message in &messages {
        let (from, len) = output(Some(message));
        assert_eq!(from, next);
        next += len as u64;
    }
    assert_eq!(next, at);
}

#[test]
fn gathered_output_waits_no_longer_than_the_batch_wait() {
    let mut batch = OutputBatch::default();
    let start = Instant::now();
    batch.add(0, b"a", start);

    assert!(
        batch
            .add(1, b"b", start + Duration::from_millis(1))
            .is_empty()
    );
    assert_eq!(
        batch.due(),
        Some(start + Duration::from_millis(1) + BATCH_WAIT)
    );
    assert_eq!(output(batch.take(start + BATCH_WAIT).as_ref()), (1, 1));
    assert_eq!(batch.due(), None);
}

#[test]
fn output_that_does_not_follow_goes_apart() {
    let mut batch = OutputBatch::default();
    let start = Instant::now();
    batch.add(0, b"a", start);
    assert!(batch.add(1, b"b", start).is_empty());

    // Skipped ahead, as when the node kept less than was asked for.
    let sent = batch.add(100, b"c", start);
    assert_eq!(output(sent.first()), (1, 1));
    assert_eq!(output(batch.take(start).as_ref()), (100, 1));
}
