use super::*;
use beep_proto::node::{
    process::{TaskId, TaskState},
    terminal::{MAX_SCROLLBACK, Size},
};
use tokio_util::task::AbortOnDropHandle;

/// A host started by the previous version outlives a node restart. It
/// writes maps and reads with plain `rmp_serde`, which takes arrays that
/// have just the fields it knows. The shapes are unchanged, so the new
/// node's compact messages are read by it and its maps by the new node.
#[test]
fn a_host_from_the_previous_version_still_works_with_this_node() {
    let size = Size::DEFAULT;
    let requests = [
        Request::Start {
            command: Some("ls".into()),
            cwd: None,
            size,
        },
        Request::List {},
        Request::Stop { id: TaskId(3) },
        Request::Attach {
            id: TaskId(3),
            from: 9,
        },
        Request::Input {
            data: serde_bytes::ByteBuf::from(vec![0u8, 200, 13]),
        },
        Request::Resize { size },
    ];

    for request in requests {
        let sent = rmp_serde::to_vec(&request).expect("encodes");
        let read: Request = rmp_serde::from_slice(&sent).expect("the old host reads it");
        assert_eq!(read, request);
    }

    let hello = Hello {
        env: vec![EnvSecret {
            name: "A".into(),
            value: SecretText::new("b"),
        }],
    };
    let sent = rmp_serde::to_vec(&hello).expect("encodes");
    let read: Hello = rmp_serde::from_slice(&sent).expect("the old host reads it");
    assert_eq!(read.env.len(), 1);
    let replies = [
        Reply::Terminals {
            terminals: Vec::new(),
        },
        Reply::Output {
            at: 1,
            data: serde_bytes::ByteBuf::from(vec![255u8]),
        },
        Reply::Error {
            message: "no".into(),
        },
    ];

    for reply in replies {
        let sent = rmp_serde::to_vec_named(&reply).expect("encodes");
        let read: Reply = lenient::from_slice(&sent).expect("this node reads it");
        assert_eq!(read, reply);
    }
}

/// A stream from a peer, relayed by the node to the host in `dir`.
fn stream(dir: &Path) -> (Conn, AbortOnDropHandle<Result<()>>) {
    let (server, node) = Stream::pair().expect("a socket pair");
    let dir = dir.to_path_buf();
    let relaying = tokio::spawn(async move { relay_in(&dir, &mut Conn::new(node)).await });
    (Conn::new(server), AbortOnDropHandle::new(relaying))
}

async fn ask(dir: &Path, request: Request) -> Reply {
    let (mut server, _relaying) = stream(dir);
    server.write(&request).await.expect("the request goes");
    server.read().await.expect("a reply").expect("a reply")
}

async fn start(dir: &Path, command: &str) -> TaskId {
    let request = Request::Start {
        command: Some(command.into()),
        cwd: None,
        size: Size::DEFAULT,
    };

    match ask(dir, request).await {
        Reply::Started { task } => task.id,
        other => panic!("not started: {other:?}"),
    }
}

/// Reads an attached stream until its output holds `needle`; returns
/// the output and where it started.
async fn read_until(server: &mut Conn, needle: &str) -> (u64, String) {
    let mut first = None;
    let mut text = String::new();
    timeout(Duration::from_secs(20), async {
        while !text.contains(needle) {
            match server.read().await.expect("a reply") {
                Some(Reply::Output { at, data }) => {
                    first.get_or_insert(at);
                    text.push_str(&String::from_utf8_lossy(&data));
                }
                other => panic!("not output: {other:?}"),
            }
        }
    })
    .await
    .unwrap_or_else(|_| panic!("no {needle:?} in {text:?}"));
    (first.unwrap_or_default(), text)
}

/// Attaches to terminal `id` from its oldest kept output.
async fn attach(dir: &Path, id: TaskId) -> (Conn, AbortOnDropHandle<Result<()>>) {
    let (mut server, relaying) = stream(dir);
    server
        .write(&Request::Attach { id, from: 0 })
        .await
        .expect("the attach goes");
    (server, relaying)
}

fn host(dir: &Path) -> AbortOnDropHandle<Result<()>> {
    let listener = Listener::bind(&dir.join(SOCKET)).expect("the host listens");
    let dir = dir.to_path_buf();
    AbortOnDropHandle::new(tokio::spawn(async move {
        serve(&dir, listener, &Terminals::default()).await
    }))
}

#[tokio::test]
async fn a_terminal_outlives_the_node_and_replays_what_it_showed() {
    let dir = tempfile::tempdir().expect("a temporary folder");
    let _host = host(dir.path());
    let id = start(dir.path(), "read line; echo \"got:$line\"; read wait").await;
    let (mut server, relaying) = attach(dir.path(), id).await;
    server
        .write(&Request::Input {
            data: b"marker\r".to_vec().into(),
        })
        .await
        .expect("the keys go");
    read_until(&mut server, "got:marker").await;
    // The node goes, as in a restart, and with it every stream.
    drop((server, relaying));

    let Reply::Terminals { terminals } = ask(dir.path(), Request::List {}).await else {
        panic!("no list");
    };
    assert_eq!(terminals.len(), 1);
    assert_eq!(terminals[0].id, id);
    assert_eq!(terminals[0].state, TaskState::Running);
    let (mut server, _relaying) = attach(dir.path(), id).await;
    read_until(&mut server, "got:marker").await;

    let Reply::Stopped { task } = ask(dir.path(), Request::Stop { id }).await else {
        panic!("not stopped");
    };
    assert_eq!(task.state, TaskState::Stopped);
}

#[tokio::test]
async fn the_scrollback_keeps_the_last_lines() {
    let dir = tempfile::tempdir().expect("a temporary folder");
    let _host = host(dir.path());
    let id = start(dir.path(), "seq 1 5000; read wait").await;
    let (mut server, _relaying) = attach(dir.path(), id).await;
    read_until(&mut server, "\n5000\r\n").await;
    let (mut server, _relaying) = attach(dir.path(), id).await;
    let (at, replayed) = read_until(&mut server, "\n5000\r\n").await;
    assert!(at > 0, "the oldest output was dropped");
    let first = 5001 - MAX_SCROLLBACK;
    assert!(
        replayed.starts_with(&format!("{first}\r\n")),
        "{replayed:?}"
    );
    assert_eq!(replayed.lines().count(), MAX_SCROLLBACK);
    ask(dir.path(), Request::Stop { id }).await;
}
