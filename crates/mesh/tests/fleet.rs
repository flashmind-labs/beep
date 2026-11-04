//! A desktop's view of a node with no server anywhere: the node runs in
//! this process, reached the way the desktop reaches the node on its own
//! Mac, and answers the requests the apps used to send through the server.

use beep_mesh::{
    PeerClient,
    machine::{
        self, NodeWork, open_agent, order, sessions,
        terminals::{self, Heard, Kept},
    },
    transport_config,
};
use beep_node::{
    agents::AgentKind,
    link::{Offers, peers::Peers},
    machine::provision::Provision,
};
use beep_proto::{
    TerminalEvent,
    node::{
        agent::{DiffAsk, Event, Request, Status},
        terminal::{Reply as PtyReply, Request as PtyRequest, Size},
    },
};
use iroh::{Endpoint, SecretKey, endpoint::presets};
use serde_bytes::ByteBuf;
use std::{collections::BTreeMap, fs, path::Path, process::Command, time::Duration};
use tempfile::TempDir;
use tokio::time::timeout;

const LIMIT: Duration = Duration::from_secs(60);

/// A fake coding agent that finishes its first turn and then idles.
const ADAPTER: &str = r#"
id() { printf '%s\n' "$line" | sed -nE 's/.*"id":("[^"]*"|[0-9]+).*/\1/p'; }
reply() { printf '{"jsonrpc":"2.0","id":%s,"result":%s}\n' "$(id)" "$1"; }
read -r line
reply '{"protocolVersion":1,"agentCapabilities":{}}'
read -r line
reply '{"sessionId":"s"}'
read -r line
reply '{"stopReason":"end_turn"}'
cat > /dev/null
"#;

struct Machine {
    client: PeerClient,
    peers: Peers,
    _dir: TempDir,
}

/// A node in this process. `network` says whether its endpoint is left open.
async fn machine(network: bool) -> Machine {
    let dir = TempDir::new().expect("a temp dir");
    let endpoint = Endpoint::builder(presets::Minimal)
        .secret_key(SecretKey::generate())
        .transport_config(transport_config())
        .bind()
        .await
        .expect("the endpoint binds");
    if !network {
        endpoint.close().await;
    }
    let agent = AgentKind {
        name: "codex".into(),
        program: "sh".into(),
        args: vec!["-c".into(), ADAPTER.into(), "fake-adapter".into()],
        env: BTreeMap::new(),
    };
    let offers = Offers::local(&[agent], Provision::new(dir.path()), false);
    let peers = Peers::new(endpoint, dir.path().to_owned(), "build-box".into(), offers);
    Machine {
        client: PeerClient::local(peers.local_session()),
        peers,
        _dir: dir,
    }
}

async fn start(client: &PeerClient, cwd: &Path) -> machine::agent::Opened {
    let first = Request::Start {
        cwd: Some(cwd.display().to_string()),
        prompt: "go".into(),
        model: None,
        effort: None,
        checkout: None,
    };
    open_agent(client, "codex", first)
        .await
        .expect("the session starts")
}

/// Reads events until the first turn is done.
async fn first_turn(opened: &mut machine::agent::Opened) {
    timeout(LIMIT, async {
        loop {
            match opened.rx.recv::<Event>().await.expect("an event reads") {
                Some(Event::TurnDone { .. }) => return,
                Some(Event::Ended { reason }) => panic!("the session ended: {reason}"),
                Some(_) => {}
                None => panic!("the stream closed"),
            }
        }
    })
    .await
    .expect("the turn finished in time");
}

#[tokio::test]
async fn an_agent_starts_and_answers_with_no_server_at_all() {
    let node = machine(true).await;
    let work = TempDir::new().expect("a temp dir");
    let mut opened = start(&node.client, work.path()).await;
    first_turn(&mut opened).await;
    let listed = sessions::list(&node.client).await.expect("the list reads");
    let mine = listed
        .iter()
        .find(|info| info.id == opened.info.id)
        .expect("the session is listed");
    assert_eq!(mine.turns, 1);
    assert_ne!(mine.state.status, Status::Ended);
    // It takes an order and acknowledges it.
    order(
        &mut opened,
        Request::Prompt {
            text: "more".into(),
        },
        LIMIT,
    )
    .await
    .expect("the node takes the prompt");
}

#[tokio::test]
async fn a_session_keeps_running_while_the_desktop_is_gone_and_reattaches() {
    let node = machine(true).await;
    let work = TempDir::new().expect("a temp dir");
    let mut opened = start(&node.client, work.path()).await;
    first_turn(&mut opened).await;
    let id = opened.info.id.clone();
    // The desktop goes away: its stream and its client are dropped.
    drop(opened);
    let again = PeerClient::local(node.peers.local_session());
    let attached = open_agent(
        &again,
        "codex",
        Request::Attach {
            session: id.clone(),
        },
    )
    .await
    .expect("the session is still there");
    assert_eq!(attached.info.id, id);
    assert_eq!(attached.info.turns, 1);
}

