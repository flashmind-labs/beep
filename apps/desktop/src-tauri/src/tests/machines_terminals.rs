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
            data: "aGk=".into()
        }
    );
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
