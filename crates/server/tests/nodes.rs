//! A real `beep-node` against the server over iroh: only linked keys get
//! in, the environment report is stored, the shell keeps its state, a
//! coding agent (a scripted stand-in speaking ACP) runs in sessions that
//! outlive the connection, a protocol the host adds, such as `desktop/1`,
//! is offered and served, and the app sees the machine.

use async_trait::async_trait;
use beep_node::agents::{AgentKind, Agents};
use beep_node::link::{Offers, Protocol, Session};
use beep_node::machine::service::logs::Logs;
use beep_server::protocol::node::{
    AppInfo, AppName, GpuInfo, GpuVendor, LinkRx, LinkTx,
    desktop::{self, Reply, Screenshot},
};
use beep_server::protocol::{
    FlshAutoCompact, FlshModelRule, FlshSettings, FlshSort, GetFlshSettings, HarnessEnv,
    ListHarnessSettings, SaveFlshSettings, SaveHarnessSettingsAll, SecretText,
};
use beep_server::{
    account::user::User,
    agent::models::ModelChoices,
    api::transport,
    app::{App, Draws, Message, Reach, ServerTools},
    client::{Client, Node},
    nodes::{self, Follower, Online, StartOn},
    protocol::{
        AddMachineOverSsh, AgentCheckoutPlan, AgentContext, AgentEffort, AgentFileChunk,
        AgentImageData, AgentMode, AgentModel, AgentModelPick, AgentModels, AgentNoticeKey,
        AgentSessionModes, AgentSessionUsage, AgentSteer, AgentTokens, AnswerApproval, ApiError,
        Approval, ApprovalDecision, CreateNodeToken, CustomAgent, CustomAgentHarness,
        CustomAgentScope, DeviceKind, DiffFile, DiffStatus, GetAgentFile, GetAgentImage,
        GetAgentSession, GetAgentSessionModes, GetAgentSessionUsage, GetAgentThumb, GetMachineLog,
        GetNodeToken, GetSession, GetSpendReport, GetUsage, GetUsageReport, GetWorkDiff, GitChange,
        HarnessSettings, HistoryEntry, ListAgentHosts, ListAgentImages, ListAgentModels,
        ListAgentSessions, ListCustomAgents, ListMachineDetails, ListMachineStats, ListMachines,
        ListMemoryScopes, ListProjects, ListSshHosts, ListWorkFolder, ListWorkTree,
        MAX_WORK_UPLOAD, MachineCapability, MemoryScope, MemorySource, PreviewAgentMachine,
        ReadWorkFile, RemoveDevice, RemoveMachine, RevokeNodeToken, RunEvent, SaveHarnessSettings,
        StartAgentSession, SteerAgentSession, Upload, WorkDiff, WorkEntry, WorkFolder, WorkTree,
        WriteWorkFile, image_key,
        link::LinkOffer,
        node::{
            EnvReport, NodeSetup,
            agent::{
                Alert, AlertKind, AnswerOption, CheckoutPlan, Effort, Entry, EntryKind, Event,
                ExternalSession, ModeOption, ModelList, ModelOption, ModelPick, OptionKind,
                Question, Request, SESSIONS_PROTOCOL, SessionInfo, SessionsReply, SessionsRequest,
                State, Status, StopReason,
            },
            usage::{
                Billing, HourSpend, LimitWindow, ModelTokens, PlanUsage, SessionSpend, TurnUsage,
            },
        },
        setup::SetupToken,
    },
    push::Pusher,
    store::{CreateRun, Fact, NewKey, NewNode, Scope, TestDb},
};
use flashmind::types::{
    AgentLlmConfig, CompletionRequest, CompletionStream, FinishReason, LlmProvider, Provider, Role,
    StreamEvent,
};
use iroh::{
    Endpoint, EndpointAddr, SecretKey,
    endpoint::{VarInt, presets},
    protocol::Router,
};
use serde_json::{Value, json};
use sqlx::PgPool;
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::{
    sync::{Notify, Semaphore, mpsc},
    time::{sleep, timeout},
};
use uuid::Uuid;

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
    Endpoint::builder(presets::Minimal)
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
    let router = nodes::accept_nodes(Router::builder(server_endpoint), handler).spawn();
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

/// A node from before `beep-node/2` dials `beep-node/1`, says hello in maps
/// and is answered in maps; the server serves it as any other.
#[tokio::test]
async fn a_node_from_before_beep_node_2_still_links() {
    use beep_server::protocol::node::{Down, NODE_ALPN_NAMED, NodeHello, link_on};
    let server = server().await;
    let key = linked_key(&server, "old box").await;
    let node = endpoint(key.clone()).await;
    let connection = node
        .connect(server.addr.clone(), NODE_ALPN_NAMED)
        .await
        .expect("the server takes beep-node/1");
    let (send, recv) = connection.open_bi().await.unwrap();
    let (mut tx, mut rx) = link_on(&connection, send, recv);
    let hello = NodeHello {
        version: "0.1.0".to_owned(),
        name: "old box".to_owned(),
        offers: Vec::new(),
        workspace: None,
        setup: None,
        credentials: false,
        checkouts: false,
        agent_settings: false,
        branch_checkouts: false,
    };
    tx.send(&hello).await.unwrap();
    let welcome = timeout(Duration::from_secs(20), rx.recv::<Down>())
        .await
        .expect("welcomed in time")
        .unwrap();
    assert!(matches!(welcome, Some(Down::Welcome { .. })), "{welcome:?}");
    let live = online(&server.app, &key.public().to_string()).await;
    assert_eq!(live.name, "old box");
}

/// A node that offers both settles on `beep-node/2`: the Router keeps the
/// order it was built in, and a swap would leave every node on maps.
#[tokio::test]
async fn a_node_offering_both_protocols_settles_on_the_newer() {
    use beep_server::protocol::node::{NODE_ALPN, NODE_ALPN_NAMED};
    let server = server().await;
    let node = endpoint(SecretKey::generate()).await;
    let connection = node
        .connect_with_opts(
            server.addr.clone(),
            NODE_ALPN,
            iroh::endpoint::ConnectOptions::new()
                .with_additional_alpns(vec![NODE_ALPN_NAMED.to_vec()]),
        )
        .await
        .expect("connects")
        .await
        .expect("the handshake finishes");
    assert_eq!(connection.alpn(), NODE_ALPN);
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
            let offers = Offers::with_protocols(vec![Arc::new(FakeDesktop)]);
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

    // The report follows the hello on its own, beside the sessions above;
    // wait until it is stored.
    timeout(Duration::from_secs(20), async {
        while user.nodes().await.unwrap()[0].env.is_none() {
            sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("the node reported");
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
        name: "pi".into(),
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
            let mut offers = Offers::default();
            offers.agents = agents;
            beep_node::link::connect(&node, addr, "build box", &offers).await
        })
    };
    let served = connect();
    let live = online(app, &node_id).await;
    assert!(live.offers("pi/1"), "{:?}", live.offers);
    assert!(live.offers("agents/1"), "{:?}", live.offers);

    let cwd = std::env::temp_dir().join(format!("beep-agent-test-{}", std::process::id()));
    std::fs::create_dir_all(&cwd).unwrap();
    let wait = Duration::from_secs(20);

    // A start that puts a choice to the user comes back with the question.
    let link = live
        .start_agent(
            "pi",
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
    let link = live.agent_session("pi", &id).await.unwrap();
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
    assert_eq!(sessions[0].agent, "pi");
    let (entries, earlier) = live.transcript(&id, 100, false).await.unwrap();
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
    let (last, earlier) = live.transcript(&id, 2, false).await.unwrap();
    assert_eq!(last.len(), 2);
    assert_eq!(earlier as usize, entries.len() - 2);

    link.send(Request::Close {}).await.unwrap();
    let closed = link.wait(edited.mark(), wait).await;
    assert_eq!(closed.ended.as_deref(), Some("closed"));

    let refused = live
        .start_agent(
            "pi",
            Some("/no/such/folder".into()),
            "hi".into(),
            StartOn::default(),
        )
        .await
        .err()
        .expect("a missing folder is refused");
    assert!(format!("{refused:#}").contains("no folder"), "{refused:#}");
    let refused = live
        .agent_session("pi", "000000")
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
            let mut offers = Offers::default();
            offers.agents = agents;
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
            let mut offers = Offers::default();
            offers.agents = agents;
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
        router.endpoint().addr(),
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

/// A project of `user` with a folder on each of `machines`, the first its
/// home and its pin, in every table that keeps folders.
async fn seed_project(pool: &PgPool, user: &str, name: &str, machines: &[&str]) -> Uuid {
    let project = Uuid::new_v4();
    sqlx::query("INSERT INTO projects (id, user_id, name, pin) VALUES ($1, $2, $3, $4)")
        .bind(project)
        .bind(user)
        .bind(name)
        .bind(machines[0])
        .execute(pool)
        .await
        .unwrap();
    for (n, machine) in machines.iter().enumerate() {
        sqlx::query(
            "INSERT INTO project_checkouts (project_id, machine, path, home)
             VALUES ($1, $2, '/srv/project', $3)",
        )
        .bind(project)
        .bind(machine)
        .bind(n == 0)
        .execute(pool)
        .await
        .unwrap();
        for table in ["project_imports", "project_sync"] {
            let url = if table == "project_imports" {
                ", url"
            } else {
                ""
            };
            let value = if table == "project_imports" {
                ", 'https://example.com/r'"
            } else {
                ""
            };
            sqlx::query(&format!(
                "INSERT INTO {table} (user_id, project, endpoint_id, folder{url})
                 VALUES ($1, $2, $3, '/srv/project'{value})"
            ))
            .bind(user)
            .bind(name)
            .bind(machine)
            .execute(pool)
            .await
            .unwrap();
        }
    }
    project
}

/// A running worker of `project` on `machine`, and its task.
async fn seed_worker(pool: &PgPool, user: &str, project: Uuid, machine: &str) -> Uuid {
    let task = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO project_tasks (id, project_id, slug, title, state)
         VALUES ($1, $2, 'fix', 'Fix it', 'running')",
    )
    .bind(task)
    .bind(project)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO project_workers
             (id, task_id, project_id, user_id, machine, agent, session, folder, state)
         VALUES ($1, $2, $3, $4, $5, 'claude', 's1', '/srv/' || $3, 'running')",
    )
    .bind(Uuid::new_v4())
    .bind(task)
    .bind(project)
    .bind(user)
    .bind(machine)
    .execute(pool)
    .await
    .unwrap();
    task
}

