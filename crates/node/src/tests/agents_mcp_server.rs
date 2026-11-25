use super::*;
use std::sync::Mutex;
use tokio::io::{AsyncBufReadExt, BufReader, duplex};

/// A backend that answers every call, and a user who answers with `answer`.
struct Fake {
    calls: Mutex<Vec<Request>>,
    asked: Mutex<Vec<String>>,
    answer: &'static str,
    /// The session is a reviewer.
    reviewer: bool,
    /// The session's folder, when it gets the media tools.
    media: Option<std::path::PathBuf>,
    /// The mesh has no other machine.
    alone: bool,
    /// The session is the home assistant.
    assistant: bool,
    /// The session works in a team project.
    team: bool,
}

impl Fake {
    fn new(answer: &'static str) -> Self {
        Self {
            calls: Mutex::default(),
            asked: Mutex::default(),
            answer,
            reviewer: false,
            media: None,
            alone: false,
            assistant: false,
            team: false,
        }
    }
}

impl Backend for Fake {
    async fn call(&self, request: &Request) -> Result<Reply> {
        self.calls.lock().unwrap().push(request.clone());
        Ok(match (request.tool, request.approved) {
            (Tool::RunOn, _) => Reply::Done {
                text: "Exit code 0".into(),
                failed: false,
            },
            (Tool::ListMachines, _) => Reply::Done {
                text: "- build-box: online".into(),
                failed: false,
            },
            _ => Reply::Done {
                text: "build-box is offline.".into(),
                failed: true,
            },
        })
    }

    async fn device(&self, _session: &SessionId, args: Value) -> Called {
        let mut called = Called::ok(format!("{} on build-box: 2 x 4 pixels.", args["device"]));
        called.image = Some(beep_proto::node::device::Frame {
            jpeg: vec![1, 2, 3],
            width: 2,
            height: 4,
        });
        called
    }

    async fn read_transcript(&self, args: Value) -> Result<Reply> {
        Ok(super::super::reply::ok(args.to_string()))
    }

    async fn ask(&self, session: &SessionId, question: Asking) -> Result<Answered> {
        assert_eq!(*session, "a3f9c1");
        self.asked.lock().unwrap().push(question.text.clone());
        Ok(question
            .options
            .into_iter()
            .find(|option| option.id == self.answer)
            .map_or_else(|| Answered::Said(self.answer.into()), Answered::Picked))
    }

    fn reviewer(&self, session: &SessionId) -> bool {
        assert_eq!(*session, "a3f9c1");
        self.reviewer
    }

    fn assistant(&self, session: &SessionId) -> bool {
        assert_eq!(*session, "a3f9c1");
        self.assistant
    }

    fn alone(&self) -> bool {
        self.alone
    }

    fn team(&self, session: &SessionId) -> bool {
        assert_eq!(*session, "a3f9c1");
        self.team
    }

    fn media(&self, _session: &SessionId) -> Option<Media> {
        self.media.clone().map(Media::new)
    }
}

/// Sends `messages` to a server over a pipe and gives every line it
/// wrote, by id.
async fn exchange(fake: &Fake, messages: &[Value]) -> Vec<Value> {
    let (client, server) = duplex(64 * 1024);
    let (server_read, server_write) = tokio::io::split(server);
    let (client_read, mut client_write) = tokio::io::split(client);
    let mut input = String::new();

    for message in messages {
        input.push_str(&format!("{message}\n"));
    }

    client_write.write_all(input.as_bytes()).await.unwrap();
    client_write.shutdown().await.unwrap();
    serve(fake, &SessionId::from("a3f9c1"), BufReader::new(server_read), server_write)
        .await
        .unwrap();
    let mut out = Vec::new();
    let mut lines = BufReader::new(client_read).lines();

    while let Some(line) = lines.next_line().await.unwrap() {
        out.push(serde_json::from_str::<Value>(&line).unwrap());
    }

    out.sort_by_key(|message| message["id"].as_i64());
    out
}

fn request(id: i64, method: &str, params: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params })
}

