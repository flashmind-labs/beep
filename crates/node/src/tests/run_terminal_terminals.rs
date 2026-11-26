use super::*;
use super::super::{pty::pty_size, screen::Screen};
use portable_pty::ChildKiller;
use std::sync::Mutex;
use tokio::{sync::mpsc, time::timeout};
use beep_proto::node::process::TaskState;
use beep_proto::watched;

#[cfg(unix)]
fn started(reply: Reply) -> TaskId {
    match reply {
        Reply::Started { task } => task.id,
        other => panic!("not started: {other:?}"),
    }
}

/// Waits until the terminal's output so far holds `needle`.
#[cfg(unix)]
async fn wait_for_output(live: &Live, needle: &str) -> String {
    let mut changed = live.changed.subscribe();
    timeout(Duration::from_secs(20), async {
        loop {
            watched::read_and_update(&mut changed, |_| ());
            let bytes: Vec<u8> = lock(&live.screen).lines.iter().flatten().copied().collect();
            let text = String::from_utf8_lossy(&bytes).into_owned();

            if text.contains(needle) {
                return text;
            }

            changed.changed().await.expect("the terminal is kept");
        }
    })
    .await
    .unwrap_or_else(|_| panic!("no {needle:?} in the terminal"))
}

// Output says which terminal printed, so a follower of one terminal is not
// woken by the others, and a follower of all of them still is.
#[cfg(unix)]
#[tokio::test]
async fn output_pings_the_topic_of_its_terminal() {
    let mut changes = crate::events::system().subscribe();
    let terminals = Terminals::default();
    let id = started(
        terminals
            .answer(Request::Start {
                command: Some("echo printed; read wait".into()),
                cwd: None,
                size: Size { cols: 80, rows: 24 },
            })
            .await,
    );
    let ours = crate::run::terminal::pty::output_topic(id);

    timeout(Duration::from_secs(20), async {
        loop {
            let change = changes.recv().await.expect("the hub runs");

            if change.topic == ours {
                return;
            }
        }
    })
    .await
    .expect("the terminal's output pinged its own topic");

    assert!(beep_proto::node::events::Topic::new("terminal-output").matches(&ours));
    assert!(!beep_proto::node::events::Topic::keyed("terminal-output", "another").matches(&ours));
    terminals.answer(Request::Stop { id }).await;
}

// A pseudo-terminal round trip: keys typed reach the shell, and what it
// prints comes back.
#[cfg(unix)]
#[tokio::test]
async fn typed_keys_reach_the_shell_and_its_output_comes_back() {
    let terminals = Terminals::default();
    let id = started(
        terminals
            .answer(Request::Start {
                command: Some("read line; echo \"got:$line\"; read wait".into()),
                cwd: None,
                size: Size {
                    cols: 100,
                    rows: 30,
                },
            })
            .await,
    );
    let live = terminals.live(id).expect("the terminal is kept");
    live.type_keys(b"hello\r".to_vec());
    wait_for_output(&live, "got:hello").await;
    assert!(
        live.resize(Size { cols: 50, rows: 10 }),
        "a new size applies"
    );
    assert!(
        !live.resize(Size { cols: 50, rows: 10 }),
        "the same size again is left alone, so the shell does not redraw"
    );
    let size = lock(&live.master).get_size().expect("a size");
    assert_eq!((size.cols, size.rows), (50, 10));
    let Reply::Stopped { task } = terminals.answer(Request::Stop { id }).await else {
        panic!("not stopped");
    };
    assert_eq!(task.state, TaskState::Stopped);
}

#[cfg(unix)]
#[tokio::test]
async fn a_terminal_ends_with_its_command() {
    let terminals = Terminals::default();
    let id = started(
        terminals
            .answer(Request::Start {
                command: Some("echo done; exit 3".into()),
                cwd: None,
                size: Size::DEFAULT,
            })
            .await,
    );
    let live = terminals.live(id).expect("the terminal is kept");
    assert!(live.wait_ended(Duration::from_secs(20)).await);
    assert_eq!(live.task().state, TaskState::Exited { code: Some(3) });
    wait_for_output(&live, "done").await;
    let listed = terminals.list();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].command, "echo done; exit 3");
    assert_eq!(listed[0].state, TaskState::Exited { code: Some(3) });
}

