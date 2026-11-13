use super::*;
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
    let Reply::Terminals { terminals: listed } = terminals.answer(Request::List {}).await else {
        panic!("no list");
    };
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].command, "echo done; exit 3");
    // Listed with how it ended once, then gone.
    let Reply::Terminals { terminals: listed } = terminals.answer(Request::List {}).await else {
        panic!("no list");
    };
    assert!(listed.is_empty());
    assert!(terminals.live(id).is_none());
}
