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

    async fn device(&self, args: Value) -> Called {
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

    async fn ask(&self, session: &str, question: Asking) -> Result<Answered> {
        assert_eq!(session, "a3f9c1");
        self.asked.lock().unwrap().push(question.text.clone());
        Ok(question
            .options
            .into_iter()
            .find(|option| option.id == self.answer)
            .map_or_else(|| Answered::Said(self.answer.into()), Answered::Picked))
    }

    fn reviewer(&self, session: &str) -> bool {
        assert_eq!(session, "a3f9c1");
        self.reviewer
    }

    fn alone(&self) -> bool {
        self.alone
    }

    fn media(&self, _session: &str) -> Option<Media> {
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
    serve(fake, "a3f9c1", BufReader::new(server_read), server_write)
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
                json!({ "name": "machines", "arguments": { "action": "list" } }),
            ),
            request(
                4,
                "tools/call",
                json!({ "name": "run", "arguments": { "action": "status", "id": "task:1@build-box" } }),
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
    assert_eq!(
        names,
        [
            "machines", "run", "files", "device", "agent", "review", "ask_user", "beep"
        ]
    );
    let hints = |name: &str| {
        let tools = out[1]["result"]["tools"].as_array().unwrap();
        let tool = tools.iter().find(|tool| tool["name"] == name).unwrap();
        tool["annotations"].clone()
    };
    // Codex asks unless a tool reads only, or is neither destructive nor open world.
    assert_eq!(hints("machines")["readOnlyHint"], true);
    assert_eq!(hints("run")["destructiveHint"], false);
    assert_eq!(hints("run")["openWorldHint"], false);
    assert_eq!(text(&out[2]), "- build-box: online");
    assert_eq!(out[2]["result"]["isError"], false);
    assert_eq!(text(&out[3]), "build-box is offline.");
    assert_eq!(out[3]["result"]["isError"], true);
    assert_eq!(out[4]["error"]["code"], -32601);
    let calls = fake.calls.lock().unwrap();
    assert_eq!(calls[0].tool, Tool::ListMachines);
    assert_eq!(calls[0].session, "a3f9c1");
    assert_eq!(calls[1].tool, Tool::JobStatus);
    assert_eq!(calls[1].args, r#"{"id":"task:1@build-box"}"#, "action is taken out");
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
            json!({"name": "agent", "arguments": {"action": "transcript", "machine": "build-box", "session": "session-a", "before": 20}}),
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
            json!({ "name": "device", "arguments": { "action": "screenshot", "device": "Pixel" } }),
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
    use crate::machine::provision::spawn_env::{self, TEST_LOCK};
    use beep_proto::{SecretText, node::EnvSecret};

    let _env = TEST_LOCK.lock().await;
    let folder = tempfile::tempdir().unwrap();
    let mut fake = Fake::new("allow");
    fake.media = Some(folder.path().to_owned());
    spawn_env::set_vars(vec![EnvSecret {
        name: "OPENROUTER_API_KEY".into(),
        value: SecretText::new("sk-or-test"),
    }]);
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
    assert!(brief.contains("API keys") && brief.contains("decision"), "{brief}");

    let names: Vec<&str> = out[0]["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect();
    for tool in ["run", "generate_image", "generate_video", "tts", "decision"] {
        assert!(names.contains(&tool), "{tool} missing from {names:?}");
    }
    assert!(!names.contains(&"generate_3d"), "{names:?}");
    assert_eq!(out[1]["result"]["isError"], true, "{out:?}");
    assert!(fake.calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn without_keys_there_are_no_media_tools() {
    use crate::machine::provision::spawn_env::TEST_LOCK;

    let _env = TEST_LOCK.lock().await;
    let folder = tempfile::tempdir().unwrap();
    let mut fake = Fake::new("allow");
    fake.media = Some(folder.path().to_owned());
    let out = exchange(&fake, &[request(1, "tools/list", json!({}))]).await;

    let listed = out[0]["result"]["tools"].as_array().unwrap();
    assert!(listed.iter().all(|tool| tool["name"] != "decision"), "{listed:?}");
}

#[tokio::test]
async fn a_grouped_tool_needs_a_known_action() {
    let fake = Fake::new("allow");
    let out = exchange(
        &fake,
        &[
            request(1, "tools/call", json!({ "name": "agent", "arguments": {} })),
            request(2, "tools/call", json!({ "name": "beep", "arguments": { "action": "reboot" } })),
        ],
    )
    .await;
    assert_eq!(
        text(&out[0]),
        "Give agent an action: start, status, send, stop, list, transcript."
    );
    assert_eq!(out[1]["result"]["isError"], true);
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
    assert_eq!(
        names,
        ["machines", "device", "agent", "review", "ask_user", "beep"]
    );
}

/// The tools agents get without API keys stay within a context budget, at
/// about four bytes a token.
#[test]
fn the_tool_definitions_fit_the_budget() {
    const TOKENS: usize = 2_500;

    let listed = catalog::tools(false, false).to_string();
    let tokens = listed.len() / 4;
    assert!(tokens <= TOKENS, "{tokens} tokens");
    let brief = (super::super::BRIEF.len()) / 4;
    assert!(brief <= 500, "the brief is {brief} tokens");
}