// Every reader sees how a terminal ended, however many look and when.
#[cfg(unix)]
#[tokio::test]
async fn every_reader_sees_a_terminals_exit() {
    let terminals = Terminals::default();
    let id = started(
        terminals
            .answer(Request::Start {
                command: Some("exit 4".into()),
                cwd: None,
                size: Size::DEFAULT,
            })
            .await,
    );
    let live = terminals.live(id).expect("the terminal is kept");
    assert!(live.wait_ended(Duration::from_secs(20)).await);
    let exit = TaskState::Exited { code: Some(4) };

    for _ in 0..2 {
        let Reply::Terminals { terminals: listed } = terminals.answer(Request::List {}).await
        else {
            panic!("no list");
        };
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].state, exit);
    }

    let reader = terminals.subscribe();
    assert_eq!(
        watched::read(&reader, |list| list[0].live.task().state),
        exit
    );
    assert!(terminals.live(id).is_some(), "an attach still finds it");
}

// The clock stays paused and moves only by `advance`, so the keep time is
// the only timer the actor waits for.
#[cfg(unix)]
#[tokio::test]
async fn an_ended_terminal_leaves_the_list_after_the_keep_time() {
    let terminals = Terminals::default();
    let id = started(
        terminals
            .answer(Request::Start {
                command: Some("exit 0".into()),
                cwd: None,
                size: Size::DEFAULT,
            })
            .await,
    );
    let live = terminals.live(id).expect("the terminal is kept");
    assert!(live.wait_ended(Duration::from_secs(20)).await);
    tokio::time::pause();
    // A publish that starts after the end, so the actor has timed it.
    let mut reader = terminals.subscribe();
    watched::read_and_update(&mut reader, |_| ());
    terminals.opener.changed.notify_one();
    reader.changed().await.expect("the terminals are kept");
    assert_eq!(terminals.list().len(), 1);

    tokio::time::advance(KEEP_ENDED - Duration::from_millis(1)).await;
    assert_eq!(terminals.list().len(), 1, "listed just before the time is up");

    tokio::time::advance(Duration::from_millis(2)).await;
    tokio::task::yield_now().await;
    assert!(terminals.list().is_empty(), "gone right after it");
    assert!(terminals.live(id).is_none());
}

#[tokio::test]
async fn stop_waits_for_output_from_a_terminal_that_already_exited() {
    use portable_pty::native_pty_system;
    use std::{
        io,
        sync::atomic::{AtomicUsize, Ordering},
    };

    #[derive(Clone, Debug, Default)]
    struct Killer(Arc<AtomicUsize>);

    impl ChildKiller for Killer {
        fn kill(&mut self) -> io::Result<()> {
            self.0.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }

        fn clone_killer(&self) -> Box<dyn ChildKiller + Send + Sync> {
            Box::new(self.clone())
        }
    }

    let terminals = Terminals::default();
    let id = TaskId(1);
    let exit = TaskState::Exited { code: Some(7) };
    let mut screen = Screen::new();
    screen.phase = Phase::Draining(exit);
    let pty = native_pty_system()
        .openpty(pty_size(Size::DEFAULT))
        .expect("a pty");
    let live = Arc::new(Live {
        id,
        command: "shell".into(),
        screen: Mutex::new(screen),
        changed: watch::channel(0).0,
        input: mpsc::channel(1).0,
        master: Mutex::new(pty.master),
        #[cfg(windows)]
        process: None,
        started_or_ended: terminals.opener.changed.clone(),
    });
    let killer = Killer::default();
    // The actor never started here, so the test is the list's writer.
    terminals.list.send_modify(|list| {
        list.push(Arc::new(Term {
            live: live.clone(),
            killer: Mutex::new(Box::new(killer.clone())),
            #[cfg(unix)]
            group: None,
            pid: None,
            cwd: None,
        }));
    });

    let stop = terminals.answer(Request::Stop { id });
    tokio::pin!(stop);
    assert!(futures::poll!(stop.as_mut()).is_pending());
    assert_eq!(killer.0.load(Ordering::Relaxed), 0);
    assert!(
        terminals.running().is_empty(),
        "an exited child cannot be reopened"
    );

    lock(&live.screen).phase = Phase::Ended(exit);
    live.changed.send_replace(1);
    let Reply::Stopped { task } = stop.await else {
        panic!("the terminal did not report its end");
    };
    assert_eq!(task.state, exit);
}