/// Every tool a session gets without API keys, in order.
const ALL: [&str; 35] = [
    "list_machines",
    "list_projects",
    "harness_info",
    "remote_run",
    "remote_run_status",
    "remote_run_cancel",
    "sync_project",
    "send_files",
    "device_list",
    "device_boot",
    "device_shutdown",
    "device_show",
    "device_screenshot",
    "device_tap",
    "device_swipe",
    "device_type",
    "device_press",
    "device_logs",
    "device_ui",
    "device_install",
    "device_launch",
    "device_reset",
    "start_agent",
    "agent_status",
    "send_to_agent",
    "stop_agent",
    "list_agents",
    "read_session_transcript",
    "review",
    "ask_user",
    "beep_status",
    "beep_logs",
    "beep_metrics",
    "beep_report",
    "team_join",
];

fn text(answer: &Value) -> &str {
    answer["result"]["content"][0]["text"].as_str().unwrap()
}

#[tokio::test]
async fn it_lists_the_tools_and_answers_a_call() {
    let fake = Fake::new("allow");
    let out = exchange(
        &fake,
        &[
            request(1, "initialize", json!({ "protocolVersion": "2025-03-26" })),
            json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
            request(2, "tools/list", json!({})),
            request(
                3,
                "tools/call",
                json!({ "name": "list_machines", "arguments": {} }),
            ),
            request(
                4,
                "tools/call",
                json!({ "name": "remote_run_status", "arguments": { "id": "task:1@build-box" } }),
            ),
            request(5, "nothing/here", json!({})),
        ],
    )
    .await;
    assert_eq!(out.len(), 5, "{out:?}");
    assert_eq!(out[0]["result"]["protocolVersion"], "2025-03-26");
    assert_eq!(out[0]["result"]["serverInfo"]["name"], "beep");
    let names: Vec<&str> = out[1]["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ALL);
    let hints = |name: &str| {
        let tools = out[1]["result"]["tools"].as_array().unwrap();
        let tool = tools.iter().find(|tool| tool["name"] == name).unwrap();
        tool["annotations"].clone()
    };
    // Codex asks unless a tool reads only, or is neither destructive nor open world.
    assert_eq!(hints("list_machines")["readOnlyHint"], true);
    assert_eq!(hints("remote_run")["destructiveHint"], false);
    assert_eq!(hints("remote_run")["openWorldHint"], false);
    assert_eq!(text(&out[2]), "- build-box: online");
    assert_eq!(out[2]["result"]["isError"], false);
    assert_eq!(text(&out[3]), "build-box is offline.");
    assert_eq!(out[4]["error"]["code"], -32601);
    let calls = fake.calls.lock().unwrap();
    assert_eq!(calls[0].tool, Tool::ListMachines);
    assert_eq!(calls[0].session, "a3f9c1");
    assert_eq!(calls[1].tool, Tool::JobStatus);
    assert_eq!(calls[1].args, r#"{"id":"task:1@build-box"}"#);
    assert!(fake.asked.lock().unwrap().is_empty(), "nothing acted");
}

#[tokio::test]
async fn transcript_calls_use_the_sessions_backend_without_acting_approval() {
    let fake = Fake::new("deny");
    let args = json!({"machine":"build-box", "session":"session-a", "before":20});
    let out = exchange(
        &fake,
        &[request(
            1,
            "tools/call",
            json!({"name": "read_session_transcript", "arguments": args}),
        )],
    )
    .await;
    assert_eq!(out[0]["result"]["isError"], false);
    assert_eq!(serde_json::from_str::<Value>(text(&out[0])).unwrap(), args);
    assert!(fake.calls.lock().unwrap().is_empty());
    assert!(fake.asked.lock().unwrap().is_empty());
}

#[tokio::test]
async fn machine_log_and_run_on_are_no_tools() {
    let fake = Fake::new("allow");

    for name in ["machine_log", "run_on"] {
        let call = json!({ "name": name, "arguments": { "machine": "build-box" } });
        let out = exchange(&fake, &[request(1, "tools/call", call)]).await;
        assert_eq!(out[0]["result"]["isError"], true);
        assert_eq!(text(&out[0]), format!("There is no tool {name}."));
    }

    assert!(fake.calls.lock().unwrap().is_empty(), "nothing was sent");
}

#[tokio::test]
async fn ask_user_gives_the_users_answer() {
    let fake = Fake::new("2");
    let out = exchange(
        &fake,
        &[request(
            1,
            "tools/call",
            json!({ "name": "ask_user", "arguments": { "question": "Which branch?", "options": ["main", "dev"] } }),
        )],
    )
    .await;
    assert_eq!(text(&out[0]), "The user picked: dev");
    assert!(
        fake.calls.lock().unwrap().is_empty(),
        "the server is not asked"
    );
    let words = Fake::new("the release branch");
    let out = exchange(
        &words,
        &[request(
            1,
            "tools/call",
            json!({ "name": "ask_user", "arguments": { "question": "Which branch?" } }),
        )],
    )
    .await;
    assert_eq!(text(&out[0]), "The user answered: the release branch");
}

#[tokio::test]
async fn only_a_reviewer_is_offered_submit_review() {
    let names = |out: &[Value]| -> Vec<String> {
        out[0]["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|tool| tool["name"].as_str().unwrap().to_owned())
            .collect()
    };
    let submit = request(
        2,
        "tools/call",
        json!({ "name": "submit_review", "arguments": { "verdict": "approve", "summary": "Fine.", "findings": [] } }),
    );
    let worker = Fake::new("allow");
    let out = exchange(
        &worker,
        &[request(1, "tools/list", json!({})), submit.clone()],
    )
    .await;
    let offered = names(&out);
    assert!(offered.contains(&"review".to_owned()), "{offered:?}");
    assert!(
        !offered.contains(&"submit_review".to_owned()),
        "{offered:?}"
    );
    assert_eq!(text(&out[1]), "There is no tool submit_review.");
    assert!(worker.calls.lock().unwrap().is_empty(), "nothing was sent");

    let reviewer = Fake {
        reviewer: true,
        ..Fake::new("allow")
    };
    let out = exchange(&reviewer, &[request(1, "tools/list", json!({})), submit]).await;
    let offered = names(&out);
    assert!(offered.contains(&"submit_review".to_owned()), "{offered:?}");
    assert!(!offered.contains(&"review".to_owned()), "{offered:?}");
    let calls = reviewer.calls.lock().unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].tool, Tool::SubmitReview);
}

#[tokio::test]
async fn a_device_screenshot_comes_as_image_content() {
    let fake = Fake::new("1");
    let out = exchange(
        &fake,
        &[request(
            1,
            "tools/call",
            json!({ "name": "device_screenshot", "arguments": { "machine": "build-box", "device": "Pixel" } }),
        )],
    )
    .await;
    let content = &out[0]["result"]["content"];
    assert_eq!(content[0]["text"], "\"Pixel\" on build-box: 2 x 4 pixels.");
    assert_eq!(content[1]["type"], "image");
    assert_eq!(content[1]["mimeType"], "image/jpeg");
    assert_eq!(content[1]["data"], "AQID");
    assert!(fake.calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_saved_key_offers_its_media_tools_and_the_node_runs_them() {
    use crate::machine::spawn_env::{self, TEST_LOCK};
    use beep_proto::{SecretText, node::EnvSecret};

    let _env = TEST_LOCK.lock().await;
    let folder = tempfile::tempdir().unwrap();
    let mut fake = Fake::new("allow");
    fake.media = Some(folder.path().to_owned());
    spawn_env::set_vars(vec![
        EnvSecret {
            name: "OPENROUTER_API_KEY".into(),
            value: SecretText::new("sk-or-test"),
        },
        EnvSecret {
            name: "MESHY_API_KEY".into(),
            value: SecretText::new("msy-test"),
        },
    ]);
    let decision = json!({ "name": "decision", "arguments": {} });
    let out = exchange(
        &fake,
        &[
            request(1, "tools/list", json!({})),
            request(2, "tools/call", decision),
            request(3, "initialize", json!({})),
        ],
    )
    .await;
    spawn_env::set_vars(Vec::new());
    let brief = out[2]["result"]["instructions"].as_str().unwrap();
    assert!(
        brief.contains("API keys") && brief.contains("decision"),
        "{brief}"
    );

    let names: Vec<&str> = out[0]["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect();
    for tool in [
        "remote_run",
        "generate_image",
        "generate_video",
        "tts",
        "decision",
        "list_models",
        "generate_3d",
        "animate_3d",
        "remesh_3d",
    ] {
        assert!(names.contains(&tool), "{tool} missing from {names:?}");
    }
    for hidden in ["image", "model_3d"] {
        assert!(!names.contains(&hidden), "{names:?}");
    }
    assert_eq!(out[1]["result"]["isError"], true, "{out:?}");
    assert!(fake.calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn without_keys_there_are_no_media_tools() {
    use crate::machine::spawn_env::TEST_LOCK;

    let _env = TEST_LOCK.lock().await;
    let folder = tempfile::tempdir().unwrap();
    let mut fake = Fake::new("allow");
    fake.media = Some(folder.path().to_owned());
    let out = exchange(&fake, &[request(1, "tools/list", json!({}))]).await;

    let listed = out[0]["result"]["tools"].as_array().unwrap();
    assert!(
        listed.iter().all(|tool| tool["name"] != "decision"),
        "{listed:?}"
    );
}

#[tokio::test]
async fn grouped_and_internal_names_are_no_tools() {
    let fake = Fake::new("allow");

    for name in ["machines", "agent", "device", "beep", "plugins", "job_status", "run_on", "machine_log"] {
        let result = call(&fake, &SessionId::from("a3f9c1"), name, json!({ "action": "list" })).await;
        assert!(result.failed && result.text == format!("There is no tool {name}."), "{result:?}");
    }

    assert!(fake.calls.lock().unwrap().is_empty(), "nothing was sent");
}

#[tokio::test]
async fn without_other_machines_run_and_files_are_left_out() {
    let fake = Fake {
        alone: true,
        ..Fake::new("allow")
    };
    let out = exchange(&fake, &[request(1, "tools/list", json!({}))]).await;
    let names: Vec<&str> = out[0]["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect();
    let remote = [
        "remote_run",
        "remote_run_status",
        "remote_run_cancel",
        "sync_project",
        "send_files",
    ];
    let expected: Vec<&str> = ALL.into_iter().filter(|name| !remote.contains(name)).collect();
    assert_eq!(names, expected);
}

fn names(tools: &Value) -> Vec<&str> {
    tools
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect()
}

/// The project tools, which only the home assistant gets.
const PROJECT_TOOLS: [&str; 5] = [
    "projects_hosts",
    "projects_create",
    "projects_clone",
    "projects_found",
    "projects_import",
];

/// The thread tools, which only the home assistant gets, each with the
/// agent tool it calls.
const THREAD_TOOLS: [(&str, Tool); 5] = [
    ("start_thread", Tool::StartAgent),
    ("thread_status", Tool::AgentStatus),
    ("send_to_thread", Tool::SendToAgent),
    ("stop_thread", Tool::StopAgent),
    ("list_threads", Tool::ListAgents),
];

#[tokio::test]
async fn thread_tools_call_the_agent_tools_for_the_assistant_alone() {
    let session = SessionId::from("a3f9c1");
    let args = |name: &str| match name {
        "start_thread" => json!({ "title": "Pricing", "prompt": "work" }),
        "send_to_thread" => json!({ "id": "b7e2d4", "text": "and the EU?" }),
        "list_threads" => json!({}),
        _ => json!({ "id": "b7e2d4" }),
    };
    let other = Fake::new("allow");

    for (name, _) in THREAD_TOOLS {
        let result = call(&other, &session, name, args(name)).await;
        assert!(
            result.failed && result.text == format!("There is no tool {name}."),
            "{result:?}"
        );
    }

    assert!(other.calls.lock().unwrap().is_empty(), "nothing is forwarded");
    let assistant = Fake {
        assistant: true,
        ..Fake::new("allow")
    };

    for (name, _) in THREAD_TOOLS {
        call(&assistant, &session, name, args(name)).await;
    }

    {
        let calls = assistant.calls.lock().unwrap();
        let called: Vec<Tool> = calls.iter().map(|call| call.tool).collect();
        assert_eq!(called, THREAD_TOOLS.map(|(_, tool)| tool));
        assert_eq!(calls[0].args, args("start_thread").to_string());
    }

    // A thread is the chat's: it has a name, and nothing says where it runs.
    for refused in [
        json!({ "prompt": "work" }),
        json!({ "title": " ", "prompt": "work" }),
        json!({ "title": "Pricing", "prompt": "work", "machine": "build-box" }),
        json!({ "title": "Pricing", "prompt": "work", "cwd": "/srv/project" }),
    ] {
        let result = call(&assistant, &session, "start_thread", refused).await;
        assert!(result.failed, "{result:?}");
    }

    assert_eq!(assistant.calls.lock().unwrap().len(), THREAD_TOOLS.len());
}

#[test]
fn assistant_session_lists_the_project_tools_in_place_of_review() {
    let listed = catalog::tools(false, false, true);
    let assistant = names(&listed);
    let expected: Vec<&str> = ALL
        .into_iter()
        .filter(|name| *name != "review")
        .chain(THREAD_TOOLS.map(|(name, _)| name))
        .chain(PROJECT_TOOLS)
        .collect();
    assert_eq!(assistant, expected);
    assert_eq!(names(&catalog::tools(false, false, false)), ALL);
}

#[tokio::test]
async fn initialize_omits_instructions_without_media_tools() {
    let initialize = || [request(1, "initialize", json!({}))];
    let assistant = Fake {
        assistant: true,
        ..Fake::new("allow")
    };
    let out = exchange(&assistant, &initialize()).await;
    assert!(
        out[0]["result"].get("instructions").is_none(),
        "{}",
        out[0]["result"]
    );

    let out = exchange(&Fake::new("allow"), &initialize()).await;
    assert!(
        out[0]["result"].get("instructions").is_none(),
        "{}",
        out[0]["result"]
    );
}

#[tokio::test]
async fn project_tools_are_refused_for_other_sessions_before_validation() {
    let fake = Fake::new("allow");
    // Invalid arguments too: the refusal comes before the schema is checked.
    for args in [json!({ "name": "notes", "machines": ["build-box"] }), json!({ "unknown": 1 })] {
        let result = call(&fake, &SessionId::from("a3f9c1"), "projects_create", args).await;
        assert!(
            result.failed && result.text == "There is no tool projects_create.",
            "{result:?}"
        );
    }
    let result = call(&fake, &SessionId::from("a3f9c1"), "projects", json!({ "action": "hosts" })).await;
    assert!(result.failed, "{result:?}");
    assert!(fake.calls.lock().unwrap().is_empty(), "nothing is forwarded");
}

#[tokio::test]
async fn project_tools_reach_the_desktop_as_projects_with_their_action() {
    let assistant = Fake {
        assistant: true,
        ..Fake::new("allow")
    };
    let args = json!({ "name": "notes", "machines": ["build-box"] });
    call(&assistant, &SessionId::from("a3f9c1"), "projects_create", args).await;
    let result = call(&assistant, &SessionId::from("a3f9c1"), "projects_hosts", json!({ "extra": 1 })).await;
    assert!(result.failed && result.text.contains("extra"), "{result:?}");

    let calls = assistant.calls.lock().unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].tool, Tool::Projects);
    let sent: Value = serde_json::from_str(&calls[0].args).unwrap();
    assert_eq!(
        sent,
        json!({ "action": "create", "name": "notes", "machines": ["build-box"] })
    );
}

#[tokio::test]
async fn team_join_tool_validates_and_routes_for_any_session() {
    let fake = Fake::new("allow");
    let session = SessionId::from("a3f9c1");

    let missing = call(&fake, &session, "team_join", json!({})).await;
    assert!(missing.failed, "{missing:?}");
    let extra = call(&fake, &session, "team_join", json!({ "ticket": "beepteam1abc", "extra": 1 })).await;
    assert!(extra.failed && extra.text.contains("extra"), "{extra:?}");

    let args = json!({ "ticket": "beepteam1abc", "machine": "build-box", "folder": "~/work" });
    call(&fake, &session, "team_join", args.clone()).await;

    let calls = fake.calls.lock().unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].tool, Tool::TeamJoin);
    assert_eq!(serde_json::from_str::<Value>(&calls[0].args).unwrap(), args);
}

/// The tools agents get without API keys stay within a context budget, at
/// about four bytes a token.
#[test]
fn the_tool_definitions_fit_the_budget() {
    const TOKENS: usize = 4_350;
    // The home assistant's list has the thread and project tools in place
    // of review.
    const ASSISTANT_TOKENS: usize = 5_050;

    for (assistant, budget) in [(false, TOKENS), (true, ASSISTANT_TOKENS)] {
        let listed = catalog::tools(false, false, assistant).to_string();
        let tokens = listed.len() / 4;
        assert!(tokens <= budget, "{tokens} tokens, assistant {assistant}");
    }
    let full = crate::agents::brief::Context {
        machine: Some("workstation".into()),
        project: Some("project".into()),
        multi_machine: true,
        rust_or_c: true,
    };
    let brief = (crate::agents::brief::environment(&full).len()
        + crate::agents::brief::ASSISTANT.len()
        + crate::agents::brief::chat(true).len())
        / 4;
    assert!(brief <= 1_000, "the assistant's brief is {brief} tokens");
}

#[test]
fn catalog_schemas_preserve_exact_serialized_json_for_every_mode() {
    use sha2::{Digest, Sha256};

    let preserved = serde_json::to_vec(&json!({ "b": 1, "a": 2 })).unwrap() == br#"{"b":1,"a":2}"#;
    let cases = [
        (
            (false, false, false),
            17_117,
            "616cd6b7ddd490137b9f468a269eb9212b04da60d6cdb0a61c8487f8d9d18f95",
            "ac821a65b22352002e0a1ab9f2477aaf0170aaffd00ffcf9429ab462349434c5",
        ),
        (
            (false, false, true),
            20_147,
            "2cade9f718b3c27e8d2cfa1c95390a532b673b67f35506e7ba3c17ed82e4b465",
            "ea4794b4612ba7db672ace97c272d0b909b5e77eb853a799bc11c1005547f07b",
        ),
        (
            (false, true, false),
            14_651,
            "0ebd764661fa195801cd819a8123c055ab657227a6cde81c8269a8473e3fe34e",
            "b463e280722b1c48b8fdc4df7014cc195a82d87005af3c6d44858136a0d54fe7",
        ),
        (
            (false, true, true),
            17_681,
            "54e59adf0da5cee5c258861901ea55576f9c4fbbc79a01b4303f2cc89eef0478",
            "d3a9e4b3bfb29a6b4e66fc2064e2502666087a4c2d5af6c70cfe890084da044e",
        ),
        (
            (true, false, false),
            17_087,
            "a7b819fe9dab28093c146f9970b210b374a38b64f9fd3621d9de5e98f7a974bb",
            "3490669ec2acb1cedc99165cfd37e6232cb1e02012ceb1ea3e9566929eba850d",
        ),
        (
            (true, false, true),
            21_187,
            "f0009879c2bb38081e8fad72bb041ebdee1db8715ff19d3ca4be6021c2ba27b0",
            "9409380fd81bda0c1788acaa5a6abb0a34eb40ec0d159762563285d6a0ecae14",
        ),
        (
            (true, true, false),
            14_621,
            "17e5ab12df7c8902c90b40bbd1256730f241a98325fe74d973969e46052cda73",
            "f62d9ecf0cf260bcc22d9a1d8fd709b65ea50cb45d264b04a52eb2ba6a1ec18b",
        ),
        (
            (true, true, true),
            18_721,
            "c522a8092ce7bb3e0c03b58028b7b4ebb71f2cbe8977266add1306137af9c8df",
            "8ebb620640a91bf618c9defdff3ddb7fcedbe8b04a61efdefdc4e9c34dff1d21",
        ),
    ];

    for ((reviewer, alone, assistant), len, ordered_sha, sorted_sha) in cases {
        let rendered = catalog::tools(reviewer, alone, assistant).to_string();
        let digest = hex::encode(Sha256::digest(rendered.as_bytes()));
        let expected = if preserved { ordered_sha } else { sorted_sha };
        assert_eq!(rendered.len(), len, "mode ({reviewer}, {alone}, {assistant})");
        assert_eq!(digest, expected, "mode ({reviewer}, {alone}, {assistant})");
    }
}

#[tokio::test]
async fn a_meshy_key_offers_the_3d_tools_and_their_models() {
    use crate::machine::spawn_env::{self, TEST_LOCK};
    use beep_proto::{SecretText, node::EnvSecret};

    let _env = TEST_LOCK.lock().await;
    let folder = tempfile::tempdir().unwrap();
    let mut fake = Fake::new("allow");
    fake.media = Some(folder.path().to_owned());
    spawn_env::set_vars(vec![EnvSecret {
        name: "MESHY_API_KEY".into(),
        value: SecretText::new("msy-test"),
    }]);
    let call = |id, name: &str, args: Value| {
        request(id, "tools/call", json!({ "name": name, "arguments": args }))
    };
    let out = exchange(
        &fake,
        &[
            request(1, "tools/list", json!({})),
            call(2, "list_models", json!({ "category": "3d" })),
            call(3, "rig_3d", json!({ "glb": "hero.glb", "prompt": "a chair" })),
        ],
    )
    .await;
    spawn_env::set_vars(Vec::new());

    let tools = out[0]["result"]["tools"].as_array().unwrap();
    let names: Vec<&str> = tools.iter().map(|tool| tool["name"].as_str().unwrap()).collect();
    for name in ["generate_3d", "rig_3d", "list_models"] {
        assert!(names.contains(&name), "{name} missing from {names:?}");
    }
    assert!(!names.contains(&"generate_image"), "{names:?}");
    let rig = tools.iter().find(|tool| tool["name"] == "rig_3d").unwrap();
    assert_eq!(rig["inputSchema"]["required"], json!(["glb"]));
    assert!(text(&out[1]).contains("meshy"), "{}", text(&out[1]));
    assert_eq!(out[2]["result"]["isError"], true, "{}", text(&out[2]));
    assert!(fake.calls.lock().unwrap().is_empty());
}

/// Every tool takes only its own arguments, checked before routing.
#[tokio::test]
async fn every_tool_rejects_unknown_arguments() {
    let fake = Fake {
        assistant: true,
        ..Fake::new("allow")
    };
    // The assistant's list has the project tools, the other one has review.
    let listed = [catalog::tools(false, false, true), catalog::tools(false, false, false)];

    for tool in listed.iter().flat_map(|tools| tools.as_array().unwrap()) {
        let name = tool["name"].as_str().unwrap();
        let schema = &tool["inputSchema"];
        assert_eq!(schema["additionalProperties"], false, "{name}");
        let mut args = json!({ "unknown_argument": true });
        for key in schema["required"].as_array().unwrap() {
            let key = key.as_str().unwrap();
            args[key] = match schema["properties"][key]["type"].as_str() {
                Some("array") => json!(["value"]),
                Some("integer" | "number") => json!(1),
                _ => json!("value"),
            };
        }
        let result = call(&fake, &SessionId::from("a3f9c1"), name, args).await;
        assert!(result.failed && result.text.contains("unknown_argument"), "{name}: {result:?}");
    }

    let result = call(&fake, &SessionId::from("a3f9c1"), "device_tap", json!({ "machine": "build-box", "device": "phone", "x": "1", "y": 2 })).await;
    assert!(result.failed && result.text.contains('x'), "{result:?}");
    assert!(fake.calls.lock().unwrap().is_empty(), "invalid calls never reach a peer");
}

#[tokio::test]
async fn review_status_rejects_start_options() {
    let fake = Fake::new("allow");

    for field in ["base", "focus", "harness", "model", "unknown_argument"] {
        let mut args = json!({"id": "review-id"});
        args[field] = json!("value");
        let result = call(&fake, &SessionId::from("a3f9c1"), "review", args).await;
        assert!(result.failed && result.text.contains(field), "{result:?}");
    }

    let result = call(&fake, &SessionId::from("a3f9c1"), "review", json!({"id": 7})).await;
    assert!(result.failed && result.text.contains("id"));
    assert!(fake.calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn submit_review_rejects_unknown_and_mistyped_fields_including_findings() {
    let fake = Fake {
        reviewer: true,
        ..Fake::new("allow")
    };
    let valid = json!({"verdict":"changes", "summary":"Fix this.", "findings":[{
        "file":"src/lib.rs", "line":1, "severity":"high", "summary":"A bug", "scenario":"An input fails"
    }]});
    assert!(catalog::validate("submit_review", &valid).is_ok());

    for (nested, field) in [
        (false, "unknown_argument"),
        (false, "summary"),
        (true, "unknown_argument"),
        (true, "file"),
        (true, "line"),
    ] {
        let mut args = valid.clone();
        let object = if nested {
            &mut args["findings"][0]
        } else {
            &mut args
        };
        object[field] = json!({"wrong":"type"});
        let result = call(&fake, &SessionId::from("a3f9c1"), "submit_review", args).await;
        assert!(result.failed && result.text.contains(field), "{result:?}");
    }

    assert!(fake.calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_saved_key_tells_the_agent_its_tools_changed() {
    use crate::machine::spawn_env::{self, TEST_LOCK};
    use beep_proto::{SecretText, node::EnvSecret};

    let _env = TEST_LOCK.lock().await;
    let folder = tempfile::tempdir().unwrap();
    let mut fake = Fake::new("allow");
    fake.media = Some(folder.path().to_owned());
    let (client, server) = duplex(64 * 1024);
    let (server_read, server_write) = tokio::io::split(server);
    let (client_read, mut client_write) = tokio::io::split(client);
    let mut lines = BufReader::new(client_read).lines();
    let mut next = async || serde_json::from_str::<Value>(&lines.next_line().await.unwrap().unwrap()).unwrap();
    let names = |answer: &Value| -> Vec<String> {
        answer["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|tool| tool["name"].as_str().unwrap().to_owned())
            .collect()
    };
    let list = |id: i64| format!("{}\n", request(id, "tools/list", json!({})));

    let served = async {
        serve(&fake, &SessionId::from("a3f9c1"), BufReader::new(server_read), server_write)
            .await
            .unwrap();
    };
    let client = async {
        client_write.write_all(list(1).as_bytes()).await.unwrap();
        let before = next().await;
        assert!(!names(&before).contains(&"generate_image".to_owned()));

        spawn_env::set_vars(vec![EnvSecret {
            name: "OPENROUTER_API_KEY".into(),
            value: SecretText::new("sk-or-test"),
        }]);
        assert_eq!(next().await["method"], "notifications/tools/list_changed");

        client_write.write_all(list(2).as_bytes()).await.unwrap();
        let after = next().await;
        assert!(names(&after).contains(&"generate_image".to_owned()), "{after}");
        client_write.shutdown().await.unwrap();
    };
    tokio::join!(served, client);
    spawn_env::set_vars(Vec::new());
}

#[tokio::test]
async fn only_sessions_in_a_team_project_get_the_team_tools() {
    let ask = || {
        let args = json!({ "name": "team_ask", "arguments": { "to": "bob", "text": "Can I take #52?" } });
        [request(1, "tools/list", json!({})), request(2, "tools/call", args)]
    };
    let listed = |out: &[Value]| -> Vec<String> {
        let tools = out[0]["result"]["tools"].as_array().unwrap();
        tools.iter().map(|tool| tool["name"].as_str().unwrap().to_owned()).collect()
    };

    let alone = Fake::new("allow");
    let out = exchange(&alone, &ask()).await;
    assert!(!listed(&out).iter().any(|name| TEAM.contains(&name.as_str())));
    assert_eq!(out[1]["result"]["isError"], true);
    assert!(alone.calls.lock().unwrap().is_empty());

    let teamed = Fake {
        team: true,
        ..Fake::new("allow")
    };
    let out = exchange(&teamed, &ask()).await;
    let names = listed(&out);
    assert!(names.ends_with(&["team".to_owned(), "team_ask".to_owned()]));
    let calls = teamed.calls.lock().unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].tool, Tool::TeamAsk);
}