/// One row in every other table the server keeps per machine.
async fn seed_session_rows(pool: &PgPool, user: &str, machine: &str) {
    let inserts = [
        "INSERT INTO agent_usage (user_id, endpoint_id, session, day, agent, cwd)
         VALUES ($1, $2, 's1', now()::date, 'claude', '/srv/project')",
        "INSERT INTO agent_session_usage (user_id, endpoint_id, agent, session, hour, model, cwd)
         VALUES ($1, $2, 'claude', 's1', now(), 'm', '/srv/project')",
        "INSERT INTO agent_session_starts (user_id, machine, agent, session, device)
         VALUES ($1, $2, 'claude', 's1', 'a-phone')",
        "INSERT INTO harness_settings (user_id, machine_id, agent) VALUES ($1, $2, 'claude')",
        "INSERT INTO checkout_checks (user_id, endpoint_id, checked_at) VALUES ($1, $2, now())",
        "INSERT INTO terminals (id, user_id, project, endpoint_id, task_id, name, command, cwd)
         VALUES (gen_random_uuid(), $1, 'acme/both', $2, 4, 'dev', 'run', '/srv/project')",
        "INSERT INTO branch_reviews (id, user_id, project, branch, base, endpoint_id, folder,
             head, merge_base, head_at, changed_lines, files)
         VALUES (gen_random_uuid(), $1, 'acme/both', 'fix', 'main', $2, '/srv/project',
             'abc', 'def', now(), 1, 1)",
    ];
    for insert in inserts {
        sqlx::query(insert)
            .bind(user)
            .bind(machine)
            .execute(pool)
            .await
            .unwrap();
    }
    let file = format!("image-{machine}");
    sqlx::query(
        "INSERT INTO files (id, user_id, name, mime, data, size)
         VALUES ($1, $2, 'image', 'image/png', '', 0)",
    )
    .bind(&file)
    .bind(user)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO agent_images (user_id, key, file_id, machine_id, session)
         VALUES ($1, $2, $2, $3, 's1')",
    )
    .bind(user)
    .bind(&file)
    .bind(machine)
    .execute(pool)
    .await
    .unwrap();
}

/// The rows left that name `machine`, in every table that does.
async fn rows_of(pool: &PgPool, machine: &str) -> i64 {
    let tables = [
        "project_imports WHERE endpoint_id",
        "project_sync WHERE endpoint_id",
        "project_checkouts WHERE machine",
        "project_workers WHERE machine",
        "projects WHERE pin",
        "agent_usage WHERE endpoint_id",
        "agent_session_usage WHERE endpoint_id",
        "agent_session_starts WHERE machine",
        "harness_settings WHERE machine_id",
        "checkout_checks WHERE endpoint_id",
        "terminals WHERE endpoint_id",
        "branch_reviews WHERE endpoint_id",
        "agent_images WHERE machine_id",
        "nodes WHERE endpoint_id",
    ];
    let mut total = 0;
    for table in tables {
        let n: i64 = sqlx::query_scalar(&format!("SELECT count(*) FROM {table}=$1"))
            .bind(machine)
            .fetch_one(pool)
            .await
            .unwrap();
        total += n;
    }
    total
}

#[tokio::test]
async fn removing_a_machine_removes_its_sessions_and_projects() {
    let mut center = CommandCenter::new(2).await;
    let running = || FakeAgents {
        sessions: vec![beep_session("a3f9c1", None)],
        ..FakeAgents::default()
    };
    let doomed = center.node(0, "build-box", running()).await;
    let kept = center.node(0, "other-box", FakeAgents::default()).await;
    let bobs = center.node(1, "bob-box", running()).await;
    let pool = center._db.raw_pool().await.unwrap();
    let alice = center.users[0].id().to_owned();
    let bob = center.users[1].id().to_owned();

    let solo = seed_project(&pool, &alice, "acme/solo", &[&doomed]).await;
    let both = seed_project(&pool, &alice, "acme/both", &[&doomed, &kept]).await;
    let solo_task = seed_worker(&pool, &alice, solo, &doomed).await;
    let both_task = seed_worker(&pool, &alice, both, &doomed).await;
    seed_session_rows(&pool, &alice, &doomed).await;
    // The same names on the kept machine and for bob, who are not touched.
    seed_session_rows(&pool, &alice, &kept).await;
    let bobs_solo = seed_project(&pool, &bob, "acme/solo", &[&bobs]).await;
    seed_worker(&pool, &bob, bobs_solo, &bobs).await;
    seed_session_rows(&pool, &bob, &bobs).await;

    let (_node, client) = center.phone(0).await;
    let (_bob_node, bob_client) = center.phone(1).await;
    assert_eq!(client.call(&ListAgentSessions {}).await.unwrap().len(), 1);
    assert!(rows_of(&pool, &doomed).await >= 14);
    let kept_rows = rows_of(&pool, &kept).await;
    let bobs_rows = rows_of(&pool, &bobs).await;

    client
        .call(&RemoveMachine {
            machine_id: doomed.clone(),
        })
        .await
        .unwrap();

    // Its session does not come back, and neither do its rows.
    assert!(client.call(&ListAgentSessions {}).await.unwrap().is_empty());
    assert_eq!(rows_of(&pool, &doomed).await, 0);

    // The project only it had went, the other stays with one folder.
    let projects = client.call(&ListProjects {}).await.unwrap();
    assert_eq!(
        projects.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(),
        ["acme/both"]
    );
    let tasks: i64 = sqlx::query_scalar("SELECT count(*) FROM project_tasks WHERE id=$1")
        .bind(solo_task)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(tasks, 0);
    let imports = center.users[0].imports().await.unwrap();
    assert_eq!(
        imports
            .iter()
            .map(|row| (row.project.as_str(), row.endpoint.as_str()))
            .collect::<Vec<_>>(),
        [("acme/both", kept.as_str())]
    );
    let home: Vec<String> =
        sqlx::query_scalar("SELECT machine FROM project_checkouts WHERE project_id=$1 AND home")
            .bind(both)
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(home, [kept.as_str()]);
    // The task that lost its worker waits for someone to decide.
    let state: String = sqlx::query_scalar("SELECT state FROM project_tasks WHERE id=$1")
        .bind(both_task)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(state, "blocked");

    // The other machine and bob keep everything.
    assert_eq!(rows_of(&pool, &kept).await, kept_rows);
    assert_eq!(rows_of(&pool, &bobs).await, bobs_rows);
    assert_eq!(
        bob_client.call(&ListAgentSessions {}).await.unwrap().len(),
        1
    );
    assert_eq!(bob_client.call(&ListProjects {}).await.unwrap().len(), 1);

    center.shutdown().await;
}

