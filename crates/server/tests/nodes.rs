//! A real `beep-node` against the server over iroh: only linked keys get
//! in, the environment report is stored, the shell keeps its state, a
//! coding agent (a scripted stand-in speaking ACP) runs in sessions that
//! outlive the connection, a protocol the host adds, such as `desktop/1`,
//! is offered and served, and the app sees the machine.

use async_trait::async_trait;
use beep_node::agents::{AgentKind, Agents};
use beep_node::link::{Offers, Protocol, Session};
use beep_server::protocol::node::{
    LinkRx, LinkTx,
    desktop::{self, Reply, Screenshot},
};
use beep_server::{
    app::{App, ServerTools},
    client::{Client, Node},
    models::ModelChoices,
    nodes::{self, Online},
    protocol::{
        ApiError, DeviceKind, ListMachines, ListMemoryScopes, MemoryScope, MemorySource,
        RemoveDevice,
        link::LinkOffer,
        node::agent::{EntryKind, Request, Status, StopReason},
    },
    push::Pusher,
    store::{Fact, NewKey, Scope, TestDb},
    transport,
    user::User,
};
use flashmind::types::{
    AgentLlmConfig, CompletionRequest, CompletionStream, LlmProvider, Provider,
};
use iroh::{Endpoint, EndpointAddr, SecretKey, endpoint::presets, protocol::Router};
use std::{
    sync::Arc,
    time::{Duration, SystemTime},
};
use tokio::time::{sleep, timeout};

/// The node tests never run a model turn.
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
        unreachable!("the node tests make no model calls")
    }
}

/// A desktop that answers screenshots with a counter, so the test can tell
/// one session from several.
struct FakeDesktop;

impl Protocol for FakeDesktop {
    fn name(&self) -> &str {
        nodes::DESKTOP
    }

    fn serve(&self, mut tx: LinkTx, mut rx: LinkRx) -> Session {
        Box::pin(async move {
            let mut served = 0;
            while let Some(request) = rx.recv::<desktop::Request>().await? {
                served += 1;
                let reply = match request {
                    desktop::Request::Screenshot { max_side } => Reply::Screenshot(Screenshot {
                        jpeg: vec![0xff, 0xd8, 0xff, served],
                        width: max_side,
                        height: max_side / 2,
                        screen_width: 1512,
                        screen_height: 945,
                    }),
                    _ => Reply::Failed {
                        reason: "not faked".into(),
                        missing: None,
                    },
                };
                tx.send(&reply).await?;
            }
            Ok(())
        })
    }
}

/// An app on `key` connected to `router`'s server, with the node it runs on.
async fn phone(key: SecretKey, router: &Router) -> (Node, Client) {
    let node = Node::bind(key, router.endpoint().addr(), Arc::new(|_| true))
        .await
        .expect("a phone binds");
    let client = node.connect().await.expect("a phone connects");
    (node, client)
}

fn memory(content: &str, scope: Scope) -> Fact<'_> {
    Fact {
        content,
        category: None,
        source: MemorySource::Learned {},
        valid: None,
        origin: None,
        scope: Some(scope),
    }
}

async fn endpoint(key: SecretKey) -> Endpoint {
    Endpoint::builder(presets::N0)
        .secret_key(key)
        .bind()
        .await
        .expect("a test endpoint binds")
}

