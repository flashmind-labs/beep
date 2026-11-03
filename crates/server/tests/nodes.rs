//! A real `beep-node` against the server over iroh: only linked keys get
//! in, the environment report is stored, the shell keeps its state, a
//! coding agent (a scripted stand-in speaking ACP) runs in sessions that
//! outlive the connection, a protocol the host adds, such as `desktop/1`,
//! is offered and served, and the app sees the machine.

use async_trait::async_trait;
use beep_node::agents::{AgentKind, Agents};
use beep_node::link::{Offers, Protocol, Session};
use beep_server::protocol::node::{
    AppInfo, AppName, GpuInfo, GpuVendor, LinkRx, LinkTx,
    desktop::{self, Reply, Screenshot},
};
use beep_server::{
    app::{App, ServerTools},
    client::{Client, Node},
    models::ModelChoices,
    nodes::{self, Online, StartOn},
    protocol::{
        AgentCheckoutPlan, AgentEffort, AgentEntryKind, AgentImageData, AgentModel, AgentModelPick,
        AgentModels, AgentPlanStatus, AgentStatus, AgentSteer, AnswerAgentQuestion, ApiError,
        CreateNodeToken, DeviceKind, GetAgentImage, GetAgentSession, GetNodeToken, HarnessSettings,
        ListAgentHosts, ListAgentImages, ListAgentModels, ListAgentSessions, ListMachineDetails,
        ListMachineStats, ListMachines, ListMemoryScopes, ListProjectSessions, MachineCapability,
        MemoryScope, MemorySource, PreviewAgentMachine, RemoveDevice, RemoveMachine,
        RevokeNodeToken, SaveHarnessSettings, StartAgentSession, SteerAgentSession, Upload,
        image_key,
        link::LinkOffer,
        node::{
            EnvReport, NodeSetup,
            agent::{
                AnswerOption, CheckoutPlan, Effort, Entry, EntryKind, Event, ExternalSession,
                ModelList, ModelOption, ModelPick, OptionKind, PlanEntry, PlanStatus, Presence,
                Question, Request, SESSIONS_PROTOCOL, SessionInfo, SessionsReply, SessionsRequest,
                State, Status, StopReason,
            },
        },
        setup::SetupToken,
    },
    push::Pusher,
    store::{Fact, NewImport, NewKey, NewNode, Scope, TestDb},
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
use tokio::{
    sync::mpsc,
    time::{sleep, timeout},
};

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

/// A server with one user, listening for nodes.
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
    let models = ModelChoices::resolve(&NoModel, model, Vec::new()).await;
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
                provision: None,
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
        env: Default::default(),
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
                provision: None,
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

    // A start that puts a choice to the user comes back with the question.
    let link = live
        .start_agent(
            "fake",
            Some(cwd.display().to_string()),
            "please pick the notes".into(),
            StartOn::default(),
        )
        .await
        .unwrap();
    let mark = link.now().mark();
    let asked = link.wait(mark, wait).await;
    let id = asked.info.id.clone();
    assert_eq!(asked.info.state.status, Status::Asking);
    let question = asked.info.question.clone().expect("the agent asked");
    assert_eq!(question.text, "Which file?");
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
        text: "pick again".into(),
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

    // A permission request is allowed on the node, never asked.
    link.send(Request::Prompt {
        text: "edit the notes".into(),
    })
    .await
    .unwrap();
    let edited = link.wait(cancelled.mark(), wait).await;
    assert_eq!(edited.info.turns, 5, "{edited:?}");
    assert!(edited.info.question.is_none());
    assert_eq!(edited.info.last_message, "Edited notes.txt");

    let sessions = live.agent_sessions().await.unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].id, id);
    assert_eq!(sessions[0].agent, "fake");
    let (entries, earlier) = live.transcript(&id, 100).await.unwrap();
    assert_eq!(earlier, 0);
    assert_eq!(entries[0].kind, EntryKind::User);
    assert_eq!(entries[0].text, "please pick the notes");
    assert!(
        entries
            .iter()
            .any(|entry| entry.kind == EntryKind::Question && entry.text == "Answered: notes.txt"),
        "{entries:?}"
    );
    assert!(
        entries
            .iter()
            .any(|entry| entry.kind == EntryKind::Question
                && entry.text == "Allowed: Edit notes.txt"),
        "{entries:?}"
    );
    let (last, earlier) = live.transcript(&id, 2).await.unwrap();
    assert_eq!(last.len(), 2);
    assert_eq!(earlier as usize, entries.len() - 2);

    link.send(Request::Close {}).await.unwrap();
    let closed = link.wait(edited.mark(), wait).await;
    assert_eq!(closed.ended.as_deref(), Some("closed"));

    let refused = live
        .start_agent(
            "fake",
            Some("/no/such/folder".into()),
            "hi".into(),
            StartOn::default(),
        )
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
                provision: None,
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
        env: claude.env,
    }]));
    let served = tokio::spawn({
        let addr = server.addr.clone();
        async move {
            let node = endpoint(node_key).await;
            let offers = Offers {
                agents,
                extra: Vec::new(),
                provision: None,
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
            StartOn::default(),
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
async fn a_user_sees_their_machines_and_what_memories_are_about() {
    let db = TestDb::new().await.unwrap();
    let store = db.store().await.unwrap();
    let alice = SecretKey::generate();
    let model = "openai:test".parse().unwrap();
    let models = ModelChoices::resolve(&NoModel, model, Vec::new()).await;
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

    // Stats follow the report on their own, kept in memory.
    let stats = timeout(Duration::from_secs(20), async {
        loop {
            let stats = alice_app.call(&ListMachineStats {}).await.unwrap();
            if !stats.is_empty() {
                return stats;
            }
            sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("the node sent stats");
    assert_eq!(stats.len(), 1);
    assert_eq!(stats[0].id, node_id);
    assert!(stats[0].memory_total_mb > 0);
    assert!(!stats[0].disks.is_empty(), "the home folder is measured");

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

    // Someone else sees none of alice's machines.
    let (_bob_node, bob_app) = phone(SecretKey::generate(), &router).await;
    assert!(bob_app.call(&ListMachines {}).await.unwrap().is_empty());
    assert!(bob_app.call(&ListMemoryScopes {}).await.unwrap().is_empty());

    // An unlinked machine leaves the list.
    alice_app.call(&RemoveDevice { id: node_id }).await.unwrap();
    assert!(alice_app.call(&ListMachines {}).await.unwrap().is_empty());

    router.shutdown().await.unwrap();
    let _ = timeout(Duration::from_secs(10), served).await;
}

#[tokio::test]
async fn any_app_of_the_account_removes_a_machine_online_or_not() {
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
    let sign_in = async |key: &SecretKey| {
        app.sign_in(&key.public())
            .await
            .unwrap()
            .expect("a new key is never revoked")
            .user
    };
    let link = async |user: &User, key: &SecretKey, name: &str| {
        user.link_key(NewKey {
            endpoint: key.public(),
            name,
            kind: DeviceKind::Desktop {},
        })
        .await
        .unwrap();
    };
    let not_found = |error: anyhow::Error| {
        matches!(error.downcast::<ApiError>().unwrap(), ApiError::NotFound {})
    };

    // Alice's phone made the account; her laptop's app is a linked key and
    // removes the machines.
    let user = sign_in(&SecretKey::generate()).await;
    let laptop = SecretKey::generate();
    link(&user, &laptop, "laptop").await;
    // A machine that went away and never came back.
    let gone = SecretKey::generate();
    link(&user, &gone, "old box").await;
    user.node_connected(NewNode {
        endpoint: gone.public(),
        name: "old box",
        version: "0.1.0",
    })
    .await
    .unwrap();
    // And one that is connected now.
    let live = SecretKey::generate();
    link(&user, &live, "build box").await;
    let node = endpoint(live.clone()).await;
    let served = tokio::spawn(async move {
        beep_node::link::connect(&node, server, "build box", &Offers::default()).await
    });
    let live_id = live.public().to_string();
    online(&app, &live_id).await;
    // Bob's machine, which alice's apps cannot touch.
    let bob = sign_in(&SecretKey::generate()).await;
    let bobs = SecretKey::generate();
    link(&bob, &bobs, "bob box").await;
    bob.node_connected(NewNode {
        endpoint: bobs.public(),
        name: "bob box",
        version: "0.1.0",
    })
    .await
    .unwrap();

    let (_laptop_node, client) = phone(laptop.clone(), &router).await;
    assert_eq!(client.call(&ListMachines {}).await.unwrap().len(), 2);

    // The offline machine goes at once and its key is refused from then on.
    let gone_id = gone.public().to_string();
    client
        .call(&RemoveMachine {
            machine_id: gone_id.clone(),
        })
        .await
        .unwrap();
    let left = client.call(&ListMachines {}).await.unwrap();
    assert_eq!(
        left.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
        [live_id.as_str()]
    );
    assert!(app.sign_in(&gone.public()).await.unwrap().is_none());
    let again = client
        .call(&RemoveMachine {
            machine_id: gone_id,
        })
        .await;
    assert!(not_found(again.unwrap_err()));

    // The connected one is dropped, and its node told why when it tries
    // again.
    client
        .call(&RemoveMachine {
            machine_id: live_id.clone(),
        })
        .await
        .unwrap();
    let dropped = timeout(Duration::from_secs(10), served)
        .await
        .expect("the node's connection was dropped")
        .unwrap()
        .unwrap_err();
    assert!(format!("{dropped:#}").contains("code 403"), "{dropped:#}");
    assert!(app.nodes.online(&live_id).is_none());
    assert!(client.call(&ListMachines {}).await.unwrap().is_empty());
    let back = beep_node::link::connect(
        &endpoint(live).await,
        server_id.into(),
        "build box",
        &Offers::default(),
    )
    .await
    .unwrap_err();
    assert!(
        format!("{back:#}").contains("was removed from one"),
        "{back:#}"
    );

    // Not another account's machine, and not the app's own key.
    let bobs_id = bobs.public().to_string();
    let theirs = client
        .call(&RemoveMachine {
            machine_id: bobs_id.clone(),
        })
        .await;
    assert!(not_found(theirs.unwrap_err()));
    assert_eq!(bob.nodes().await.unwrap()[0].endpoint, bobs_id);
    let own = client
        .call(&RemoveMachine {
            machine_id: laptop.public().to_string(),
        })
        .await
        .unwrap_err();
    assert!(matches!(
        own.downcast::<ApiError>().unwrap(),
        ApiError::BadRequest { .. }
    ));
    let junk = client
        .call(&RemoveMachine {
            machine_id: "not a key".into(),
        })
        .await;
    assert!(not_found(junk.unwrap_err()));

    router.shutdown().await.unwrap();
}

fn gpu_report() -> EnvReport {
    EnvReport {
        gpus: vec![GpuInfo {
            vendor: GpuVendor::Nvidia,
            model: "NVIDIA GeForce RTX 4090".into(),
            vram_mb: Some(24564),
            driver: Some("560.35".into()),
        }],
        apps: vec![AppInfo {
            name: AppName::Blender,
            version: Some("4.2".into()),
        }],
        ..EnvReport::default()
    }
}

/// A router with the app's own protocol on it, for phones to call.
async fn phone_router(app: &Arc<App>, store: beep_server::store::Store) -> Router {
    let server_endpoint = endpoint(SecretKey::generate()).await;
    let pusher = Pusher::with_retry_waits(
        server_endpoint.clone(),
        Arc::new(store),
        None,
        vec![Duration::from_millis(200)],
    );
    Router::builder(server_endpoint)
        .accept(
            transport::ALPN,
            transport::BeepHandler::new(app.clone(), pusher),
        )
        .spawn()
}

#[tokio::test]
async fn machine_details_lists_apps_gpus_and_capabilities() {
    let db = TestDb::new().await.unwrap();
    let store = db.store().await.unwrap();
    let alice = SecretKey::generate();
    let model = "openai:test".parse().unwrap();
    let models = ModelChoices::resolve(&NoModel, model, Vec::new()).await;
    let app = App::new(
        store.clone(),
        Arc::new(NoModel),
        AgentLlmConfig::new("openai:test".parse().unwrap()),
        ServerTools::default(),
        models,
    );
    let router = phone_router(&app, store).await;
    let user = app
        .sign_in(&alice.public())
        .await
        .unwrap()
        .expect("a new key is never revoked")
        .user;
    let mut linked = Vec::new();
    for name in ["gpu box", "new box", "gone box"] {
        let key = SecretKey::generate();
        if name != "gone box" {
            user.link_key(NewKey {
                endpoint: key.public(),
                name,
                kind: DeviceKind::Desktop {},
            })
            .await
            .unwrap();
        }
        user.node_connected(NewNode {
            endpoint: key.public(),
            name,
            version: "0.1.0",
        })
        .await
        .unwrap();
        linked.push(key.public());
    }
    user.save_node_env(&linked[0], &gpu_report()).await.unwrap();
    user.save_node_env(&linked[2], &gpu_report()).await.unwrap();

    let (_node, client) = phone(alice, &router).await;
    let mut details = client.call(&ListMachineDetails {}).await.unwrap();
    details.sort_by_key(|machine| machine.apps.len());
    // The machine without a report is listed, empty; the unlinked one is not.
    assert_eq!(details.len(), 2);
    assert_eq!(details[0].id, linked[1].to_string());
    assert!(details[0].apps.is_empty() && details[0].capabilities.is_empty());
    let gpu = &details[1];
    assert_eq!(gpu.id, linked[0].to_string());
    assert_eq!(
        gpu.capabilities,
        [
            MachineCapability::Gpu {},
            MachineCapability::Cuda {},
            MachineCapability::Blender {}
        ]
    );
    assert_eq!(gpu.apps[0].name, "Blender");
    assert_eq!(gpu.apps[0].version.as_deref(), Some("4.2"));
    assert_eq!(gpu.gpus[0].vendor, "NVIDIA");
    assert_eq!(gpu.gpus[0].vram_mb, Some(24564));
    router.shutdown().await.unwrap();
}

#[tokio::test]
async fn machine_details_of_a_user_without_machines_are_empty() {
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
    let router = phone_router(&app, store).await;
    let (_node, bob) = phone(SecretKey::generate(), &router).await;
    assert!(bob.call(&ListMachineDetails {}).await.unwrap().is_empty());
    assert!(bob.call(&ListMachineStats {}).await.unwrap().is_empty());
    router.shutdown().await.unwrap();
}

/// What a fake node's `agents/1` and `claude/1` answer.
#[derive(Clone, Default)]
struct FakeAgents {
    /// The Beep sessions `List` gives.
    sessions: Vec<SessionInfo>,
    /// The sessions `External` gives, by agent; `None` answers an error.
    external: Option<Vec<ExternalSession>>,
    /// Takes requests and never answers.
    hang: bool,
    /// Where the `claude/1` stream hands the orders it gets.
    orders: Option<mpsc::UnboundedSender<Request>>,
    /// The PNGs `Image` finds, by session.
    images: Vec<(String, Vec<u8>)>,
    /// The PNGs `FollowImages` sends, by session, before it holds the
    /// stream.
    sent: Vec<(String, Vec<u8>)>,
    /// What `Models` answers; `None` closes the stream unanswered, as a
    /// node from before it does.
    models: Option<ModelList>,
}

impl FakeAgents {
    fn offers(self) -> Offers {
        let fake = Arc::new(self);
        Offers {
            agents: Arc::default(),
            extra: vec![
                Arc::new(FakeSessions(fake.clone())),
                Arc::new(FakeClaude(fake)),
            ],
            provision: None,
        }
    }
}

struct FakeSessions(Arc<FakeAgents>);

impl Protocol for FakeSessions {
    fn name(&self) -> &str {
        SESSIONS_PROTOCOL
    }

    fn serve(&self, mut tx: LinkTx, mut rx: LinkRx) -> Session {
        let fake = self.0.clone();
        Box::pin(async move {
            let Some(request) = rx.recv::<SessionsRequest>().await? else {
                return Ok(());
            };
            if fake.hang {
                // Holds the stream until the server drops it.
                while rx.recv::<SessionsRequest>().await?.is_some() {}
                return Ok(());
            }
            if matches!(request, SessionsRequest::FollowImages {}) {
                for (session, data) in &fake.sent {
                    tx.send(&SessionsReply::SessionImage {
                        session: session.clone(),
                        mime: "image/png".into(),
                        data: data.clone(),
                    })
                    .await?;
                }
                while rx.recv::<SessionsRequest>().await?.is_some() {}
                return Ok(());
            }
            if let SessionsRequest::Models { agent } = &request {
                let Some(models) = fake.models.clone() else {
                    return Ok(());
                };
                let reply = if agent == "claude" {
                    SessionsReply::Models { models }
                } else {
                    SessionsReply::Error {
                        message: format!("no agent {agent}"),
                    }
                };
                return tx.send(&reply).await;
            }
            let reply = match (request, &fake.external) {
                (SessionsRequest::List {}, _) => SessionsReply::Sessions {
                    sessions: fake.sessions.clone(),
                },
                (SessionsRequest::Transcript { session, last }, _) => SessionsReply::Transcript {
                    entries: vec![Entry {
                        kind: EntryKind::Agent,
                        text: format!("{session}: the last {last}"),
                    }],
                    earlier: 4,
                },
                (SessionsRequest::External { agent, .. }, Some(external)) => {
                    SessionsReply::External {
                        sessions: external
                            .iter()
                            .filter(|session| session.agent == agent)
                            .cloned()
                            .collect(),
                    }
                }
                (SessionsRequest::InFolders { .. }, Some(external)) => SessionsReply::External {
                    sessions: external.clone(),
                },
                (SessionsRequest::Image { session, key }, _) => fake
                    .images
                    .iter()
                    .find(|(of, data)| *of == session && image_key(data) == key)
                    .map_or_else(
                        || SessionsReply::Error {
                            message: "no such image".into(),
                        },
                        |(_, data)| SessionsReply::Image {
                            mime: "image/png".into(),
                            data: data.clone(),
                        },
                    ),
                _ => SessionsReply::Error {
                    message: "not faked".into(),
                },
            };
            tx.send(&reply).await
        })
    }
}

struct FakeClaude(Arc<FakeAgents>);

impl Protocol for FakeClaude {
    fn name(&self) -> &str {
        "claude/1"
    }

    fn serve(&self, mut tx: LinkTx, mut rx: LinkRx) -> Session {
        let fake = self.0.clone();
        Box::pin(async move {
            let first = rx.recv::<Request>().await?;
            if let Some(Request::Start { cwd, prompt, .. }) = &first {
                if let Some(orders) = &fake.orders {
                    orders
                        .send(first.clone().expect("a start"))
                        .expect("the test still listens");
                }
                let info = SessionInfo {
                    id: "b0b0b0".into(),
                    agent: "claude".into(),
                    cwd: cwd.clone().unwrap_or_else(|| "/home/user".into()),
                    title: Some(prompt.clone()),
                    started_at: 1_760_000_300,
                    ..SessionInfo::default()
                };
                return tx
                    .send(&Event::Attached {
                        session: Box::new(info),
                    })
                    .await;
            }
            let Some(Request::Attach { session }) = first else {
                return Ok(());
            };
            let Some(info) = fake.sessions.iter().find(|info| info.id == session) else {
                let reason = "no session".into();
                return tx.send(&Event::Refused { reason }).await;
            };
            tx.send(&Event::Attached {
                session: Box::new(info.clone()),
            })
            .await?;
            while let Some(request) = rx.recv::<Request>().await? {
                if let Some(orders) = &fake.orders {
                    orders.send(request).expect("the test still listens");
                }
                tx.send(&Event::Ack { error: None }).await?;
            }
            Ok(())
        })
    }
}

/// A Beep session of Claude asking `question`.
fn beep_session(id: &str, question: Option<u32>) -> SessionInfo {
    SessionInfo {
        id: id.into(),
        agent: "claude".into(),
        cwd: "/srv/project".into(),
        title: Some("Fix the build".into()),
        started_at: 1_760_000_000,
        state: State {
            status: if question.is_some() {
                Status::Asking
            } else {
                Status::Working
            },
            summary: "Run cargo test".into(),
            ..State::default()
        },
        question: question.map(|id| Question {
            id,
            text: "Run cargo test".into(),
            detail: Some("cargo test -p beep".into()),
            options: vec![
                AnswerOption {
                    id: "allow".into(),
                    name: "Allow".into(),
                    kind: OptionKind::AllowOnce,
                },
                AnswerOption {
                    id: "reject".into(),
                    name: "Reject".into(),
                    kind: OptionKind::RejectOnce,
                },
            ],
        }),
        ..SessionInfo::default()
    }
}

fn external_session(id: &str, presence: Presence) -> ExternalSession {
    ExternalSession {
        id: id.into(),
        agent: "claude".into(),
        cwd: "/srv/project".into(),
        title: Some("Write the docs".into()),
        started_at: 1_760_000_100,
        last_activity: 1_760_000_200,
        presence,
        ..ExternalSession::default()
    }
}

/// A server that takes nodes and phones, with `users` users.
struct CommandCenter {
    _db: TestDb,
    app: Arc<App>,
    router: Router,
    keys: Vec<SecretKey>,
    users: Vec<Arc<User>>,
    nodes: Vec<tokio::task::JoinHandle<anyhow::Result<()>>>,
}

impl CommandCenter {
    async fn new(users: usize) -> Self {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let keys: Vec<SecretKey> = (0..users).map(|_| SecretKey::generate()).collect();
        let model = "openai:test".parse().unwrap();
        let models = ModelChoices::resolve(&NoModel, model, Vec::new()).await;
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
        let mut users = Vec::new();
        for key in &keys {
            let user = app
                .sign_in(&key.public())
                .await
                .unwrap()
                .expect("a new key is never revoked")
                .user;
            users.push(user);
        }
        Self {
            _db: db,
            app,
            router,
            keys,
            users,
            nodes: Vec::new(),
        }
    }

    /// Links a node named `name` to user `user`, connects it serving
    /// `fake`, and waits until it is online. Gives its machine id.
    async fn node(&mut self, user: usize, name: &'static str, fake: FakeAgents) -> String {
        let key = SecretKey::generate();
        self.users[user]
            .link_key(NewKey {
                endpoint: key.public(),
                name,
                kind: DeviceKind::Desktop {},
            })
            .await
            .unwrap();
        let addr = self.router.endpoint().addr();
        let node = endpoint(key.clone()).await;
        self.nodes.push(tokio::spawn(async move {
            beep_node::link::connect(&node, addr, name, &fake.offers()).await
        }));
        let id = key.public().to_string();
        online(&self.app, &id).await;
        id
    }

    async fn phone(&self, user: usize) -> (Node, Client) {
        phone(self.keys[user].clone(), &self.router).await
    }

    async fn shutdown(self) {
        for node in &self.nodes {
            node.abort();
        }
        self.router.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn agent_sessions_cover_every_node() {
    let mut center = CommandCenter::new(1).await;
    let build = center
        .node(
            0,
            "build-box",
            FakeAgents {
                sessions: vec![beep_session("a3f9c1", Some(3))],
                external: Some(Vec::new()),
                ..FakeAgents::default()
            },
        )
        .await;
    let mac = center
        .node(
            0,
            "mac-box",
            FakeAgents {
                external: Some(vec![external_session("ext-1", Presence::Working)]),
                ..FakeAgents::default()
            },
        )
        .await;
    let (_node, client) = center.phone(0).await;
    let sessions = client.call(&ListAgentSessions {}).await.unwrap();
    assert_eq!(sessions.len(), 2, "{sessions:?}");
    // Newest first: the external one was active after the Beep one started.
    let external = &sessions[0];
    assert_eq!(external.machine_id, mac);
    assert_eq!(external.machine_name, "mac-box");
    assert_eq!(external.id, "ext-1");
    assert!(external.external);
    assert_eq!(external.status, AgentStatus::Working {});
    assert!(external.question.is_none());
    assert_eq!(
        external.last_activity,
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_760_000_200)
    );
    let beep = &sessions[1];
    assert_eq!(beep.machine_id, build);
    assert_eq!(beep.machine_name, "build-box");
    assert_eq!(beep.id, "a3f9c1");
    assert_eq!(beep.agent, "claude");
    assert_eq!(beep.cwd, "/srv/project");
    assert_eq!(beep.title.as_deref(), Some("Fix the build"));
    assert!(!beep.external);
    assert_eq!(beep.status, AgentStatus::Asking {});
    assert_eq!(beep.doing.as_deref(), Some("Run cargo test"));
    let question = beep.question.as_ref().expect("the session asks");
    assert_eq!(question.id, 3);
    assert_eq!(question.detail.as_deref(), Some("cargo test -p beep"));
    let allows: Vec<bool> = question.options.iter().map(|option| option.allow).collect();
    assert_eq!(allows, [true, false]);
    center.shutdown().await;
}

#[tokio::test]
async fn agent_sessions_skip_a_hung_node() {
    let mut center = CommandCenter::new(1).await;
    for name in ["build-box", "mac-box"] {
        center
            .node(
                0,
                name,
                FakeAgents {
                    sessions: vec![beep_session(name, None)],
                    external: Some(Vec::new()),
                    ..FakeAgents::default()
                },
            )
            .await;
    }
    center
        .node(
            0,
            "hung-box",
            FakeAgents {
                sessions: vec![beep_session("hung", None)],
                hang: true,
                ..FakeAgents::default()
            },
        )
        .await;
    let (_node, client) = center.phone(0).await;
    let sessions = timeout(Duration::from_secs(5), client.call(&ListAgentSessions {}))
        .await
        .expect("a hung node does not hold up the list")
        .unwrap();
    let mut names: Vec<&str> = sessions
        .iter()
        .map(|session| session.machine_name.as_str())
        .collect();
    names.sort();
    assert_eq!(names, ["build-box", "mac-box"]);
    center.shutdown().await;
}

#[tokio::test]
async fn project_sessions_skip_a_hung_node() {
    let mut center = CommandCenter::new(1).await;
    let mut machines = Vec::new();
    for (name, hang) in [("build-box", false), ("mac-box", false), ("hung-box", true)] {
        let fake = FakeAgents {
            external: Some(vec![external_session(name, Presence::Stopped)]),
            hang,
            ..FakeAgents::default()
        };
        machines.push(center.node(0, name, fake).await);
    }
    for machine in &machines {
        center.users[0]
            .save_import(NewImport {
                project: "org/proj",
                endpoint: machine,
                folder: "/srv/project",
                url: "https://example.com/org/proj",
            })
            .await
            .unwrap();
    }
    let (_node, client) = center.phone(0).await;
    let asked = ListProjectSessions {
        project: "org/proj".into(),
    };
    let sessions = timeout(Duration::from_secs(5), client.call(&asked))
        .await
        .expect("a hung node does not hold up the list")
        .unwrap();
    let mut names: Vec<&str> = sessions
        .iter()
        .map(|session| session.machine_name.as_str())
        .collect();
    names.sort();
    assert_eq!(names, ["build-box", "mac-box"]);
    center.shutdown().await;
}

#[tokio::test]
async fn agent_sessions_survive_no_external() {
    let mut center = CommandCenter::new(1).await;
    center
        .node(
            0,
            "build-box",
            FakeAgents {
                sessions: vec![beep_session("a3f9c1", None)],
                external: None,
                ..FakeAgents::default()
            },
        )
        .await;
    let (_node, client) = center.phone(0).await;
    let sessions = client.call(&ListAgentSessions {}).await.unwrap();
    assert_eq!(sessions.len(), 1, "{sessions:?}");
    assert_eq!(sessions[0].id, "a3f9c1");
    assert!(!sessions[0].external);
    center.shutdown().await;
}

#[tokio::test]
async fn agent_sessions_drop_stopped_external() {
    let mut center = CommandCenter::new(1).await;
    center
        .node(
            0,
            "mac-box",
            FakeAgents {
                external: Some(vec![
                    external_session("stopped", Presence::Stopped),
                    external_session("recent", Presence::Recent),
                    external_session("waiting", Presence::Waiting),
                ]),
                ..FakeAgents::default()
            },
        )
        .await;
    let (_node, client) = center.phone(0).await;
    let sessions = client.call(&ListAgentSessions {}).await.unwrap();
    assert_eq!(sessions.len(), 1, "{sessions:?}");
    assert_eq!(sessions[0].id, "waiting");
    assert_eq!(sessions[0].status, AgentStatus::Waiting {});
    center.shutdown().await;
}

#[tokio::test]
async fn answer_agent_question_sends_answer() {
    let mut center = CommandCenter::new(1).await;
    let (orders, mut received) = mpsc::unbounded_channel();
    let machine = center
        .node(
            0,
            "build-box",
            FakeAgents {
                sessions: vec![beep_session("a3f9c1", Some(3))],
                orders: Some(orders),
                ..FakeAgents::default()
            },
        )
        .await;
    let (_node, client) = center.phone(0).await;
    client
        .call(&AnswerAgentQuestion {
            machine_id: machine,
            agent: "claude".into(),
            session: "a3f9c1".into(),
            question: 3,
            reply: "allow".into(),
        })
        .await
        .unwrap();
    let order = timeout(Duration::from_secs(5), received.recv())
        .await
        .expect("the node got an order")
        .expect("the fake still runs");
    assert_eq!(
        order,
        Request::Answer {
            question: 3,
            reply: "allow".into()
        }
    );
    center.shutdown().await;
}

#[tokio::test]
async fn answer_agent_question_conflict_when_gone() {
    let mut center = CommandCenter::new(1).await;
    let machine = center
        .node(
            0,
            "build-box",
            FakeAgents {
                sessions: vec![beep_session("a3f9c1", None)],
                ..FakeAgents::default()
            },
        )
        .await;
    let (_node, client) = center.phone(0).await;
    let refused = client
        .call(&AnswerAgentQuestion {
            machine_id: machine,
            agent: "claude".into(),
            session: "a3f9c1".into(),
            question: 3,
            reply: "allow".into(),
        })
        .await
        .unwrap_err();
    assert!(matches!(
        refused.downcast::<ApiError>().unwrap(),
        ApiError::Conflict { .. }
    ));
    center.shutdown().await;
}

#[tokio::test]
async fn agent_sessions_list_none_of_another_users_machines() {
    let mut center = CommandCenter::new(1).await;
    center.node(0, "build-box", FakeAgents::default()).await;
    let (_node, bob) = phone(SecretKey::generate(), &center.router).await;
    assert!(bob.call(&ListAgentSessions {}).await.unwrap().is_empty());
    center.shutdown().await;
}

#[tokio::test]
async fn agent_hosts_list_machines_with_agents() {
    let mut center = CommandCenter::new(1).await;
    let build = center.node(0, "build-box", FakeAgents::default()).await;
    let (_node, client) = center.phone(0).await;
    let hosts = client.call(&ListAgentHosts {}).await.unwrap();
    assert_eq!(hosts.len(), 1, "{hosts:?}");
    assert_eq!(hosts[0].machine_id, build);
    assert_eq!(hosts[0].machine_name, "build-box");
    assert_eq!(hosts[0].agents, ["claude"]);
    center.shutdown().await;
}

#[tokio::test]
async fn a_harness_turned_off_is_not_offered() {
    let mut center = CommandCenter::new(1).await;
    let build = center.node(0, "build-box", FakeAgents::default()).await;
    let (_node, client) = center.phone(0).await;
    let off = HarnessSettings {
        enabled: false,
        program: String::new(),
        home: String::new(),
        args: Vec::new(),
        env: Vec::new(),
    };
    client
        .call(&SaveHarnessSettings {
            machine_id: build.clone(),
            agent: "claude".into(),
            settings: off.clone(),
        })
        .await
        .unwrap();
    let hosts = client.call(&ListAgentHosts {}).await.unwrap();
    assert!(hosts.is_empty(), "{hosts:?}");
    client
        .call(&SaveHarnessSettings {
            machine_id: build,
            agent: "claude".into(),
            settings: HarnessSettings {
                enabled: true,
                ..off
            },
        })
        .await
        .unwrap();
    let hosts = client.call(&ListAgentHosts {}).await.unwrap();
    assert_eq!(hosts[0].agents, ["claude"]);
    center.shutdown().await;
}

fn start(machine_id: Option<String>, prompt: &str) -> StartAgentSession {
    StartAgentSession {
        machine_id,
        agent: None,
        cwd: Some("/srv/project".into()),
        prompt: prompt.into(),
        needs: Vec::new(),
        chat: None,
        model: None,
        effort: None,
        checkout: None,
    }
}

#[tokio::test]
async fn start_agent_session_on_a_named_machine() {
    let mut center = CommandCenter::new(1).await;
    let (orders, mut received) = mpsc::unbounded_channel();
    let build = center
        .node(
            0,
            "build-box",
            FakeAgents {
                orders: Some(orders),
                ..FakeAgents::default()
            },
        )
        .await;
    let (_node, client) = center.phone(0).await;
    let started = client
        .call(&start(Some(build.clone()), "Fix the tests"))
        .await
        .unwrap();
    assert_eq!(started.why, "");
    assert_eq!(started.session.machine_id, build);
    assert_eq!(started.session.id, "b0b0b0");
    assert_eq!(started.session.cwd, "/srv/project");
    let order = timeout(Duration::from_secs(5), received.recv())
        .await
        .expect("the node got the start")
        .expect("the fake still runs");
    assert_eq!(
        order,
        Request::Start {
            cwd: Some("/srv/project".into()),
            prompt: "Fix the tests".into(),
            model: None,
            effort: None,
            checkout: None,
        }
    );
    let refused = client.call(&start(Some(build), "   ")).await.unwrap_err();
    assert!(matches!(
        refused.downcast::<ApiError>().unwrap(),
        ApiError::BadRequest { .. }
    ));
    center.shutdown().await;
}

#[tokio::test]
async fn auto_starts_on_the_least_busy_machine_and_refuses_what_none_has() {
    let mut center = CommandCenter::new(1).await;
    center
        .node(
            0,
            "busy-box",
            FakeAgents {
                sessions: vec![beep_session("a3f9c1", None)],
                ..FakeAgents::default()
            },
        )
        .await;
    let idle = center.node(0, "idle-box", FakeAgents::default()).await;
    let (_node, client) = center.phone(0).await;
    // Both fake nodes run here, so both have this folder.
    let here = std::env::temp_dir().to_string_lossy().into_owned();
    let auto = |prompt: &str, cwd: &str| StartAgentSession {
        cwd: Some(cwd.into()),
        ..start(None, prompt)
    };
    let started = client.call(&auto("Fix the tests", &here)).await.unwrap();
    assert_eq!(started.session.machine_id, idle);
    assert_eq!(started.why, "It runs the fewest sessions.");
    // The fakes report no capabilities: a prompt naming iOS still starts,
    // and says what it could not meet.
    let hinted = client
        .call(&auto("Fix the iOS widget", &here))
        .await
        .unwrap();
    assert_eq!(hinted.session.machine_id, idle);
    assert_eq!(
        hinted.why,
        "It runs the fewest sessions. No machine has Xcode, which the task mentions."
    );
    let message = |refused: anyhow::Error| match refused.downcast::<ApiError>().unwrap() {
        ApiError::BadRequest { message } => message,
        other => panic!("not a bad request: {other:?}"),
    };
    // What the app asks for is required.
    let needs_xcode = StartAgentSession {
        needs: vec![MachineCapability::Xcode {}],
        ..auto("Fix the widget", &here)
    };
    let refused = message(client.call(&needs_xcode).await.unwrap_err());
    assert!(refused.contains("has Xcode"), "{refused}");
    // A folder no machine has is said, not started in the environment.
    let missing = "/srv/no-such-project-for-auto";
    let refused = message(
        client
            .call(&auto("Fix the tests", missing))
            .await
            .unwrap_err(),
    );
    assert_eq!(
        refused,
        format!("There is no folder {missing} on busy-box and idle-box.")
    );
    center.shutdown().await;
}

#[tokio::test]
async fn auto_preview_names_the_machine_without_starting() {
    let mut center = CommandCenter::new(1).await;
    let (orders, mut received) = mpsc::unbounded_channel();
    center
        .node(
            0,
            "busy-box",
            FakeAgents {
                sessions: vec![beep_session("a3f9c1", None)],
                ..FakeAgents::default()
            },
        )
        .await;
    let idle = center
        .node(
            0,
            "idle-box",
            FakeAgents {
                orders: Some(orders),
                ..FakeAgents::default()
            },
        )
        .await;
    let (_node, client) = center.phone(0).await;
    let here = std::env::temp_dir().to_string_lossy().into_owned();
    let preview = |needs: Vec<MachineCapability>| PreviewAgentMachine {
        agent: None,
        cwd: Some(here.clone()),
        prompt: "Fix the iOS widget".into(),
        needs,
    };
    let pick = client.call(&preview(Vec::new())).await.unwrap();
    assert_eq!(pick.machine_id, idle);
    assert_eq!(pick.machine_name, "idle-box");
    assert_eq!(
        pick.why,
        "It runs the fewest sessions. No machine has Xcode, which the task mentions."
    );
    let refused = client
        .call(&preview(vec![MachineCapability::Xcode {}]))
        .await
        .unwrap_err();
    assert!(matches!(
        refused.downcast::<ApiError>().unwrap(),
        ApiError::BadRequest { .. }
    ));
    assert!(received.try_recv().is_err(), "nothing was started");
    center.shutdown().await;
}

#[tokio::test]
async fn get_agent_session_reads_plan_files_and_transcript() {
    let mut center = CommandCenter::new(1).await;
    let mut session = beep_session("a3f9c1", None);
    session.state.plan = vec![
        PlanEntry {
            content: "Find the bug".into(),
            status: PlanStatus::Completed,
        },
        PlanEntry {
            content: "Fix it".into(),
            status: PlanStatus::InProgress,
        },
    ];
    session.state.files_touched = vec!["src/main.rs".into()];
    session.turns = 2;
    let build = center
        .node(
            0,
            "build-box",
            FakeAgents {
                sessions: vec![session],
                ..FakeAgents::default()
            },
        )
        .await;
    let (_node, client) = center.phone(0).await;
    let get = |session: &str| GetAgentSession {
        machine_id: build.clone(),
        agent: "claude".into(),
        session: session.into(),
        last: 30,
    };
    let view = client.call(&get("a3f9c1")).await.unwrap();
    assert_eq!(view.session.status, AgentStatus::Working {});
    let plan: Vec<AgentPlanStatus> = view.plan.iter().map(|step| step.status).collect();
    assert_eq!(
        plan,
        [
            AgentPlanStatus::Completed {},
            AgentPlanStatus::InProgress {}
        ]
    );
    assert_eq!(view.files, ["src/main.rs"]);
    assert_eq!(view.turns, 2);
    assert_eq!(view.entries.len(), 1);
    assert_eq!(view.entries[0].kind, AgentEntryKind::Agent {});
    assert_eq!(view.entries[0].text, "a3f9c1: the last 30");
    assert_eq!(view.earlier, 4);
    let missing = client.call(&get("000000")).await.unwrap_err();
    assert!(matches!(
        missing.downcast::<ApiError>().unwrap(),
        ApiError::NotFound {}
    ));
    center.shutdown().await;
}

#[tokio::test]
async fn steer_agent_session_sends_prompts_and_stops() {
    let mut center = CommandCenter::new(1).await;
    let (orders, mut received) = mpsc::unbounded_channel();
    let build = center
        .node(
            0,
            "build-box",
            FakeAgents {
                sessions: vec![beep_session("a3f9c1", None)],
                orders: Some(orders),
                ..FakeAgents::default()
            },
        )
        .await;
    let (_node, client) = center.phone(0).await;
    let steer = |session: &str, steer: AgentSteer| SteerAgentSession {
        machine_id: build.clone(),
        agent: "claude".into(),
        session: session.into(),
        steer,
    };
    client
        .call(&steer(
            "a3f9c1",
            AgentSteer::Prompt {
                text: "Also run clippy".into(),
            },
        ))
        .await
        .unwrap();
    client
        .call(&steer("a3f9c1", AgentSteer::Cancel {}))
        .await
        .unwrap();
    let mut got = Vec::new();
    for _ in 0..2 {
        got.push(
            timeout(Duration::from_secs(5), received.recv())
                .await
                .expect("the node got an order")
                .expect("the fake still runs"),
        );
    }
    assert_eq!(
        got,
        [
            Request::Prompt {
                text: "Also run clippy".into()
            },
            Request::Cancel {}
        ]
    );
    let missing = client
        .call(&steer("000000", AgentSteer::Close {}))
        .await
        .unwrap_err();
    assert!(matches!(
        missing.downcast::<ApiError>().unwrap(),
        ApiError::NotFound {}
    ));
    center.shutdown().await;
}

#[tokio::test]
async fn agent_images_pass_through_from_the_node() {
    let mut center = CommandCenter::new(1).await;
    let png = b"\x89PNG\r\n\x1a\nshot".to_vec();
    let (orders, mut received) = mpsc::unbounded_channel();
    let build = center
        .node(
            0,
            "build-box",
            FakeAgents {
                sessions: vec![beep_session("a3f9c1", None)],
                orders: Some(orders),
                images: vec![("a3f9c1".into(), png.clone())],
                ..FakeAgents::default()
            },
        )
        .await;
    let (_node, client) = center.phone(0).await;
    let get = |session: &str, key: &str| GetAgentImage {
        machine_id: build.clone(),
        session: session.into(),
        key: key.into(),
    };
    let key = image_key(&png);
    let got = client.call(&get("a3f9c1", &key)).await.unwrap();
    assert_eq!(
        got,
        AgentImageData {
            key: key.clone(),
            mime: "image/png".into(),
            data: png.clone(),
        }
    );
    let not_found = |error: anyhow::Error| {
        matches!(error.downcast::<ApiError>().unwrap(), ApiError::NotFound {})
    };
    // Once read, the server keeps a copy, which any session shows by key.
    let elsewhere = client.call(&get("000000", &key)).await.unwrap();
    assert_eq!(elsewhere.data, png);
    let unknown = client.call(&get("a3f9c1", &image_key(b"other"))).await;
    assert!(not_found(unknown.unwrap_err()));
    let refused = client.call(&get("a3f9c1", "shot.png")).await.unwrap_err();
    assert!(matches!(
        refused.downcast::<ApiError>().unwrap(),
        ApiError::BadRequest { .. }
    ));
    // An image sent with a prompt goes to the node; the old listing stays
    // empty for the apps that still ask.
    let steer = SteerAgentSession {
        machine_id: build.clone(),
        agent: "claude".into(),
        session: "a3f9c1".into(),
        steer: AgentSteer::PromptImages {
            text: "Look at this".into(),
            images: vec![Upload {
                name: "shot.png".into(),
                mime: "image/png".into(),
                data: png,
            }],
        },
    };
    client.call(&steer).await.unwrap();
    let order = timeout(Duration::from_secs(5), received.recv())
        .await
        .expect("the node got the prompt")
        .expect("the fake still runs");
    assert!(matches!(order, Request::PromptImages { .. }), "{order:?}");
    let listed = client
        .call(&ListAgentImages {
            machine_id: build,
            session: "a3f9c1".into(),
        })
        .await
        .unwrap();
    assert!(listed.is_empty());
    center.shutdown().await;
}

#[tokio::test]
async fn pictures_a_node_sends_show_while_it_is_offline() {
    let mut center = CommandCenter::new(2).await;
    let png = b"\x89PNG\r\n\x1a\nchart".to_vec();
    let build = center
        .node(
            0,
            "build-box",
            FakeAgents {
                sessions: vec![beep_session("a3f9c1", None)],
                sent: vec![("a3f9c1".into(), png.clone())],
                ..FakeAgents::default()
            },
        )
        .await;
    let (_node, client) = center.phone(0).await;
    let get = GetAgentImage {
        machine_id: build.clone(),
        session: "a3f9c1".into(),
        key: image_key(&png),
    };
    // The node cannot answer `Image`, so the picture comes from the copy
    // kept of what it sent. Polls: keeping it has no signal to wait on.
    let got = timeout(Duration::from_secs(20), async {
        loop {
            if let Ok(got) = client.call(&get).await {
                return got;
            }
            sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("the picture was kept");
    assert_eq!(got.data, png);
    assert_eq!(got.mime, "image/png");
    // Another user does not get it.
    let (_other, theirs) = center.phone(1).await;
    assert!(theirs.call(&get).await.is_err());

    for node in &center.nodes {
        node.abort();
    }
    let online = center.app.nodes.online(&build).expect("still registered");
    online.connection.close(0u32.into(), b"gone");
    timeout(Duration::from_secs(20), async {
        while center.app.nodes.online(&build).is_some() {
            sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("the node went offline");
    assert_eq!(client.call(&get).await.unwrap().data, png);
    center.shutdown().await;
}

#[tokio::test]
async fn agent_control_keeps_to_the_callers_machines_and_chats() {
    let mut center = CommandCenter::new(2).await;
    let mine = center.node(0, "build-box", FakeAgents::default()).await;
    let theirs = center
        .node(
            1,
            "gpu-box",
            FakeAgents {
                sessions: vec![beep_session("a3f9c1", None)],
                ..FakeAgents::default()
            },
        )
        .await;
    let (_node, client) = center.phone(0).await;
    let not_found = |error: anyhow::Error| {
        matches!(error.downcast::<ApiError>().unwrap(), ApiError::NotFound {})
    };
    let hosts = client.call(&ListAgentHosts {}).await.unwrap();
    let listed: Vec<&str> = hosts.iter().map(|host| host.machine_id.as_str()).collect();
    assert_eq!(listed, [mine.as_str()]);
    let refused = client
        .call(&start(Some(theirs.clone()), "Fix the tests"))
        .await
        .unwrap_err();
    assert!(not_found(refused));
    let get = GetAgentSession {
        machine_id: theirs.clone(),
        agent: "claude".into(),
        session: "a3f9c1".into(),
        last: 30,
    };
    assert!(not_found(client.call(&get).await.unwrap_err()));
    let steer = SteerAgentSession {
        machine_id: theirs,
        agent: "claude".into(),
        session: "a3f9c1".into(),
        steer: AgentSteer::Cancel {},
    };
    assert!(not_found(client.call(&steer).await.unwrap_err()));
    // A chat of another user's is not one to start in.
    let chat = uuid::Uuid::new_v4();
    center.users[1]
        .start_research(chat, "Shaders")
        .await
        .unwrap();
    let in_their_chat = StartAgentSession {
        chat: Some(chat),
        ..start(Some(mine), "Fix the tests")
    };
    assert!(not_found(client.call(&in_their_chat).await.unwrap_err()));
    center.shutdown().await;
}

#[tokio::test]
async fn agent_control_never_reaches_another_users_machines() {
    let mut center = CommandCenter::new(1).await;
    center.node(0, "build-box", FakeAgents::default()).await;
    let (_node, bob) = phone(SecretKey::generate(), &center.router).await;
    assert!(bob.call(&ListAgentHosts {}).await.unwrap().is_empty());
    let refused = bob.call(&start(None, "Fix the tests")).await.unwrap_err();
    assert!(matches!(
        refused.downcast::<ApiError>().unwrap(),
        ApiError::BadRequest { .. }
    ));
    center.shutdown().await;
}

/// A server taking both phones and nodes on one endpoint, as the real one
/// does, with alice as a user.
struct OneEndpoint {
    db: TestDb,
    app: Arc<App>,
    router: Router,
    alice: SecretKey,
}

async fn one_endpoint() -> OneEndpoint {
    let db = TestDb::new().await.unwrap();
    let store = db.store().await.unwrap();
    let alice = SecretKey::generate();
    let model = "openai:test".parse().unwrap();
    let models = ModelChoices::resolve(&NoModel, model, Vec::new()).await;
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
    let nodes_handler = nodes::NodeHandler::new(app.clone(), server_endpoint.id());
    let router = Router::builder(server_endpoint)
        .accept(
            transport::ALPN,
            transport::BeepHandler::new(app.clone(), pusher),
        )
        .accept(nodes::NODE_ALPN, nodes_handler)
        .spawn();
    OneEndpoint {
        db,
        app,
        router,
        alice,
    }
}

fn setup_of(token: &str) -> NodeSetup {
    NodeSetup {
        token: token.to_owned(),
        name: None,
    }
}

#[tokio::test]
async fn a_machine_sets_itself_up_with_a_token_once() {
    let server = one_endpoint().await;
    let addr = server.router.endpoint().addr();
    let (_phone, client) = phone(server.alice.clone(), &server.router).await;
    let made = client
        .call(&CreateNodeToken {
            name: Some("build box".into()),
        })
        .await
        .unwrap();
    let token = SetupToken::parse(&made.token).unwrap();
    assert_eq!(token.server, server.router.endpoint().id());
    let left = made
        .expires_at
        .duration_since(SystemTime::now())
        .expect("it expires later");
    assert!(left <= Duration::from_secs(600) && left > Duration::from_secs(590));

    // Only the hash is kept.
    let pool = server.db.raw_pool().await.unwrap();
    let stored: Vec<String> = sqlx::query_scalar("SELECT hash FROM node_tokens")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert_eq!(stored, [token.hash()]);
    let text = made.token.to_lowercase().replace('-', "");
    assert!(!stored[0].contains(&text[3..30]));

    let status = GetNodeToken {
        token: made.token.clone(),
    };
    assert_eq!(client.call(&status).await.unwrap(), None);
    let key = SecretKey::generate();
    let machine = endpoint(key.clone()).await;
    let claimed = beep_node::link::claim(&machine, addr.clone(), "host", &setup_of(&made.token))
        .await
        .unwrap();
    assert_eq!(
        client.call(&status).await.unwrap(),
        Some(key.public().to_string())
    );
    assert_eq!(claimed.name, "build box");
    let alice_id = server
        .app
        .sign_in(&server.alice.public())
        .await
        .unwrap()
        .expect("alice signs in")
        .user
        .id()
        .to_owned();
    assert_eq!(
        claimed.account,
        beep_server::protocol::names::username(&server.alice.public())
    );
    let user = server
        .app
        .sign_in_linked(&key.public())
        .await
        .unwrap()
        .expect("the machine's key signs in")
        .user;
    assert_eq!(user.id(), alice_id);
    let keys = user.account_keys().await.unwrap();
    let linked = keys
        .iter()
        .find(|k| k.endpoint_id == key.public().to_string())
        .expect("a key row");
    assert_eq!(linked.name, "build box");
    assert_eq!(linked.kind, DeviceKind::Desktop {});
    let rows = user.nodes().await.unwrap();
    assert_eq!(
        rows[0].name, "build box",
        "the node row is written on connect"
    );

    // Used: another machine is refused and links nothing.
    let other = SecretKey::generate();
    let refused = beep_node::link::claim(
        &endpoint(other.clone()).await,
        addr.clone(),
        "host",
        &setup_of(&made.token),
    )
    .await
    .unwrap_err();
    assert!(
        format!("{refused:#}").contains("already used"),
        "{refused:#}"
    );
    assert!(
        server
            .app
            .sign_in_linked(&other.public())
            .await
            .unwrap()
            .is_none()
    );

    // The linked machine now connects like any node.
    let served = tokio::spawn(async move {
        beep_node::link::connect(&machine, addr, "build box", &Offers::default()).await
    });
    let live = online(&server.app, &key.public().to_string()).await;
    assert_eq!(live.name, "build box");
    served.abort();
    server.router.shutdown().await.unwrap();
}

#[tokio::test]
async fn every_user_links_a_machine_that_only_they_see() {
    let server = one_endpoint().await;
    let addr = server.router.endpoint().addr();
    let (_alice_phone, alice) = phone(server.alice.clone(), &server.router).await;
    let bob_key = SecretKey::generate();
    let (_bob_phone, bob) = phone(bob_key.clone(), &server.router).await;
    let made = bob
        .call(&CreateNodeToken {
            name: Some("bobs box".into()),
        })
        .await
        .unwrap();
    let key = SecretKey::generate();
    let machine = endpoint(key.clone()).await;
    beep_node::link::claim(&machine, addr.clone(), "host", &setup_of(&made.token))
        .await
        .unwrap();
    let served = tokio::spawn(async move {
        beep_node::link::connect(&machine, addr, "bobs box", &Offers::default()).await
    });
    let live = online(&server.app, &key.public().to_string()).await;
    let bob_id = server
        .app
        .sign_in(&bob_key.public())
        .await
        .unwrap()
        .expect("bob signs in")
        .user
        .id()
        .to_owned();
    assert_eq!(live.user, bob_id);

    let his = bob.call(&ListMachines {}).await.unwrap();
    assert_eq!(his.len(), 1);
    assert_eq!(his[0].name, "bobs box");
    assert!(alice.call(&ListMachines {}).await.unwrap().is_empty());
    assert!(alice.call(&ListMachineDetails {}).await.unwrap().is_empty());
    assert!(alice.call(&ListAgentSessions {}).await.unwrap().is_empty());
    served.abort();
    server.router.shutdown().await.unwrap();
}

#[tokio::test]
async fn an_expired_unknown_or_foreign_token_is_refused() {
    let server = one_endpoint().await;
    let addr = server.router.endpoint().addr();
    let (_phone, client) = phone(server.alice.clone(), &server.router).await;
    let made = client.call(&CreateNodeToken { name: None }).await.unwrap();
    let pool = server.db.raw_pool().await.unwrap();
    sqlx::query("UPDATE node_tokens SET expires_at = now() - interval '1 second'")
        .execute(&pool)
        .await
        .unwrap();
    let key = SecretKey::generate();
    let machine = endpoint(key.clone()).await;
    let expired = beep_node::link::claim(&machine, addr.clone(), "host", &setup_of(&made.token))
        .await
        .unwrap_err();
    assert!(format!("{expired:#}").contains("expired"), "{expired:#}");

    let unknown = SetupToken::generate(server.router.endpoint().id()).to_string();
    let refused = beep_node::link::claim(&machine, addr.clone(), "host", &setup_of(&unknown))
        .await
        .unwrap_err();
    assert!(format!("{refused:#}").contains("unknown"), "{refused:#}");

    let foreign = SetupToken::generate(SecretKey::generate().public()).to_string();
    let refused = beep_node::link::claim(&machine, addr, "host", &setup_of(&foreign))
        .await
        .unwrap_err();
    assert!(
        format!("{refused:#}").contains("another server"),
        "{refused:#}"
    );
    assert!(
        server
            .app
            .sign_in_linked(&key.public())
            .await
            .unwrap()
            .is_none()
    );
    server.router.shutdown().await.unwrap();
}

#[tokio::test]
async fn an_unlinked_key_gets_only_a_small_hello_read() {
    let Server {
        _db,
        router,
        addr,
        app,
        ..
    } = server().await;
    let key = SecretKey::generate();
    let stranger = endpoint(key.clone()).await;
    // Offers padding the hello far past what a real one carries.
    let offers = Offers {
        agents: Arc::default(),
        extra: (0..200)
            .map(|_| Arc::new(BigName) as Arc<dyn Protocol>)
            .collect(),
        provision: None,
    };
    let refused = beep_node::link::connect(&stranger, addr, "stranger", &offers)
        .await
        .unwrap_err();
    // Closed for the hello it would not read, before any "not linked".
    assert!(format!("{refused:#}").contains("no hello"), "{refused:#}");
    assert!(app.nodes.online(&key.public().to_string()).is_none());
    router.shutdown().await.unwrap();
}

/// A protocol with a long name, to make a hello large.
struct BigName;

impl Protocol for BigName {
    fn name(&self) -> &str {
        static NAME: std::sync::OnceLock<String> = std::sync::OnceLock::new();
        NAME.get_or_init(|| "x".repeat(200))
    }

    fn serve(&self, _tx: LinkTx, _rx: LinkRx) -> Session {
        Box::pin(async { Ok(()) })
    }
}

#[tokio::test]
async fn only_the_maker_revokes_a_token() {
    let server = one_endpoint().await;
    let addr = server.router.endpoint().addr();
    let (_alice_phone, alice) = phone(server.alice.clone(), &server.router).await;
    let (_bob_phone, bob) = phone(SecretKey::generate(), &server.router).await;
    // Bob gets a token of his own too.
    bob.call(&CreateNodeToken { name: None }).await.unwrap();

    let made = alice.call(&CreateNodeToken { name: None }).await.unwrap();
    let revoke = RevokeNodeToken {
        token: made.token.clone(),
    };
    let refused = bob.call(&revoke).await.unwrap_err();
    assert!(matches!(
        refused.downcast::<ApiError>().unwrap(),
        ApiError::NotFound {}
    ));
    let junk = bob
        .call(&RevokeNodeToken {
            token: "nope".into(),
        })
        .await
        .unwrap_err();
    assert!(matches!(
        junk.downcast::<ApiError>().unwrap(),
        ApiError::BadRequest { .. }
    ));

    alice.call(&revoke).await.unwrap();
    let machine = endpoint(SecretKey::generate()).await;
    let refused = beep_node::link::claim(&machine, addr, "host", &setup_of(&made.token))
        .await
        .unwrap_err();
    assert!(format!("{refused:#}").contains("unknown"), "{refused:#}");
    server.router.shutdown().await.unwrap();
}

#[tokio::test]
async fn agent_models_come_from_the_node_and_reach_the_start() {
    let mut center = CommandCenter::new(1).await;
    let (orders, mut received) = mpsc::unbounded_channel();
    let build = center
        .node(
            0,
            "build-box",
            FakeAgents {
                orders: Some(orders),
                models: Some(ModelList {
                    pick: ModelPick::Listed,
                    current: Some("fast".into()),
                    models: vec![ModelOption {
                        id: "deep".into(),
                        name: "Deep".into(),
                        description: Some("Slower, more careful".into()),
                    }],
                }),
                ..FakeAgents::default()
            },
        )
        .await;
    let older = center.node(0, "old-box", FakeAgents::default()).await;
    let (_node, client) = center.phone(0).await;
    let list = |machine_id: &str, agent: &str| ListAgentModels {
        machine_id: machine_id.into(),
        agent: agent.into(),
    };
    let listed = client.call(&list(&build, "claude")).await.unwrap();
    assert_eq!(
        listed,
        AgentModels {
            pick: AgentModelPick::Listed {},
            current: Some("fast".into()),
            models: vec![AgentModel {
                id: "deep".into(),
                name: "Deep".into(),
                description: Some("Slower, more careful".into()),
            }],
        }
    );
    // A node from before models closes the stream: the form hides the field.
    let fixed = client.call(&list(&older, "claude")).await.unwrap();
    assert_eq!(fixed.pick, AgentModelPick::Fixed {});
    assert!(fixed.models.is_empty());
    let bad_request = |error: anyhow::Error| {
        matches!(
            error.downcast::<ApiError>().unwrap(),
            ApiError::BadRequest { .. }
        )
    };
    let unknown = client.call(&list(&build, "codex")).await.unwrap_err();
    assert!(bad_request(unknown), "the machine offers no codex");
    let elsewhere = client.call(&list("nope", "claude")).await.unwrap_err();
    assert!(matches!(
        elsewhere.downcast::<ApiError>().unwrap(),
        ApiError::NotFound {}
    ));

    let on_model = |model: &str| StartAgentSession {
        model: Some(model.into()),
        ..start(Some(build.clone()), "Fix the tests")
    };
    client.call(&on_model(" deep ")).await.unwrap();
    let order = timeout(Duration::from_secs(5), received.recv())
        .await
        .expect("the node got the start")
        .expect("the fake still runs");
    assert_eq!(
        order,
        Request::Start {
            cwd: Some("/srv/project".into()),
            prompt: "Fix the tests".into(),
            model: Some("deep".into()),
            effort: None,
            checkout: None,
        }
    );
    let refused = client.call(&on_model("deep\nrm")).await.unwrap_err();
    assert!(bad_request(refused));

    // Thinking goes to the node for a level the agent takes, and is
    // refused for one it does not.
    let thinking = |effort: AgentEffort| StartAgentSession {
        agent: Some("claude".into()),
        effort: Some(effort),
        ..start(Some(build.clone()), "Fix the tests")
    };
    client.call(&thinking(AgentEffort::Xhigh {})).await.unwrap();
    let order = timeout(Duration::from_secs(5), received.recv())
        .await
        .expect("the node got the start")
        .expect("the fake still runs");
    assert_eq!(
        order,
        Request::Start {
            cwd: Some("/srv/project".into()),
            prompt: "Fix the tests".into(),
            model: None,
            effort: Some(Effort::Xhigh),
            checkout: None,
        }
    );
    let refused = client
        .call(&thinking(AgentEffort::Minimal {}))
        .await
        .unwrap_err();
    assert!(bad_request(refused), "Claude Code has no minimal level");

    // A checkout plan goes to a node that makes them; a base that is not
    // a ref name is refused first.
    let in_worktree = |base: &str| StartAgentSession {
        checkout: Some(AgentCheckoutPlan::NewWorktree {
            base: Some(base.into()),
        }),
        ..start(Some(build.clone()), "Fix the tests")
    };
    client.call(&in_worktree("main")).await.unwrap();
    let order = timeout(Duration::from_secs(5), received.recv())
        .await
        .expect("the node got the start")
        .expect("the fake still runs");
    assert_eq!(
        order,
        Request::Start {
            cwd: Some("/srv/project".into()),
            prompt: "Fix the tests".into(),
            model: None,
            effort: None,
            checkout: Some(CheckoutPlan::NewWorktree {
                base: Some("main".into()),
            }),
        }
    );
    let refused = client.call(&in_worktree("a b")).await.unwrap_err();
    assert!(bad_request(refused));
    center.shutdown().await;
}