#[tokio::test]
async fn the_ssh_requests_answer_that_they_were_removed() {
    let mut center = CommandCenter::new(1).await;
    let machine = center.node(0, "build-box", FakeAgents::default()).await;
    let (_node, client) = center.phone(0).await;
    let hosts = client
        .call(&ListSshHosts {
            machine_id: machine.clone(),
        })
        .await
        .unwrap();
    assert!(hosts.is_empty());
    let added = client
        .call(&AddMachineOverSsh {
            machine_id: machine,
            target: "user@host".into(),
            search: String::new(),
        })
        .await
        .unwrap();
    assert_eq!(added.machine_id, None);
    assert!(added.error.contains("removed"), "{}", added.error);
    center.shutdown().await;
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
async fn a_session_that_asks_or_finishes_notifies_unless_a_chat_hears_of_it() {
    let mut center = CommandCenter::new(1).await;
    let mut notices = center.app.notifications.subscribe();
    let alert = |session: &str, kind: AlertKind| Alert {
        session: session.into(),
        agent: "claude".into(),
        title: Some("Fix the login test".into()),
        cwd: "/srv/project".into(),
        kind,
    };
    let finished = AlertKind::TurnEnded {
        stop_reason: StopReason::EndTurn,
        last_message: "All tests pass.".into(),
        error: None,
    };
    let fake = FakeAgents {
        alerts: Some(vec![
            alert("owned1", finished.clone()),
            alert("a3f9c1", finished),
        ]),
        ..FakeAgents::default()
    };
    let go = fake.alerts_go.clone();
    let machine = center.node(0, "build-box", fake).await;
    // A session a chat started reports to that chat instead.
    let online = online(&center.app, &machine).await;
    online.report_to(
        "owned1",
        Follower {
            user: center.users[0].clone(),
            chat: uuid::Uuid::new_v4(),
            node: "build-box".into(),
            agent: "claude".into(),
        },
    );
    go.notify_one();
    let sent = timeout(Duration::from_secs(20), async {
        loop {
            let sent = notices.recv().await.expect("the notices go on");
            if !sent.notice.title.is_empty() {
                return sent;
            }
        }
    })
    .await
    .expect("a notice arrives");
    assert_eq!(sent.user, center.users[0].id());
    assert_eq!(sent.notice.title, "Claude Code on build-box finished");
    assert_eq!(sent.notice.body, "Fix the login test: All tests pass.");
    assert_eq!(sent.notice.session_id, "");
    let key: AgentNoticeKey = sent
        .desktop
        .expect("computers get the session")
        .session_id
        .parse()
        .unwrap();
    assert_eq!(key.session, "a3f9c1", "the chat's session told nobody");
    assert_eq!(key.machine_id, machine);
    assert_eq!(sent.to, Reach::Desktops, "no device started it");
    center.shutdown().await;
}

/// A finished alert for `session`, as the fake node sends it.
fn finished_alert(session: &str) -> Alert {
    Alert {
        session: session.into(),
        agent: "claude".into(),
        title: Some("Fix the login test".into()),
        cwd: "/srv/project".into(),
        kind: AlertKind::TurnEnded {
            stop_reason: StopReason::EndTurn,
            last_message: "All tests pass.".into(),
            error: None,
        },
    }
}

/// Starts a session (the fake names it b0b0b0) on a node from `starter`,
/// then has the node send a finished alert for it and one for a3f9c1,
/// which no device started. Gives whom each alert went to, in order.
async fn alert_reach(center: &mut CommandCenter, starter: &Client) -> Vec<(String, Reach)> {
    alert_reach_of(center, starter, finished_alert).await
}

/// `alert_reach` for the alerts `make` builds.
async fn alert_reach_of(
    center: &mut CommandCenter,
    starter: &Client,
    make: fn(&str) -> Alert,
) -> Vec<(String, Reach)> {
    let mut notices = center.app.notifications.subscribe();
    let fake = FakeAgents {
        alerts: Some(vec![make("b0b0b0"), make("a3f9c1")]),
        ..FakeAgents::default()
    };
    let go = fake.alerts_go.clone();
    let machine = center.node(0, "build-box", fake).await;
    let started = starter
        .call(&start(Some(machine), "Fix the login test"))
        .await
        .unwrap();
    assert_eq!(started.session.id, "b0b0b0");
    go.notify_one();
    timeout(Duration::from_secs(20), async {
        let mut reached = Vec::new();
        while reached.len() < 2 {
            let sent = notices.recv().await.expect("the notices go on");
            if sent.notice.title.is_empty() {
                continue;
            }
            let key: AgentNoticeKey = sent
                .desktop
                .expect("computers get the session")
                .session_id
                .parse()
                .unwrap();
            reached.push((key.session, sent.to));
        }
        reached
    })
    .await
    .expect("both alerts arrive")
}

/// A question for `session`, as a node raises one for an agent or for a permission it asks itself.
fn asking_alert(session: &str) -> Alert {
    Alert {
        kind: AlertKind::Asks {
            question: Question {
                id: 1,
                text: "Allow send files? (Your machines)".into(),
                detail: Some("Machine: box-b".into()),
                options: Vec::new(),
            },
        },
        ..finished_alert(session)
    }
}

#[tokio::test]
async fn a_question_from_a_node_reaches_every_device_whoever_started_the_session() {
    let mut center = CommandCenter::new(1).await;
    let (_node, phone) = center.phone(0).await;
    let reached = alert_reach_of(&mut center, &phone, asking_alert).await;
    assert_eq!(
        reached,
        [
            ("b0b0b0".to_owned(), Reach::Everyone),
            ("a3f9c1".to_owned(), Reach::Everyone),
        ],
        "phones and computers both hear a question"
    );
    center.shutdown().await;
}

#[tokio::test]
async fn a_session_started_on_the_phone_alerts_only_the_phone() {
    let mut center = CommandCenter::new(1).await;
    let (_node, phone) = center.phone(0).await;
    let reached = alert_reach(&mut center, &phone).await;
    assert_eq!(
        reached,
        [
            ("b0b0b0".to_owned(), Reach::Device(center.keys[0].public())),
            ("a3f9c1".to_owned(), Reach::Desktops),
        ],
        "the phone's session tells the phone; one started outside Beep tells the computers"
    );
    center.shutdown().await;
}

#[tokio::test]
async fn a_session_started_on_a_computer_alerts_only_that_computer() {
    let mut center = CommandCenter::new(1).await;
    let mut macs = Vec::new();
    for name in ["MacBook", "Mac Studio"] {
        let key = SecretKey::generate();
        center.users[0]
            .link_key(NewKey {
                endpoint: key.public(),
                name,
                kind: DeviceKind::Desktop {},
            })
            .await
            .unwrap();
        macs.push(key);
    }
    let (_node, macbook) = phone(macs[0].clone(), &center.router).await;
    let reached = alert_reach(&mut center, &macbook).await;
    assert_eq!(
        reached,
        [
            ("b0b0b0".to_owned(), Reach::Device(macs[0].public())),
            ("a3f9c1".to_owned(), Reach::Desktops),
        ],
        "only the Mac that started it, not the other Mac or the phone"
    );
    center.shutdown().await;
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
    /// Where a hanging node hands each first request it takes.
    heard: Option<mpsc::UnboundedSender<SessionsRequest>>,
    /// Where the `claude/1` stream hands the orders it gets.
    orders: Option<mpsc::UnboundedSender<Request>>,
    /// The PNGs `Image` finds, by session.
    images: Vec<(String, Vec<u8>)>,
    /// The files `File` reads, by path; another path has `NoFile`, and one
    /// starting `/refused` an `Error`.
    files: Vec<(String, Vec<u8>)>,
    /// The PNGs `FollowImages` sends, by session, before it holds the
    /// stream.
    sent: Vec<(String, Vec<u8>)>,
    /// What `Models` answers; `None` closes the stream unanswered, as a
    /// node from before it does.
    models: Option<ModelList>,
    /// What `FollowUsage` sends before it holds the stream; `None` closes
    /// it, as a node from before it does.
    usage: Option<Vec<SessionsReply>>,
    /// What `FollowAlerts` sends once `alerts_go` is notified, before it
    /// holds the stream; `None` closes it, as a node from before it does.
    alerts: Option<Vec<Alert>>,
    alerts_go: Arc<Notify>,
}

impl FakeAgents {
    fn offers(self) -> Offers {
        let fake = Arc::new(self);
        Offers::with_protocols(vec![
            Arc::new(FakeSessions(fake.clone())),
            Arc::new(FakeClaude(fake)),
        ])
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
                if let Some(heard) = &fake.heard {
                    let _ = heard.send(request.clone());
                }
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
            if matches!(request, SessionsRequest::FollowAlerts {}) {
                let Some(alerts) = &fake.alerts else {
                    return Ok(());
                };
                fake.alerts_go.notified().await;
                for alert in alerts {
                    tx.send(&SessionsReply::Alert {
                        alert: alert.clone(),
                    })
                    .await?;
                }
                while rx.recv::<SessionsRequest>().await?.is_some() {}
                return Ok(());
            }
            if matches!(request, SessionsRequest::FollowUsage { .. }) {
                let Some(usage) = &fake.usage else {
                    return Ok(());
                };
                for reply in usage {
                    tx.send(reply).await?;
                }
                // Asks to read the plans again are taken and dropped.
                while let Some(SessionsRequest::RefreshPlanUsage {}) = rx.recv().await? {}
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
                (SessionsRequest::Transcript { session, last, .. }, _) => {
                    SessionsReply::Transcript {
                        entries: vec![Entry::new(
                            EntryKind::Agent,
                            format!("{session}: the last {last}"),
                        )],
                        earlier: 4,
                    }
                }
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
                (SessionsRequest::File { path, .. }, _) if path.starts_with("/refused") => {
                    SessionsReply::Error {
                        message: "not a file".into(),
                    }
                }
                (SessionsRequest::File { path, offset }, _) => fake
                    .files
                    .iter()
                    .find(|(at, _)| *at == path)
                    .map_or(SessionsReply::NoFile {}, |(_, data)| {
                        let from = usize::try_from(offset).unwrap().min(data.len());
                        SessionsReply::File {
                            size: data.len() as u64,
                            mime: "video/mp4".into(),
                            data: data[from..].to_vec(),
                        }
                    }),
                // The working folder's requests: a path with `..` is
                // refused as the node does; anything else answers in kind.
                (
                    SessionsRequest::Folder { path, .. }
                    | SessionsRequest::ReadFile { path, .. }
                    | SessionsRequest::WriteFile { path, .. },
                    _,
                ) if path.contains("..") => SessionsReply::Error {
                    message: format!("{path} leads outside the session's folder"),
                },
                // A diff: refused outside `/srv`, as a node refuses a
                // folder outside its reach; else one file named by the head.
                (SessionsRequest::Diff { ask }, _) if !ask.root.starts_with("/srv") => {
                    SessionsReply::Error {
                        message: format!(
                            "{} is outside the folders Beep may read changes in",
                            ask.root
                        ),
                    }
                }
                (SessionsRequest::Diff { ask }, _) => SessionsReply::Diff {
                    diff: WorkDiff {
                        root: ask.root,
                        base: if ask.base.is_empty() {
                            "origin/main".into()
                        } else {
                            ask.base
                        },
                        merge_base: "abc1234".into(),
                        working_tree: ask.head.is_empty(),
                        files: vec![DiffFile {
                            path: if ask.head.is_empty() {
                                "src/main.rs".into()
                            } else {
                                ask.head
                            },
                            old_path: None,
                            status: DiffStatus::Modified {},
                            additions: 1,
                            deletions: 0,
                            hunks: Vec::new(),
                            left_out: None,
                        }],
                        additions: 1,
                        deletions: 0,
                        more: 0,
                    },
                },
                // A tree from the top; below it, a folder alone, as a node
                // from before trees answers.
                (SessionsRequest::Folder { path, depth, .. }, _)
                    if depth > 0 && path.is_empty() =>
                {
                    SessionsReply::Tree {
                        tree: WorkTree {
                            folders: ["", "src"]
                                .map(|path| WorkFolder {
                                    path: path.into(),
                                    entries: Vec::new(),
                                    more: 0,
                                    git: true,
                                })
                                .to_vec(),
                            truncated: true,
                        },
                    }
                }
                (SessionsRequest::Folder { path, ignored, .. }, _) => SessionsReply::Folder {
                    folder: WorkFolder {
                        path,
                        entries: vec![WorkEntry {
                            name: "main.rs".into(),
                            folder: false,
                            size: 12,
                            ignored,
                            change: Some(GitChange::Modified {}),
                        }],
                        more: 0,
                        git: true,
                    },
                },
                (
                    SessionsRequest::ReadFile {
                        root,
                        path,
                        offset,
                        len,
                    },
                    _,
                ) => fake
                    .files
                    .iter()
                    .find(|(at, _)| *at == format!("{root}/{path}"))
                    .map_or(SessionsReply::NoFile {}, |(_, data)| {
                        let from = usize::try_from(offset).unwrap().min(data.len());
                        let to = (from + len as usize).min(data.len());
                        SessionsReply::File {
                            size: data.len() as u64,
                            mime: "application/octet-stream".into(),
                            data: data[from..to].to_vec(),
                        }
                    }),
                (SessionsRequest::WriteFile { path, .. }, _) => SessionsReply::Written {
                    path: format!("{path}.taken"),
                },
                (SessionsRequest::CustomAgents { folder }, _) if folder.starts_with("/srv") => {
                    SessionsReply::CustomAgents {
                        agents: vec![CustomAgent {
                            name: "reviewer".into(),
                            harness: CustomAgentHarness::Claude {},
                            scope: CustomAgentScope::Project {},
                            description: "Reviews a branch".into(),
                            model: Some("opus".into()),
                            tools: vec!["Read".into()],
                            color: Some("blue".into()),
                            path: ".claude/agents/reviewer.md".into(),
                            prompt: "Review it.".into(),
                            prompt_cut: false,
                            error: None,
                        }],
                    }
                }
                (SessionsRequest::CustomAgents { folder }, _) => SessionsReply::Error {
                    message: format!("the folder {folder} is not there"),
                },
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
        Self::with_model(users, Arc::new(NoModel)).await
    }

    async fn with_model(users: usize, provider: Arc<impl LlmProvider + 'static>) -> Self {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let keys: Vec<SecretKey> = (0..users).map(|_| SecretKey::generate()).collect();
        let model = "openai:test".parse().unwrap();
        let models = ModelChoices::resolve(provider.as_ref(), model, Vec::new()).await;
        let app = App::new(
            store.clone(),
            provider,
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
        let key = self.node_key(user, name, fake).await;
        key.public().to_string()
    }

    /// `node`, giving the node's key.
    async fn node_key(&mut self, user: usize, name: &'static str, fake: FakeAgents) -> SecretKey {
        let key = SecretKey::generate();
        self.users[user]
            .link_key(NewKey {
                endpoint: key.public(),
                name,
                kind: DeviceKind::Desktop {},
            })
            .await
            .unwrap();
        self.connect(&key, name, fake).await;
        online(&self.app, &key.public().to_string()).await;
        key
    }

    /// Connects a node with the linked key `key` serving `fake`, on an
    /// endpoint of its own, as a node does when it starts again.
    async fn connect(&mut self, key: &SecretKey, name: &'static str, fake: FakeAgents) {
        let addr = self.router.endpoint().addr();
        let node = endpoint(key.clone()).await;
        self.nodes.push(tokio::spawn(async move {
            beep_node::link::connect(&node, addr, name, &fake.offers()).await
        }));
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
async fn agent_sessions_list_none_of_another_users_machines() {
    let mut center = CommandCenter::new(1).await;
    center.node(0, "build-box", FakeAgents::default()).await;
    let (_node, bob) = phone(SecretKey::generate(), &center.router).await;
    assert!(bob.call(&ListAgentSessions {}).await.unwrap().is_empty());
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

#[tokio::test]
async fn harness_settings_save_to_every_machine_even_one_offline() {
    let mut center = CommandCenter::new(1).await;
    let build = center.node(0, "build-box", FakeAgents::default()).await;
    let gpu = center.node(0, "gpu-box", FakeAgents::default()).await;
    // gpu-box goes offline; it is still one of the user's machines.
    center.nodes[1].abort();
    let (_node, client) = center.phone(0).await;
    let own = |program: &str| HarnessSettings {
        enabled: true,
        program: program.into(),
        home: String::new(),
        args: Vec::new(),
        env: Vec::new(),
    };
    client
        .call(&SaveHarnessSettings {
            machine_id: gpu.clone(),
            agent: "claude".into(),
            settings: own("/srv/claude"),
        })
        .await
        .unwrap();
    let saved = client
        .call(&SaveHarnessSettingsAll {
            from_machine: build.clone(),
            agent: "claude".into(),
            settings: HarnessSettings {
                args: vec!["--verbose".into()],
                env: vec![HarnessEnv {
                    name: "HTTPS_PROXY".into(),
                    value: Some(SecretText::new("http://proxy:3128")),
                }],
                ..own("/opt/claude")
            },
            include_program: false,
        })
        .await
        .unwrap();
    assert_eq!(saved, 2);
    let mut all = client
        .call(&ListHarnessSettings {
            agent: "claude".into(),
        })
        .await
        .unwrap();
    all.sort_by(|a, b| a.machine_name.cmp(&b.machine_name));
    assert_eq!(all[0].machine_name, "build-box");
    assert_eq!(all[0].settings.program, "/opt/claude");
    // The binary path stays the machine's own; the rest is copied.
    assert_eq!(all[1].machine_name, "gpu-box");
    assert_eq!(all[1].settings.program, "/srv/claude");
    for machine in &all {
        assert_eq!(machine.settings.args, ["--verbose"]);
        assert_eq!(machine.settings.env[0].name, "HTTPS_PROXY");
        assert_eq!(machine.settings.env[0].value, None);
    }
    let missing = client
        .call(&SaveHarnessSettingsAll {
            from_machine: "not-mine".into(),
            agent: "claude".into(),
            settings: own(""),
            include_program: false,
        })
        .await;
    let missing = missing.unwrap_err().downcast::<ApiError>().unwrap();
    assert!(matches!(missing, ApiError::NotFound {}), "{missing:?}");
    center.shutdown().await;
}

#[tokio::test]
async fn flsh_settings_save_read_back_and_refuse_bad_values() {
    let center = CommandCenter::new(1).await;
    let (_node, client) = center.phone(0).await;
    assert_eq!(
        client.call(&GetFlshSettings {}).await.unwrap(),
        FlshSettings::default()
    );
    let settings = FlshSettings {
        zdr: Some(true),
        sort: Some(FlshSort::Price),
        models: vec![FlshModelRule {
            model: "openrouter:vendor/model".parse().unwrap(),
            autocompact: FlshAutoCompact {
                enabled: true,
                tokens: Some(80_000),
            },
        }],
        ..FlshSettings::default()
    };
    client
        .call(&SaveFlshSettings {
            settings: settings.clone(),
        })
        .await
        .unwrap();
    assert_eq!(client.call(&GetFlshSettings {}).await.unwrap(), settings);
    let too_small = FlshSettings {
        autocompact: Some(FlshAutoCompact {
            enabled: true,
            tokens: Some(10),
        }),
        ..FlshSettings::default()
    };
    let refused = client
        .call(&SaveFlshSettings {
            settings: too_small,
        })
        .await
        .unwrap_err();
    assert!(matches!(
        refused.downcast::<ApiError>().unwrap(),
        ApiError::BadRequest { .. }
    ));
    // A refused save keeps the settings before.
    assert_eq!(client.call(&GetFlshSettings {}).await.unwrap(), settings);
    center.shutdown().await;
}

/// Calls `code_agent` with the next arguments at the start of each turn, then answers. A turn
/// waits for a permit, so the test can follow its run.
struct CallsCodeAgent {
    turns: Mutex<VecDeque<Value>>,
    go: Arc<Semaphore>,
}

#[async_trait]
impl LlmProvider for CallsCodeAgent {
    fn name(&self) -> &str {
        "test"
    }
    fn provider(&self) -> Provider {
        Provider::OpenAi
    }
    fn complete(&self, request: CompletionRequest) -> CompletionStream {
        let main = request.tools.iter().any(|tool| tool.name == "code_agent");
        let after_tool = request
            .messages
            .last()
            .is_some_and(|message| message.role == Role::Tool);
        let args = (main && !after_tool)
            .then(|| self.turns.lock().unwrap().pop_front())
            .flatten();
        let go = self.go.clone();
        Box::pin(async_stream::stream! {
            let Some(args) = args else {
                yield Ok(StreamEvent::ContentDelta(if main { "Done." } else { "{}" }.into()));
                yield Ok(StreamEvent::Finished(FinishReason::Stop));
                return;
            };
            go.acquire().await.expect("the test keeps the semaphore").forget();
            yield Ok(StreamEvent::ToolCallStart {
                index: 0,
                id: format!("call-{}", args["action"].as_str().expect("an action")),
                name: "code_agent".into(),
            });
            yield Ok(StreamEvent::ToolCallDelta { index: 0, arguments: args.to_string() });
            yield Ok(StreamEvent::Finished(FinishReason::ToolCalls));
        })
    }
}

/// Runs one turn in `chat` until it ends.
async fn turn(center: &CommandCenter, go: &Semaphore, chat: Uuid) {
    let message = Message {
        session_id: chat,
        request_id: Uuid::new_v4(),
        message: "Go ahead".into(),
        cron_id: None,
        scheduled: None,
        always_sent: false,
        woken: None,
        images: Vec::new(),
        files: Vec::new(),
        voice: None,
        spoken: false,
        answer: None,
        model: None,
        thinking: None,
        origin: DeviceKind::Phone {},
        platform: None,
        draws: Draws::default(),
        finished_research: false,
    };
    let CreateRun::Created(run) = center.app.submit(&center.users[0], message).await.unwrap()
    else {
        panic!("the run was not created");
    };
    let live = center
        .app
        .live(&run.id)
        .await
        .expect("the turn waits for its permit");
    let mut events = live.subscribe(0);
    go.add_permits(1);
    timeout(Duration::from_secs(30), async {
        let mut backlog = std::mem::take(&mut events.backlog).into_iter();
        loop {
            let event = match backlog.next() {
                Some(event) => event,
                None => events.live.recv().await.expect("the run streams"),
            };
            if matches!(event.event, RunEvent::RunFinished { .. }) {
                return;
            }
        }
    })
    .await
    .expect("the turn ends");
}

/// The card `chat` stopped on, with the call it answers.
async fn card_in(client: &Client, chat: Uuid) -> Option<(String, Approval)> {
    let id = chat.to_string();
    let history = client.call(&GetSession { id }).await.unwrap();
    history.into_iter().find_map(|entry| match entry {
        HistoryEntry::Approval { query_id, approval } => Some((query_id, approval)),
        _ => None,
    })
}

/// A chat that starts a coding agent on a machine: nothing runs until the user allows the card.
#[tokio::test]
async fn permissions_ask_before_a_chat_starts_a_coding_agent() {
    let go = Arc::new(Semaphore::new(0));
    let model = CallsCodeAgent {
        turns: Mutex::new(VecDeque::from([
            json!({ "action": "status" }),
            json!({
                "action": "start",
                "agent": "claude",
                "in": "build-box",
                "cwd": "/srv/project",
                "prompt": "Fix the tests",
                "wait_secs": 0,
            }),
        ])),
        go: go.clone(),
    };
    let mut center = CommandCenter::with_model(1, Arc::new(model)).await;
    let (orders, mut received) = mpsc::unbounded_channel();
    center
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

    let chat = Uuid::new_v4();
    turn(&center, &go, chat).await;
    assert!(card_in(&client, chat).await.is_none(), "a status asked");

    let chat = Uuid::new_v4();
    turn(&center, &go, chat).await;
    assert!(received.try_recv().is_err(), "the agent started unasked");
    let (query_id, approval) = card_in(&client, chat).await.expect("the start's card");
    let shown: Vec<(&str, &str)> = approval
        .fields
        .iter()
        .map(|field| (field.label.as_str(), field.value.as_str()))
        .collect();
    assert_eq!(
        shown,
        [
            ("Action", "Start"),
            ("Agent", "Claude Code"),
            ("Machine", "build-box"),
            ("Folder", "/srv/project"),
            ("Prompt", "Fix the tests"),
        ]
    );

    client
        .call(&AnswerApproval {
            session_id: chat,
            query_id,
            decision: ApprovalDecision::Allow {},
            for_session: false,
        })
        .await
        .unwrap();
    let order = timeout(Duration::from_secs(10), received.recv())
        .await
        .expect("the agent starts once allowed")
        .expect("the fake still runs");
    assert!(
        matches!(&order, Request::Start { prompt, .. } if prompt == "Fix the tests"),
        "{order:?}"
    );
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
    // Polls: the reply does not wait for the copy, and keeping it has no
    // signal to wait on.
    let elsewhere = timeout(Duration::from_secs(20), async {
        loop {
            if let Ok(got) = client.call(&get("000000", &key)).await {
                return got;
            }
            sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("the picture was kept");
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
async fn agent_files_are_read_from_the_node_in_parts() {
    let mut center = CommandCenter::new(1).await;
    let clip = b"ftypmp42 frames".to_vec();
    let build = center
        .node(
            0,
            "build-box",
            FakeAgents {
                files: vec![("/srv/project/out/clip.mp4".into(), clip.clone())],
                ..FakeAgents::default()
            },
        )
        .await;
    let (_node, client) = center.phone(0).await;
    let get = |path: &str, offset: u64| GetAgentFile {
        machine_id: build.clone(),
        path: path.into(),
        offset,
    };
    let got = client
        .call(&get("/srv/project/out/clip.mp4", 0))
        .await
        .unwrap();
    assert_eq!(
        got,
        AgentFileChunk {
            size: clip.len() as u64,
            mime: "video/mp4".into(),
            data: clip.clone(),
        }
    );
    let rest = client
        .call(&get("/srv/project/out/clip.mp4", 9))
        .await
        .unwrap();
    assert_eq!(rest.data, b"frames");
    let elsewhere = GetAgentFile {
        machine_id: "nowhere".into(),
        path: "/srv/project/out/clip.mp4".into(),
        offset: 0,
    };
    let cases = [
        (get("/srv/project/out/gone.mp4", 0), true),
        (get("/srv/project/notes.txt", 0), false),
        (get("/refused/clip.mp4", 0), false),
        (elsewhere, true),
    ];
    for (request, missing) in cases {
        let error = client.call(&request).await.unwrap_err();
        let error = error.downcast::<ApiError>().unwrap();
        if missing {
            assert!(matches!(error, ApiError::NotFound {}), "{request:?}");
        } else {
            assert!(matches!(error, ApiError::BadRequest { .. }), "{request:?}");
        }
    }
    center.shutdown().await;
}

#[tokio::test]
async fn a_sessions_folder_is_listed_read_and_written_through_its_node() {
    let mut center = CommandCenter::new(1).await;
    let build = center
        .node(
            0,
            "build-box",
            FakeAgents {
                files: vec![("/srv/project/notes.txt".into(), b"0123456789".to_vec())],
                ..FakeAgents::default()
            },
        )
        .await;
    let (_node, client) = center.phone(0).await;
    let folder = client
        .call(&ListWorkFolder {
            machine_id: build.clone(),
            root: "/srv/project".into(),
            path: "src".into(),
            ignored: true,
        })
        .await
        .unwrap();
    assert_eq!(folder.path, "src");
    assert_eq!(folder.entries[0].change, Some(GitChange::Modified {}));
    assert!(folder.entries[0].ignored);
    let tree = |path: &str| ListWorkTree {
        machine_id: build.clone(),
        root: "/srv/project".into(),
        path: path.into(),
        ignored: true,
        depth: 3,
    };
    let top = client.call(&tree("")).await.unwrap();
    let paths: Vec<_> = top.folders.iter().map(|f| f.path.as_str()).collect();
    assert_eq!((paths, top.truncated), (vec!["", "src"], true));
    let alone = client.call(&tree("src")).await.unwrap();
    assert_eq!(alone.folders, [folder]);
    assert!(!alone.truncated);
    let read = |path: &str, offset: u64, len: u32| ReadWorkFile {
        machine_id: build.clone(),
        root: "/srv/project".into(),
        path: path.into(),
        offset,
        len,
    };
    let part = client.call(&read("notes.txt", 2, 3)).await.unwrap();
    assert_eq!((part.size, part.data), (10, b"234".to_vec()));
    let written = client
        .call(&WriteWorkFile {
            machine_id: build.clone(),
            root: "/srv/project".into(),
            path: ".beep/uploads/build.log".into(),
            offset: 0,
            size: 2,
            data: b"hi".to_vec(),
        })
        .await
        .unwrap();
    assert_eq!(written, ".beep/uploads/build.log.taken");
    let too_big = WriteWorkFile {
        machine_id: build.clone(),
        root: "/srv/project".into(),
        path: "big.bin".into(),
        offset: 0,
        size: MAX_WORK_UPLOAD + 1,
        data: Vec::new(),
    };
    let outside = ListWorkFolder {
        machine_id: build.clone(),
        root: "/srv/project".into(),
        path: "../etc".into(),
        ignored: false,
    };
    for error in [
        client.call(&too_big).await.unwrap_err(),
        client.call(&outside).await.unwrap_err(),
    ] {
        let error = error.downcast::<ApiError>().unwrap();
        assert!(matches!(error, ApiError::BadRequest { .. }), "{error:?}");
    }
    let missing = client.call(&read("gone.txt", 0, 10)).await.unwrap_err();
    assert!(matches!(
        missing.downcast::<ApiError>().unwrap(),
        ApiError::NotFound {}
    ));
    center.shutdown().await;
}

/// A node that restarts leaves the server holding its old connection for a
/// while; a folder read sent on it goes again on the new one instead of
/// failing.
#[tokio::test]
async fn a_folder_read_lost_with_a_replaced_connection_goes_again() {
    let mut center = CommandCenter::new(1).await;
    let (heard, mut taken) = mpsc::unbounded_channel();
    let stale = FakeAgents {
        hang: true,
        heard: Some(heard),
        ..FakeAgents::default()
    };
    let key = center.node_key(0, "build-box", stale).await;
    let (_node, client) = center.phone(0).await;
    let list = ListWorkFolder {
        machine_id: key.public().to_string(),
        root: "/srv/project".into(),
        path: "src".into(),
        ignored: false,
    };
    let call = client.call(&list);
    tokio::pin!(call);
    // The stale connection takes the read and never answers it.
    loop {
        tokio::select! {
            done = &mut call => panic!("answered on the stale connection: {done:?}"),
            request = taken.recv() => {
                if matches!(request, Some(SessionsRequest::Folder { .. })) {
                    break;
                }
            }
        }
    }
    // The node starts again and reconnects, replacing that connection.
    center
        .connect(&key, "build-box", FakeAgents::default())
        .await;
    let folder = timeout(Duration::from_secs(30), call)
        .await
        .expect("the read ends")
        .expect("the read goes again on the new connection");
    assert_eq!(folder.path, "src");
    assert_eq!(folder.entries[0].name, "main.rs");
    center.shutdown().await;
}

#[tokio::test]
async fn a_sessions_changes_are_read_through_its_node() {
    let mut center = CommandCenter::new(1).await;
    let build = center.node(0, "build-box", FakeAgents::default()).await;
    let (_node, client) = center.phone(0).await;
    let ask = |root: &str| GetWorkDiff {
        machine_id: build.clone(),
        root: root.into(),
        base: String::new(),
        head: String::new(),
        only: Vec::new(),
        lockfiles: false,
    };
    let diff = client.call(&ask("/srv/project")).await.unwrap();
    assert_eq!(diff.root, "/srv/project");
    assert_eq!(diff.base, "origin/main");
    assert!(diff.working_tree);
    assert_eq!(diff.files[0].path, "src/main.rs");
    let refused = client.call(&ask("/etc")).await.unwrap_err();
    let refused = refused.downcast::<ApiError>().unwrap();
    assert!(
        matches!(&refused, ApiError::BadRequest { message } if message.contains("build-box")),
        "{refused:?}"
    );
    let mut elsewhere = ask("/srv/project");
    elsewhere.machine_id = "not-a-machine".into();
    let missing = client.call(&elsewhere).await.unwrap_err();
    assert!(matches!(
        missing.downcast::<ApiError>().unwrap(),
        ApiError::NotFound {}
    ));
    center.shutdown().await;
}

#[tokio::test]
async fn a_projects_custom_agents_are_read_through_its_node() {
    let mut center = CommandCenter::new(1).await;
    let build = center.node(0, "build-box", FakeAgents::default()).await;
    let (_node, client) = center.phone(0).await;
    let agents = client
        .call(&ListCustomAgents {
            machine_id: build.clone(),
            folder: "/srv/project".into(),
        })
        .await
        .unwrap();
    assert_eq!(agents.len(), 1);
    assert_eq!(agents[0].name, "reviewer");
    assert_eq!(agents[0].harness, CustomAgentHarness::Claude {});
    let missing = ListCustomAgents {
        machine_id: build.clone(),
        folder: "/gone".into(),
    };
    let error = client.call(&missing).await.unwrap_err();
    let error = error.downcast::<ApiError>().unwrap();
    assert!(matches!(error, ApiError::BadRequest { .. }), "{error:?}");
    let elsewhere = ListCustomAgents {
        machine_id: "no-such-machine".into(),
        folder: "/srv/project".into(),
    };
    let error = client.call(&elsewhere).await.unwrap_err();
    let error = error.downcast::<ApiError>().unwrap();
    assert!(matches!(error, ApiError::NotFound {}), "{error:?}");
    center.shutdown().await;
}

#[tokio::test]
async fn a_thumb_is_small_and_the_full_picture_is_what_is_kept() {
    let mut center = CommandCenter::new(1).await;
    // Noise, which a JPEG of it is smaller than.
    let pixels = image::RgbImage::from_fn(1600, 900, |x, y| {
        let [r, g, b, _] = (x ^ (y << 12)).wrapping_mul(2_654_435_761).to_be_bytes();
        image::Rgb([r, g, b])
    });
    let mut png = std::io::Cursor::new(Vec::new());
    pixels.write_to(&mut png, image::ImageFormat::Png).unwrap();
    let png = png.into_inner();
    let build = center
        .node(
            0,
            "build-box",
            FakeAgents {
                sessions: vec![beep_session("a3f9c1", None)],
                images: vec![("a3f9c1".into(), png.clone())],
                ..FakeAgents::default()
            },
        )
        .await;
    let (_node, client) = center.phone(0).await;
    let key = image_key(&png);
    let thumb = client
        .call(&GetAgentThumb {
            machine_id: build.clone(),
            session: "a3f9c1".into(),
            key: key.clone(),
        })
        .await
        .unwrap();
    assert_eq!(thumb.key, key);
    assert_eq!(thumb.mime, "image/jpeg");
    assert!(thumb.data.len() < png.len());
    let full = GetAgentImage {
        machine_id: build,
        session: "000000".into(),
        key,
    };
    // Polls: the reply does not wait for the copy, and keeping it has no
    // signal to wait on. Only the kept copy answers for another session.
    let kept = timeout(Duration::from_secs(20), async {
        loop {
            if let Ok(got) = client.call(&full).await {
                return got;
            }
            sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("the picture was kept");
    assert_eq!(kept.mime, "image/png");
    assert_eq!(kept.data, png);
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
async fn a_sessions_usage_and_modes_come_from_its_node() {
    let mut center = CommandCenter::new(1).await;
    let machine = center
        .node(
            0,
            "build-box",
            FakeAgents {
                sessions: vec![SessionInfo {
                    context: Some(AgentContext {
                        used: 124_000,
                        size: 200_000,
                    }),
                    tokens: 184_000,
                    cost_usd: Some(1.42),
                    modes: vec![ModeOption {
                        id: "plan".into(),
                        name: "Plan".into(),
                    }],
                    mode: Some("plan".into()),
                    plan_ready: Some("1. Add a flag".into()),
                    ..beep_session("a3f9c1", None)
                }],
                ..FakeAgents::default()
            },
        )
        .await;
    let (_node, client) = center.phone(0).await;
    let modes = GetAgentSessionModes {
        machine_id: machine.clone(),
        agent: "claude".into(),
        session: "a3f9c1".into(),
    };
    assert_eq!(
        client.call(&modes).await.unwrap(),
        AgentSessionModes {
            current: Some("plan".into()),
            modes: vec![AgentMode {
                id: "plan".into(),
                name: "Plan".into(),
            }],
            plan: Some("1. Add a flag".into()),
        }
    );
    let get = |session: &str| GetAgentSessionUsage {
        machine_id: machine.clone(),
        agent: "claude".into(),
        session: session.into(),
    };
    let usage = client.call(&get("a3f9c1")).await.unwrap();
    assert_eq!(
        usage,
        AgentSessionUsage {
            context: Some(AgentContext {
                used: 124_000,
                size: 200_000
            }),
            tokens: 184_000,
            cost_usd: Some(1.42),
        }
    );
    let missing = client.call(&get("b7e2d0")).await.unwrap_err();
    assert!(matches!(
        missing.downcast::<ApiError>().unwrap(),
        ApiError::NotFound {}
    ));
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

/// Links a node of the first user's serving `extra` and waits until it is
/// online. Gives its machine id.
async fn log_node(
    center: &mut CommandCenter,
    name: &'static str,
    extra: Vec<Arc<dyn Protocol>>,
) -> String {
    let key = SecretKey::generate();
    center.users[0]
        .link_key(NewKey {
            endpoint: key.public(),
            name,
            kind: DeviceKind::Desktop {},
        })
        .await
        .unwrap();
    let addr = center.router.endpoint().addr();
    let node = endpoint(key.clone()).await;
    center.nodes.push(tokio::spawn(async move {
        let offers = Offers::with_protocols(extra);
        beep_node::link::connect(&node, addr, name, &offers).await
    }));
    let id = key.public().to_string();
    online(&center.app, &id).await;
    id
}

#[tokio::test]
async fn an_app_reads_the_recent_log_of_its_own_machines() {
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("beep.log");
    let pad = "x".repeat(400);
    // Dated ahead so no line is too old. The last line is chatter to hide.
    let mut log: String = (0..1500)
        .map(|n| format!("2099-01-01T00:00:00Z   INFO beep_node::link: link.rs:1 line {n} {pad}\n"))
        .collect();
    log.push_str("2099-01-01T00:00:00Z   INFO iroh::socket: socket.rs:1 poll_send\n");
    std::fs::write(&path, log).unwrap();

    let mut center = CommandCenter::new(2).await;
    let with_log = log_node(&mut center, "with-log", vec![Arc::new(Logs::file(path))]).await;
    let no_log = log_node(
        &mut center,
        "no-log",
        vec![Arc::new(Logs::file(folder.path().join("missing.log")))],
    )
    .await;
    let old = log_node(&mut center, "old", Vec::new()).await;
    let (_alice_node, alice) = center.phone(0).await;
    let (_bob_node, bob) = center.phone(1).await;
    let ask = |machine_id: &str, lines| GetMachineLog {
        machine_id: machine_id.to_owned(),
        lines,
    };
    let refusal = |error: anyhow::Error| match error.downcast::<ApiError>().unwrap() {
        ApiError::BadRequest { message } => message,
        other => panic!("{other:?}"),
    };

    let last = alice.call(&ask(&with_log, 2)).await.unwrap();
    let last: Vec<&str> = last.lines().collect();
    assert_eq!(last.len(), 2);
    assert!(last[0].contains("line 1498 "), "{}", last[0]);
    assert!(last[1].contains("line 1499 "), "{}", last[1]);

    // Past the byte cap it gives whole lines from the end.
    let capped = alice.call(&ask(&with_log, u32::MAX)).await.unwrap();
    assert!(capped.len() <= beep_server::protocol::node::logs::MAX_BYTES);
    assert!(capped.starts_with("2099-"), "{}", &capped[..40]);
    assert!(capped.ends_with(&format!("line 1499 {pad}\n")));

    let none = alice.call(&ask(&no_log, 10)).await.unwrap_err();
    assert!(refusal(none).starts_with("opening "));
    let too_old = alice.call(&ask(&old, 10)).await.unwrap_err();
    assert_eq!(
        refusal(too_old),
        "beep-node on old is too old to send its log. Update it."
    );
    let foreign = bob.call(&ask(&with_log, 10)).await.unwrap_err();
    assert!(matches!(
        foreign.downcast::<ApiError>().unwrap(),
        ApiError::NotFound {}
    ));
    center.shutdown().await;
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
    let offers = Offers::with_protocols(
        (0..200)
            .map(|_| Arc::new(BigName) as Arc<dyn Protocol>)
            .collect(),
    );
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

#[tokio::test]
async fn what_agents_spend_and_their_plans_reach_the_pill() {
    let mut center = CommandCenter::new(2).await;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let turn =
        |session: &str, agent: &str, cost: Option<f64>, tokens: u64| SessionsReply::TurnUsage {
            usage: TurnUsage {
                session: session.into(),
                agent: agent.into(),
                cwd: "/srv/work/widget".into(),
                at: now,
                cost_usd: cost,
                tokens,
                models: vec![ModelTokens {
                    model: format!("{agent}-model").into(),
                    tokens,
                }],
                agent_session: None,
            },
        };
    let plan = |at: u64, used: f64| SessionsReply::PlanUsage {
        usage: PlanUsage {
            agent: "claude".into(),
            plan: Some("max".into()),
            windows: vec![LimitWindow {
                minutes: 300,
                used_percent: used,
                resets_at: Some(now + 3600),
                scope: None,
            }],
            at,
        },
    };
    center
        .node(
            0,
            "build-box",
            FakeAgents {
                usage: Some(vec![
                    turn("a3f9c1", "claude", Some(1.5), 4_000),
                    turn("a3f9c1", "claude", Some(0.25), 1_000),
                    turn("b7e2d0", "codex", None, 900_000),
                    plan(now - 60, 38.0),
                ]),
                ..FakeAgents::default()
            },
        )
        .await;
    // A machine whose plan report is older does not replace the fresher.
    center
        .node(
            0,
            "mac",
            FakeAgents {
                usage: Some(vec![plan(now - 600, 90.0)]),
                ..FakeAgents::default()
            },
        )
        .await;
    // A node from before `FollowUsage` closes the stream.
    center.node(0, "old", FakeAgents::default()).await;
    let (_node, client) = center.phone(0).await;
    // Polls: keeping the reports has no signal to wait on.
    let view = timeout(Duration::from_secs(20), async {
        loop {
            let view = client.call(&GetUsage { refresh: true }).await.unwrap();
            let codex = view.harnesses.iter().any(|h| h.agent == "codex");
            let claude = view.harnesses.iter().any(|h| h.tokens == 5_000);
            if codex && claude && !view.limits.is_empty() {
                return view;
            }
            sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("the usage was kept");
    assert_eq!(view.today.len(), 1);
    assert_eq!(view.today[0].project, "widget");
    assert!((view.today[0].cost_usd - 1.75).abs() < 1e-9);
    assert_eq!(
        view.today[0].unpriced,
        [AgentTokens {
            agent: "codex".into(),
            tokens: 900_000
        }]
    );
    assert!((view.week.cost_usd - 1.75).abs() < 1e-9);
    let claude = &view.harnesses[0];
    assert_eq!(
        (claude.agent.as_str(), claude.cost_usd),
        ("claude", Some(1.75))
    );
    assert_eq!(claude.models[0].model, "claude-model");
    assert_eq!(view.harnesses[1].cost_usd, None);
    assert_eq!(view.limits.len(), 1);
    assert_eq!(view.limits[0].windows[0].used_percent, 38.0);
    assert_eq!(view.limits[0].machine_name, "build-box");
    // Another user sees none of it.
    let (_other, theirs) = center.phone(1).await;
    let none = theirs.call(&GetUsage { refresh: false }).await.unwrap();
    assert!(none.today.is_empty() && none.limits.is_empty() && none.harnesses.is_empty());

    for node in &center.nodes {
        node.abort();
    }
    center.shutdown().await;
}

#[tokio::test]
async fn sessions_read_from_the_logs_count_once() {
    let mut center = CommandCenter::new(1).await;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let hour = now / 3600 * 3600;
    let spend =
        |session: &str, agent: &str, cost: f64, billing: Billing| SessionsReply::SessionSpend {
            usage: SessionSpend {
                agent: agent.into(),
                session: session.into(),
                cwd: "/srv/work/widget".into(),
                since: now - 35 * 24 * 3600,
                hours: vec![HourSpend {
                    hour,
                    model: format!("{agent}-model").into(),
                    input_tokens: 1_000,
                    output_tokens: 500,
                    cost_usd: cost,
                    billing,
                    ..HourSpend::default()
                }],
            },
        };
    // A turn Beep watched of a session its log also covers.
    let watched = SessionsReply::TurnUsage {
        usage: TurnUsage {
            session: "beep-1".into(),
            agent: "claude".into(),
            cwd: "/srv/work/widget".into(),
            at: now,
            cost_usd: Some(1.5),
            tokens: 1_500,
            models: Vec::new(),
            agent_session: Some("c-41".into()),
        },
    };
    center
        .node(
            0,
            "build-box",
            FakeAgents {
                usage: Some(vec![
                    watched,
                    spend("c-41", "claude", 2.25, Billing::Unknown),
                    // Read again, the session replaces what was kept of it.
                    spend("c-41", "claude", 3.0, Billing::Unknown),
                    spend("x-9", "codex", 0.5, Billing::Unknown),
                    spend("f-2", "flsh", 1.25, Billing::Metered),
                ]),
                ..FakeAgents::default()
            },
        )
        .await;
    let (_node, client) = center.phone(0).await;
    // Polls: keeping the reports has no signal to wait on.
    let report = timeout(Duration::from_secs(20), async {
        loop {
            let report = client
                .call(&GetUsageReport { refresh: true })
                .await
                .unwrap();
            if (report.month.total.cost_usd - 4.75).abs() < 1e-9 {
                return report;
            }
            sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("the sessions were kept");
    for period in [&report.today, &report.week, &report.month] {
        assert!((period.total.cost_usd - 4.75).abs() < 1e-9);
        assert_eq!(period.tokens, 4_500);
        assert_eq!(period.projects.len(), 1);
        assert_eq!(period.projects[0].project, "widget");
        let agents: Vec<&str> = period.harnesses.iter().map(|h| h.agent.as_str()).collect();
        assert_eq!(agents, ["claude", "flsh", "codex"]);
        assert_eq!(period.harnesses[0].models[0].model, "claude-model");
    }

    // Claude Code and Codex that could not tell count as subscriptions:
    // only flsh's metered dollars are spent.
    let bill = client
        .call(&GetSpendReport { refresh: false })
        .await
        .unwrap();
    for period in [&bill.today, &bill.week, &bill.month] {
        assert!((period.cost_usd - 1.25).abs() < 1e-9);
        assert_eq!(period.metered_tokens, 1_500);
        assert_eq!(period.subscription_tokens, 3_000);
        let shown: Vec<(&str, bool, Option<f64>)> = period
            .harnesses
            .iter()
            .map(|h| (h.agent.as_str(), h.subscription, h.cost_usd))
            .collect();
        assert_eq!(
            shown,
            [
                ("flsh", false, Some(1.25)),
                ("claude", true, None),
                ("codex", true, None)
            ]
        );
        assert_eq!(period.harnesses[1].models[0].model, "claude-model");
        let widget = &period.projects[0];
        assert!((widget.cost_usd - 1.25).abs() < 1e-9);
        assert_eq!(widget.subscription.len(), 2);
    }

    for node in &center.nodes {
        node.abort();
    }
    center.shutdown().await;
}

/// A node whose working link keeps dropping comes back each time within
/// seconds: a welcome resets the wait, so the drops do not add up to a long
/// one (they did to 8s by the fourth).
#[tokio::test]
async fn a_node_whose_link_keeps_dropping_comes_back_within_seconds() {
    let server = server().await;
    let key = linked_key(&server, "build box").await;
    let id = key.public().to_string();
    let node = endpoint(key).await;
    let addr = server.addr.clone();
    let stays = tokio::spawn(async move {
        beep_node::link::stay(&node, addr, "build box", &Offers::default()).await
    });
    let mut current = online(&server.app, &id).await;
    for _ in 0..4 {
        current.connection.close(VarInt::from_u32(0), b"cut off");
        current = timeout(
            Duration::from_secs(3),
            online_after(&server.app, &id, Some(&current)),
        )
        .await
        .expect("the node came back within seconds");
    }
    stays.abort();
    server.router.shutdown().await.unwrap();
}

/// A node that connects again before the server saw its old link drop
/// takes over at once, and the old link's teardown does not mark it
/// offline.
#[tokio::test]
async fn a_reconnect_replaces_the_stale_link_and_stays_online() {
    let server = server().await;
    let key = linked_key(&server, "build box").await;
    let id = key.public().to_string();
    let connect = |node: Endpoint| {
        let addr = server.addr.clone();
        tokio::spawn(async move {
            beep_node::link::connect(&node, addr, "build box", &Offers::default()).await
        })
    };
    let first = connect(endpoint(key.clone()).await);
    let stale = online(&server.app, &id).await;
    // The node starts again on a new endpoint.
    let second = connect(endpoint(key).await);
    let fresh = online_after(&server.app, &id, Some(&stale)).await;
    // The old link was closed as replaced, and the node heard it.
    let replaced = timeout(Duration::from_secs(10), first)
        .await
        .expect("the old link ended")
        .unwrap()
        .unwrap_err();
    assert!(format!("{replaced:#}").contains("replaced"), "{replaced:#}");
    let now = server.app.nodes.online(&id).expect("still online");
    assert_eq!(now.connection.stable_id(), fresh.connection.stable_id());
    assert!(now.connection.close_reason().is_none());
    second.abort();
    server.router.shutdown().await.unwrap();
}