/// Waits for the server to register `node` on a connection other than
/// `old`'s. Polls, because registering is the server's own business and has
/// no signal to wait on.
async fn online_after(app: &App, node: &str, old: Option<&Online>) -> Online {
    timeout(Duration::from_secs(20), async {
        loop {
            if let Some(online) = app.nodes.online(node)
                && old.is_none_or(|old| old.connection.stable_id() != online.connection.stable_id())
            {
                return online;
            }
            sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("the node came online")
}

async fn online(app: &App, node: &str) -> Online {
    online_after(app, node, None).await
}

/// A server with one admin user, listening for nodes.
struct Server {
    _db: TestDb,
    app: Arc<App>,
    router: Router,
    addr: EndpointAddr,
    user: Arc<User>,
}

async fn server() -> Server {
    let db = TestDb::new().await.unwrap();
    let store = db.store().await.unwrap();
    let alice = SecretKey::generate();
    let model = "openai:test".parse().unwrap();
    let models = ModelChoices::resolve(&NoModel, model, Vec::new(), &[alice.public()]).await;
    let app = App::new(
        store,
        Arc::new(NoModel),
        AgentLlmConfig::new("openai:test".parse().unwrap()),
        ServerTools::default(),
        models,
    );
    let server_endpoint = endpoint(SecretKey::generate()).await;
    let handler = nodes::NodeHandler::new(app.clone(), server_endpoint.id());
    let router = Router::builder(server_endpoint)
        .accept(nodes::NODE_ALPN, handler)
        .spawn();
    let addr = router.endpoint().addr();
    let user = app
        .sign_in(&alice.public())
        .await
        .unwrap()
        .expect("a new key is never revoked")
        .user;
    Server {
        _db: db,
        app,
        router,
        addr,
        user,
    }
}

/// Links a new node key to the server's user.
async fn linked_key(server: &Server, name: &str) -> SecretKey {
    let key = SecretKey::generate();
    server
        .user
        .link_key(NewKey {
            endpoint: key.public(),
            name,
            kind: DeviceKind::Desktop {},
        })
        .await
        .unwrap();
    key
}

#[tokio::test]
async fn a_linked_node_reports_and_runs_shell_commands() {
    let Server {
        _db,
        app,
        router,
        addr: server,
        user,
    } = server().await;

    // A key nobody linked is turned away.
    let stranger = endpoint(SecretKey::generate()).await;
    let refused =
        beep_node::link::connect(&stranger, server.clone(), "stranger", &Offers::default())
            .await
            .unwrap_err();
    assert!(format!("{refused:#}").contains("not linked"), "{refused:#}");

    let node_key = SecretKey::generate();
    user.link_key(NewKey {
        endpoint: node_key.public(),
        name: "build box",
        kind: DeviceKind::Desktop {},
    })
    .await
    .unwrap();
    let node = endpoint(node_key.clone()).await;
    let served = tokio::spawn({
        let server = server.clone();
        async move {
            let offers = Offers {
                agents: Arc::default(),
                extra: vec![Arc::new(FakeDesktop)],
            };
            beep_node::link::connect(&node, server, "build box", &offers).await
        }
    });

    let live = online(&app, &node_key.public().to_string()).await;
    assert_eq!(live.name, "build box");
    assert!(live.offers(nodes::SHELL), "{:?}", live.offers);
    assert!(live.offers(nodes::DESKTOP), "{:?}", live.offers);

    // Desktop requests share one session stream.
    for served in 1..=2 {
        let reply = live
            .desktop(&desktop::Request::Screenshot { max_side: 1280 })
            .await
            .unwrap();
        let Reply::Screenshot(shot) = reply else {
            panic!("expected a screenshot, got {reply:?}");
        };
        assert_eq!((shot.width, shot.height), (1280, 640));
        assert_eq!(shot.jpeg, [0xff, 0xd8, 0xff, served]);
    }

    let limit = Duration::from_secs(20);
    let ran = live
        .run_shell("cd / && export GREETING=hi && echo ready", limit)
        .await
        .unwrap();
    assert_eq!(ran.output, "ready\n");
    assert_eq!(ran.exit_code, Some(0));
    assert_eq!(ran.cwd.as_deref(), Some("/"));
    assert!(ran.fresh);

    let ran = live
        .run_shell(
            "echo $GREETING; pwd; exit_code_is() { return 4; }; exit_code_is",
            limit,
        )
        .await
        .unwrap();
    assert_eq!(ran.output, "hi\n/\n");
    assert_eq!(ran.exit_code, Some(4));
    assert!(!ran.fresh, "the same shell ran it");

    let ran = live
        .run_shell("sleep 30", Duration::from_secs(1))
        .await
        .unwrap();
    assert!(ran.timed_out);
    assert_eq!(ran.exit_code, None);

    let ran = live.run_shell("pwd", limit).await.unwrap();
    assert!(ran.fresh, "a timed out shell is replaced");
    assert_ne!(ran.output, "/\n");

    // The report arrived after the hello; by now it is stored.
    let rows = user.nodes().await.unwrap();
    let env = rows[0].env.as_ref().expect("the node reported");
    assert_eq!(env.os, std::env::consts::OS);

    router.shutdown().await.unwrap();
    // Closed or lost, either way the node's connection ends with the server.
    let _ = timeout(Duration::from_secs(10), served)
        .await
        .expect("the node noticed the server left")
        .expect("the node task did not panic");
}

/// The stand-in agent: `python3` running `testdata/fake_acp.py` from the node crate.
fn fake_agent() -> AgentKind {
    let script = concat!(env!("CARGO_MANIFEST_DIR"), "/../node/testdata/fake_acp.py");
    AgentKind {
        name: "fake".into(),
        program: "python3".into(),
        args: vec!["-I".into(), script.into()],
    }
}

#[tokio::test]
async fn a_node_runs_a_coding_agent_that_outlives_its_stream() {
    let server = server().await;
    let app = &server.app;
    let node_key = linked_key(&server, "build box").await;
    let node_id = node_key.public().to_string();
    // Made once, as `beep-node run` does, so sessions outlive connections.
    let agents = Arc::new(Agents::new(vec![fake_agent()]));
    let connect = || {
        let (key, addr, agents) = (node_key.clone(), server.addr.clone(), agents.clone());
        tokio::spawn(async move {
            let node = endpoint(key).await;
            let offers = Offers {
                agents,
                extra: Vec::new(),
            };
            beep_node::link::connect(&node, addr, "build box", &offers).await
        })
    };
    let served = connect();
    let live = online(app, &node_id).await;
    assert!(live.offers("fake/1"), "{:?}", live.offers);
    assert!(live.offers("agents/1"), "{:?}", live.offers);

    let cwd = std::env::temp_dir().join(format!("beep-agent-test-{}", std::process::id()));
    std::fs::create_dir_all(&cwd).unwrap();
    let wait = Duration::from_secs(20);

    // A start that asks permission comes back with the question.
    let link = live
        .start_agent(
            "fake",
            Some(cwd.display().to_string()),
            "please edit the notes".into(),
        )
        .await
        .unwrap();
    let mark = link.now().mark();
    let asked = link.wait(mark, wait).await;
    let id = asked.info.id.clone();
    assert_eq!(asked.info.state.status, Status::Asking);
    let question = asked.info.question.clone().expect("the agent asked");
    assert_eq!(question.text, "Edit notes.txt");
    assert_eq!(question.options.len(), 2);
    assert_eq!(asked.info.state.plan.len(), 2);

    // The connection drops; the session waits on the node and a new
    // stream attaches to it, question and all.
    served.abort();
    drop(link);
    let served = connect();
    let live = online_after(app, &node_id, Some(&live)).await;
    let link = live.agent_session("fake", &id).await.unwrap();
    let now = link.now();
    assert_eq!(now.info.state.status, Status::Asking);
    assert_eq!(now.info.question.as_ref().map(|q| q.id), Some(question.id));

    link.send(Request::Answer {
        question: question.id,
        reply: "allow".into(),
    })
    .await
    .unwrap();
    let done = link.wait(now.mark(), wait).await;
    assert_eq!(done.info.turns, 1);
    assert_eq!(done.last_stop, Some(StopReason::EndTurn));
    assert_eq!(done.info.last_message, "Edited notes.txt");
    assert_eq!(done.info.state.status, Status::Idle);
    assert_eq!(done.info.state.files_touched, ["notes.txt"]);

    // Text that is no option declines, then goes to the agent as a prompt.
    link.send(Request::Prompt {
        text: "edit again".into(),
    })
    .await
    .unwrap();
    let asked = link.wait(done.mark(), wait).await;
    let question = asked.info.question.clone().expect("asked again");
    link.send(Request::Answer {
        question: question.id,
        reply: "write it in other.txt instead".into(),
    })
    .await
    .unwrap();
    // The link keeps only the latest state, so the declined turn may already
    // be followed by the prompt's turn when the wait returns.
    let mut echoed = link.wait(asked.mark(), wait).await;
    if echoed.info.turns == 2 {
        assert_eq!(echoed.info.last_message, "Left notes.txt alone");
        echoed = link.wait(echoed.mark(), wait).await;
    }
    assert_eq!(echoed.info.turns, 3);
    assert_eq!(
        echoed.info.last_message,
        "Echo: write it in other.txt instead"
    );

    // A turn that runs on is cancelled.
    link.send(Request::Prompt {
        text: "wait for me".into(),
    })
    .await
    .unwrap();
    let working = link.wait(echoed.mark(), Duration::from_millis(300)).await;
    assert_eq!(working.info.state.status, Status::Working);
    assert_eq!(working.info.state.summary, "Wait");
    link.send(Request::Cancel {}).await.unwrap();
    let cancelled = link.wait(working.mark(), wait).await;
    assert_eq!(cancelled.last_stop, Some(StopReason::Cancelled));
    assert_eq!(cancelled.info.turns, 4);

    let sessions = live.agent_sessions().await.unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].id, id);
    assert_eq!(sessions[0].agent, "fake");
    let (entries, earlier) = live.transcript(&id, 100).await.unwrap();
    assert_eq!(earlier, 0);
    assert_eq!(entries[0].kind, EntryKind::User);
    assert_eq!(entries[0].text, "please edit the notes");
    assert!(
        entries
            .iter()
            .any(|entry| entry.kind == EntryKind::Question && entry.text == "Answered: Allow"),
        "{entries:?}"
    );
    let (last, earlier) = live.transcript(&id, 2).await.unwrap();
    assert_eq!(last.len(), 2);
    assert_eq!(earlier as usize, entries.len() - 2);

    link.send(Request::Close {}).await.unwrap();
    let closed = link.wait(cancelled.mark(), wait).await;
    assert_eq!(closed.ended.as_deref(), Some("closed"));

    let refused = live
        .start_agent("fake", Some("/no/such/folder".into()), "hi".into())
        .await
        .err()
        .expect("a missing folder is refused");
    assert!(format!("{refused:#}").contains("no folder"), "{refused:#}");
    let refused = live
        .agent_session("fake", "000000")
        .await
        .err()
        .expect("no such session");
    assert!(format!("{refused:#}").contains("no session"), "{refused:#}");

    std::fs::remove_dir_all(&cwd).unwrap();
    served.abort();
    server.router.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_machine_links_from_the_link_its_node_printed() {
    let server = server().await;
    let key = SecretKey::generate();
    let server_id = server.router.endpoint().id();
    let offer = LinkOffer::sign(&key, server_id, "build box", SystemTime::now()).unwrap();

    let check_code = nodes::link_machine(&server.app.nodes, &server.user, &offer.code())
        .await
        .unwrap_err();
    assert!(
        format!("{check_code:#}").contains("check code"),
        "{check_code:#}"
    );
    let other_server = SecretKey::generate().public();
    let elsewhere = LinkOffer::sign(&key, other_server, "build box", SystemTime::now()).unwrap();
    let refused = nodes::link_machine(&server.app.nodes, &server.user, &elsewhere.to_uri())
        .await
        .unwrap_err();
    assert!(format!("{refused:#}").contains("signature"), "{refused:#}");
    let old = SystemTime::now() - Duration::from_secs(3600);
    let expired = LinkOffer::sign(&key, server_id, "build box", old).unwrap();
    let refused = nodes::link_machine(&server.app.nodes, &server.user, &expired.to_uri())
        .await
        .unwrap_err();
    assert!(format!("{refused:#}").contains("expired"), "{refused:#}");

    let name = nodes::link_machine(&server.app.nodes, &server.user, &offer.to_uri())
        .await
        .unwrap();
    assert_eq!(name, "build box");
    let again = nodes::link_machine(&server.app.nodes, &server.user, &offer.to_uri())
        .await
        .unwrap_err();
    assert!(format!("{again:#}").contains("already linked"), "{again:#}");

    // The linked key now gets in as a node of the user.
    let agents = Arc::new(Agents::new(Vec::new()));
    let served = tokio::spawn({
        let addr = server.addr.clone();
        async move {
            let node = endpoint(key).await;
            let offers = Offers {
                agents,
                extra: Vec::new(),
            };
            beep_node::link::connect(&node, addr, "build box", &offers).await
        }
    });
    let live = online(&server.app, &offer.device.to_string()).await;
    assert_eq!(live.name, "build box");
    served.abort();
    server.router.shutdown().await.unwrap();
}

/// A smoke run against the real Claude Code ACP adapter. It needs `claude`
/// signed in, `npx` and the network, so it only runs when asked:
/// `cargo test -p beep-server --test nodes -- --ignored real_claude`.
#[tokio::test]
#[ignore = "runs the real Claude Code"]
async fn a_real_claude_session_writes_a_file() {
    let server = server().await;
    let node_key = linked_key(&server, "build box").await;
    let node_id = node_key.public().to_string();
    let claude = beep_node::agents::detect(&[])
        .into_iter()
        .find(|kind| kind.name == "claude")
        .expect("claude is installed");
    // The smallest model is plenty for one file.
    let mut args = vec!["ANTHROPIC_MODEL=haiku".to_owned(), claude.program];
    args.extend(claude.args);
    let agents = Arc::new(Agents::new(vec![AgentKind {
        name: "claude".into(),
        program: "env".into(),
        args,
    }]));
    let served = tokio::spawn({
        let addr = server.addr.clone();
        async move {
            let node = endpoint(node_key).await;
            let offers = Offers {
                agents,
                extra: Vec::new(),
            };
            beep_node::link::connect(&node, addr, "build box", &offers).await
        }
    });
    let live = online(&server.app, &node_id).await;
    let cwd = std::env::temp_dir().join(format!("beep-claude-smoke-{}", std::process::id()));
    std::fs::create_dir_all(&cwd).unwrap();
    let link = live
        .start_agent(
            "claude",
            Some(cwd.display().to_string()),
            "Create a file named hello.txt containing the word hi. Then reply with just: done"
                .into(),
        )
        .await
        .unwrap();
    let wait = Duration::from_secs(180);
    let mut now = link.wait(link.now().mark(), wait).await;
    while let Some(question) = now.info.question.clone() {
        eprintln!("question: {question:?}");
        link.send(Request::Answer {
            question: question.id,
            reply: "allow".into(),
        })
        .await
        .unwrap();
        now = link.wait(now.mark(), wait).await;
    }
    eprintln!("state: {now:#?}");
    assert_eq!(now.info.turns, 1, "{now:#?}");
    assert_eq!(
        std::fs::read_to_string(cwd.join("hello.txt"))
            .unwrap()
            .trim(),
        "hi"
    );
    link.send(Request::Close {}).await.unwrap();
    std::fs::remove_dir_all(&cwd).unwrap();
    served.abort();
    server.router.shutdown().await.unwrap();
}

#[tokio::test]
async fn an_admin_sees_their_machines_and_what_memories_are_about() {
    let db = TestDb::new().await.unwrap();
    let store = db.store().await.unwrap();
    let alice = SecretKey::generate();
    let model = "openai:test".parse().unwrap();
    let models = ModelChoices::resolve(&NoModel, model, Vec::new(), &[alice.public()]).await;
    let app = App::new(
        store.clone(),
        Arc::new(NoModel),
        AgentLlmConfig::new("openai:test".parse().unwrap()),
        ServerTools::default(),
        models,
    );
    let server_endpoint = endpoint(SecretKey::generate()).await;
    let pusher = Pusher::with_retry_waits(
        server_endpoint.clone(),
        Arc::new(store),
        None,
        vec![Duration::from_millis(200)],
    );
    let server_id = server_endpoint.id();
    let router = Router::builder(server_endpoint)
        .accept(
            nodes::NODE_ALPN,
            nodes::NodeHandler::new(app.clone(), server_id),
        )
        .accept(
            transport::ALPN,
            transport::BeepHandler::new(app.clone(), pusher),
        )
        .spawn();
    let server = router.endpoint().addr();

    let user = app
        .sign_in(&alice.public())
        .await
        .unwrap()
        .expect("a new key is never revoked")
        .user;
    let node_key = SecretKey::generate();
    user.link_key(NewKey {
        endpoint: node_key.public(),
        name: "build box",
        kind: DeviceKind::Desktop {},
    })
    .await
    .unwrap();
    let node = endpoint(node_key.clone()).await;
    let served = tokio::spawn(async move {
        beep_node::link::connect(&node, server, "build box", &Offers::default()).await
    });
    let node_id = node_key.public().to_string();
    online(&app, &node_id).await;
    // The report follows the hello on its own; wait until it is stored.
    timeout(Duration::from_secs(20), async {
        while user.nodes().await.unwrap()[0].env.is_none() {
            sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("the node reported");

    let (_alice_node, alice_app) = phone(alice.clone(), &router).await;
    let machines = alice_app.call(&ListMachines {}).await.unwrap();
    assert_eq!(machines.len(), 1);
    assert_eq!(machines[0].id, node_id);
    assert_eq!(machines[0].name, "build box");
    assert!(machines[0].online);
    let env = machines[0]
        .env
        .as_ref()
        .expect("the report reaches the app");
    assert_eq!(env.os, std::env::consts::OS);

    user.add_memory(memory(
        "Has the beep repo in /srv/beep.",
        Scope::Machine(node_key.public()),
    ))
    .await
    .unwrap();
    user.add_memory(memory("Ships from main.", Scope::project("Beep").unwrap()))
        .await
        .unwrap();
    user.add_memory(memory(
        "Signs in to GitHub as build-user.",
        Scope::account("github").unwrap(),
    ))
    .await
    .unwrap();
    let about_user = user
        .add_memory(Fact {
            scope: None,
            ..memory("Lives in Lisbon.", Scope::User)
        })
        .await
        .unwrap();
    let mut scopes: Vec<MemoryScope> = alice_app
        .call(&ListMemoryScopes {})
        .await
        .unwrap()
        .into_iter()
        .inspect(|scoped| assert_ne!(scoped.id, about_user))
        .map(|scoped| scoped.scope)
        .collect();
    scopes.sort_by_key(|scope| format!("{scope:?}"));
    assert_eq!(
        scopes,
        [
            MemoryScope::Account {
                app: "github".into()
            },
            MemoryScope::Machine {
                id: node_id.clone(),
                name: Some("build box".into())
            },
            MemoryScope::Project {
                name: "beep".into()
            },
        ]
    );

    // Someone who is not an admin gets no machines.
    let (_bob_node, bob_app) = phone(SecretKey::generate(), &router).await;
    let refused = bob_app.call(&ListMachines {}).await.unwrap_err();
    assert!(matches!(
        refused.downcast::<ApiError>().unwrap(),
        ApiError::Unauthorized {}
    ));
    assert!(bob_app.call(&ListMemoryScopes {}).await.unwrap().is_empty());

    // An unlinked machine leaves the list.
    alice_app.call(&RemoveDevice { id: node_id }).await.unwrap();
    assert!(alice_app.call(&ListMachines {}).await.unwrap().is_empty());

    router.shutdown().await.unwrap();
    let _ = timeout(Duration::from_secs(10), served).await;
}