#[tokio::test]
async fn opening_a_local_session_makes_no_network_call() {
    // The endpoint is closed: whatever the calls reach is in this process.
    let node = machine(false).await;
    let work = TempDir::new().expect("a temp dir");
    let mut opened = start(&node.client, work.path()).await;
    first_turn(&mut opened).await;
    let (entries, _) = sessions::transcript(&node.client, &opened.info.id, 10, false)
        .await
        .expect("the transcript reads");
    assert!(!entries.is_empty());
}

#[tokio::test]
async fn a_files_diff_and_tree_read_with_no_server() {
    let node = machine(true).await;
    let repo = TempDir::new().expect("a temp dir");
    let git = |args: &[&str]| {
        let status = Command::new("git")
            .args(args)
            .current_dir(repo.path())
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@example.com")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@example.com")
            .status()
            .expect("git runs");
        assert!(status.success(), "git {args:?}");
    };
    git(&["init", "-q", "-b", "main"]);
    fs::write(repo.path().join("a.txt"), "one\n").expect("a file");
    git(&["add", "."]);
    git(&["commit", "-q", "-m", "first"]);
    fs::write(repo.path().join("a.txt"), "one\ntwo\n").expect("a change");
    let root = repo.path().display().to_string();

    let NodeWork::Done(tree) = sessions::tree(
        &node.client,
        &beep_proto::ListWorkTree {
            machine_id: String::new(),
            root: root.clone(),
            path: String::new(),
            ignored: false,
            depth: 2,
        },
    )
    .await
    .expect("the tree reads") else {
        panic!("the node refused the tree");
    };
    assert!(
        tree.folders
            .iter()
            .flat_map(|folder| &folder.entries)
            .any(|entry| entry.name == "a.txt")
    );

    let NodeWork::Done(diff) = sessions::diff(
        &node.client,
        DiffAsk {
            root: root.clone(),
            base: String::new(),
            head: String::new(),
            only: Vec::new(),
            lockfiles: false,
        },
    )
    .await
    .expect("the diff reads") else {
        panic!("the node refused the diff");
    };
    assert_eq!(diff.additions, 1);
    assert!(diff.files.iter().any(|file| file.path == "a.txt"));

    let NodeWork::Done(_) = sessions::write_file(
        &node.client,
        sessions::WorkWrite {
            root: root.clone(),
            path: "b.txt".into(),
            offset: 0,
            size: 3,
            data: b"new".to_vec(),
        },
    )
    .await
    .expect("the write answers") else {
        panic!("the node refused the write");
    };
    assert_eq!(
        fs::read(repo.path().join("b.txt")).expect("written"),
        b"new"
    );
}

#[tokio::test]
async fn a_terminal_opens_and_echoes_with_no_server() {
    let node = machine(true).await;
    // The node keeps its terminals in a host; with no beep-node binary here
    // to start one, it runs in this process.
    let host = TempDir::new().expect("a temp dir");
    tokio::spawn(beep_node::terminal_host_in_process(host.path()).expect("a terminal host"));
    let folder = TempDir::new().expect("a temp dir");
    let size = Size::clamped(100, 30);
    let task = terminals::open(
        &node.client,
        None,
        Some(folder.path().display().to_string()),
        size,
    )
    .await
    .expect("the terminal opens");
    let kept = Kept {
        task: task.id.0,
        pty: true,
        command: &task.command,
    };
    let listed = terminals::list_ptys(&node.client).await.expect("it lists");
    assert_eq!(
        terminals::state_of(&kept, &listed),
        beep_proto::TerminalState::Running {}
    );
    let (mut tx, mut rx) = terminals::attach(&node.client, task.id, 0, size)
        .await
        .expect("it attaches");
    tx.send(&PtyRequest::Input {
        data: ByteBuf::from(b"echo sum-$((40+2))\r".to_vec()),
    })
    .await
    .expect("keys are sent");
    let mut seen = Vec::new();
    timeout(LIMIT, async {
        while !String::from_utf8_lossy(&seen).contains("sum-42") {
            let reply = rx
                .recv::<PtyReply>()
                .await
                .expect("the stream reads")
                .expect("the stream is open");
            match terminals::heard(reply) {
                Heard::Show(TerminalEvent::Output { data, .. }) => seen.extend_from_slice(&data),
                other => panic!("expected output, got {other:?}"),
            }
        }
    })
    .await
    .expect("the shell answered");
    terminals::stop(&node.client, &kept)
        .await
        .expect("it stops");
}
