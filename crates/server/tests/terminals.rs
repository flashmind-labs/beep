//! An interactive terminal end to end: an app opens a shell on a real node
//! in this process, attaches to it, types, and reads the shell's answer
//! back through the server, then the shell's exit ends the stream.

use async_trait::async_trait;
use beep_node::link::Offers;
use beep_server::{
    agent::models::ModelChoices,
    api::http,
    app::{App, ServerTools},
    nodes,
    protocol::{
        AttachTerminal, DeviceKind, ListTerminals, OpenTerminal, TerminalEvent, TerminalInput,
        TerminalState, client::Client,
    },
    push::Pulse,
    store::{NewKey, TestDb},
};
use flashmind::types::{
    AgentLlmConfig, CompletionRequest, CompletionStream, LlmProvider, Provider,
};
use iroh::{Endpoint, SecretKey, endpoint::presets, protocol::Router};
use serde_bytes::ByteBuf;
use std::{sync::Arc, time::Duration};
use tempfile::TempDir;
use tokio::time::timeout;

const PROJECT: &str = "acme/widget";

struct NoModel;

#[async_trait]
impl LlmProvider for NoModel {
    fn name(&self) -> &str {
        "none"
    }
    fn provider(&self) -> Provider {
        Provider::OpenAi
    }
    fn complete(&self, _request: CompletionRequest) -> CompletionStream {
        unreachable!("the terminal tests make no model calls")
    }
}

async fn endpoint(key: SecretKey) -> Endpoint {
    Endpoint::builder(presets::Minimal)
        .secret_key(key)
        .bind()
        .await
        .expect("a test endpoint binds")
}

/// A server, a user's node linked to it and the user's app.
struct Setup {
    _db: TestDb,
    router: Router,
    _apps: http::Local,
    machine: String,
    client: Client,
    _node: tokio::task::JoinHandle<anyhow::Result<()>>,
    _terminals: TempDir,
}

async fn setup() -> Setup {
    let db = TestDb::new().await.unwrap();
    let store = db.store().await.unwrap();
    let model = "openai:test".parse().unwrap();
    let models = ModelChoices::resolve(&NoModel, model, Vec::new()).await;
    let app = App::new(
        store.clone(),
        Arc::new(NoModel),
        AgentLlmConfig::new("openai:test".parse().unwrap()),
        ServerTools::default(),
        models,
    );
    let server_key = SecretKey::generate();
    let server = endpoint(server_key.clone()).await;
    let server_id = server.id();
    let router = Router::builder(server)
        .accept(
            nodes::NODE_ALPN,
            nodes::NodeHandler::new(app.clone(), server_id),
        )
        .spawn();
    let apps = http::Local::start(app.clone(), &server_key, Pulse::manual())
        .await
        .expect("the apps' HTTP starts");
    let phone_key = SecretKey::generate();
    let user = app
        .sign_in(&phone_key.public())
        .await
        .unwrap()
        .expect("a new key is never revoked")
        .user;
    let node_key = SecretKey::generate();
    user.link_key(NewKey {
        endpoint: node_key.public(),
        name: "build-box",
        kind: DeviceKind::Desktop {},
    })
    .await
    .unwrap();
    // The node keeps its terminals in a host; with no beep-node binary
    // here to start one, it runs in this process.
    let terminals = TempDir::new().expect("a temp folder");
    let host = beep_node::terminal_host_in_process(terminals.path()).expect("a terminal host");
    tokio::spawn(host);
    let addr = router.endpoint().addr();
    let node = endpoint(node_key.clone()).await;
    let task = tokio::spawn(async move {
        beep_node::link::connect(&node, addr, "build-box", &Offers::default()).await
    });
    let machine = node_key.public().to_string();
    timeout(Duration::from_secs(20), async {
        while app.nodes.online(&machine).is_none() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("the node came online");
    let client = Client::new(apps.url(), phone_key).expect("a phone client");
    Setup {
        _db: db,
        router,
        _apps: apps,
        machine,
        client,
        _node: task,
        _terminals: terminals,
    }
}

#[tokio::test]
async fn a_shell_takes_keys_and_answers_through_the_server() {
    let setup = setup().await;
    let folder = TempDir::new().expect("a temp folder");
    let terminal = setup
        .client
        .call(&OpenTerminal {
            project: PROJECT.into(),
            machine_id: setup.machine.clone(),
            name: String::new(),
            command: String::new(),
            cwd: folder.path().to_string_lossy().into_owned(),
            cols: 80,
            rows: 24,
        })
        .await
        .expect("the terminal opens");
    assert_eq!(terminal.name, "shell");
    assert_eq!(terminal.state, TerminalState::Running {});

    let (mut keys, mut events) = setup
        .client
        .duplex(&AttachTerminal {
            id: terminal.id,
            from: 0,
            cols: 100,
            rows: 30,
        })
        .await
        .expect("it attaches");
    // The shell works the sum out, so the answer is not the echo of the keys.
    keys.send(&TerminalInput::Keys {
        data: ByteBuf::from(b"echo sum-$((40+2))\r".to_vec()),
    })
    .await
    .unwrap();
    let mut seen = Vec::new();
    let mut next = 0;
    timeout(Duration::from_secs(30), async {
        while !String::from_utf8_lossy(&seen).contains("sum-42") {
            match events.next().await.expect("the stream reads") {
                Some(TerminalEvent::Output { at, data }) => {
                    assert_eq!(at, next, "output comes in order");
                    next += data.len() as u64;
                    seen.extend_from_slice(&data);
                }
                other => panic!("expected output, got {other:?}"),
            }
        }
    })
    .await
    .expect("the shell answered");

    keys.send(&TerminalInput::Keys {
        data: ByteBuf::from(b"exit 0\r".to_vec()),
    })
    .await
    .unwrap();
    let ended = timeout(Duration::from_secs(30), async {
        loop {
            match events.next().await.expect("the stream reads") {
                Some(TerminalEvent::Output { .. }) => {}
                other => return other,
            }
        }
    })
    .await
    .expect("the shell ended");
    assert_eq!(
        ended,
        Some(TerminalEvent::Ended {
            state: TerminalState::Exited { code: Some(0) }
        })
    );
    let listed = setup
        .client
        .call(&ListTerminals {
            project: PROJECT.into(),
        })
        .await
        .unwrap();
    // An ended shell leaves the list on its own.
    assert!(listed.is_empty(), "still listed: {listed:?}");
    setup.router.shutdown().await.unwrap();
}
