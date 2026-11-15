//! `beep-node claude-acp`, Claude Code's Direct driver, against a fake
//! `claude` that speaks stream-json: a client drives it as the node's
//! session does (initialize, `session/new`, the switch to bypassing
//! permissions, a prompt) and answers a tool permission and an
//! `AskUserQuestion` form, which reach the CLI as its control responses.
#![cfg(unix)]

use serde_json::{Value, json};
use std::{os::unix::fs::PermissionsExt, path::Path, process::Stdio, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines},
    process::{ChildStdin, ChildStdout, Command},
    time::timeout,
};

/// A `claude` that answers `initialize` and the mode switch, then plays one
/// turn: text, a Bash call it asks permission for, a question it asks, and
/// the result. It writes its arguments and what it reads to `$FAKE_DIR`.
const FAKE_CLAUDE: &str = r#"#!/bin/sh
printf '%s\n' "$@" > "$FAKE_DIR/args"
while [ $# -gt 0 ]; do
    [ "$1" = --append-system-prompt-file ] && cp "$2" "$FAKE_DIR/system"
    shift
done
next() { read -r line; printf '%s\n' "$line" >> "$FAKE_DIR/stdin"; }
ok() {
    id=$(printf '%s\n' "$line" | sed -nE 's/.*"request_id":"([^"]*)".*/\1/p')
    printf '{"type":"control_response","response":{"subtype":"success","request_id":"%s","response":%s}}\n' "$id" "$1"
}
ask='{"questions":[{"question":"Tea or coffee?","header":"Drink","multiSelect":false,"options":[{"label":"Tea","description":"Hot leaves"},{"label":"Coffee"}]}]}'
next
ok '{"models":[{"value":"default","displayName":"Default","description":"The usual"},{"value":"model-fast","displayName":"Fast"}],"commands":[{"name":"review","description":"Review the change","argumentHint":"<path>"}]}'
next
ok '{"mode":"bypassPermissions"}'
next
echo '{"type":"user","message":{"role":"user","content":[{"type":"text","text":"Fix the tests"}]},"parent_tool_use_id":null,"isReplay":true}'
echo '{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Looking."}},"parent_tool_use_id":null}'
echo '{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"Looking."},{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"cargo test"}}],"usage":{"input_tokens":10,"cache_read_input_tokens":90,"output_tokens":5}},"parent_tool_use_id":null}'
echo '{"type":"control_request","request_id":"c1","request":{"subtype":"can_use_tool","tool_name":"Bash","input":{"command":"cargo test"},"tool_use_id":"t1"}}'
next
echo '{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"test result: ok","is_error":false}]},"parent_tool_use_id":null}'
echo '{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"t2","name":"AskUserQuestion","input":'"$ask"'}]},"parent_tool_use_id":null}'
echo '{"type":"control_request","request_id":"c2","request":{"subtype":"can_use_tool","tool_name":"AskUserQuestion","input":'"$ask"',"tool_use_id":"t2"}}'
next
echo '{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t2","content":"Tea"}]},"parent_tool_use_id":null}'
echo '{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Tea it is."}},"parent_tool_use_id":null}'
echo '{"type":"result","subtype":"success","is_error":false,"stop_reason":"end_turn","result":"Tea it is.","total_cost_usd":0.01,"usage":{"input_tokens":10,"output_tokens":5,"cache_read_input_tokens":90},"modelUsage":{"model-a":{"contextWindow":200000}}}'
cat > /dev/null
"#;

/// The bridge's stdio, as a client sees it.
struct Client {
    stdin: ChildStdin,
    stdout: Lines<BufReader<ChildStdout>>,
    next_id: u64,
}

impl Client {
    async fn send(&mut self, message: Value) {
        let line = format!("{message}\n");
        self.stdin.write_all(line.as_bytes()).await.unwrap();
    }

    async fn read(&mut self) -> Value {
        let line = timeout(Duration::from_secs(20), self.stdout.next_line())
            .await
            .expect("the bridge answers in time")
            .unwrap()
            .expect("the bridge is still running");
        serde_json::from_str(&line).unwrap()
    }

    /// Sends `method`; gives every message before its answer, then the answer.
    async fn call(&mut self, method: &str, params: Value) -> (Vec<Value>, Value) {
        self.next_id += 1;
        let id = self.next_id;
        self.send(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}))
            .await;
        let mut before = Vec::new();

        loop {
            let message = self.read().await;
            if message["id"] == id && message.get("method").is_none() {
                return (before, message);
            }
            before.push(message);
        }
    }

    async fn answer(&mut self, request: &Value, result: Value) {
        self.send(json!({"jsonrpc": "2.0", "id": request["id"], "result": result}))
            .await;
    }
}

fn updates<'a>(messages: &'a [Value], kind: &'a str) -> impl Iterator<Item = &'a Value> {
    messages
        .iter()
        .filter(|message| message["method"] == "session/update")
        .map(|message| &message["params"]["update"])
        .filter(move |update| update["sessionUpdate"] == kind)
}

fn lines(path: &Path) -> Vec<Value> {
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

#[tokio::test]
async fn a_turn_with_a_permission_and_a_question_reaches_claude_code() {
    let dir = tempfile::tempdir().unwrap();
    let claude = dir.path().join("claude");
    std::fs::write(&claude, FAKE_CLAUDE).unwrap();
    std::fs::set_permissions(&claude, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_beep-node"))
        .args(["claude-acp", "--cli"])
        .arg(&claude)
        .args(["--", "--effort", "high"])
        .env("FAKE_DIR", dir.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .expect("the bridge starts");
    let mut client = Client {
        stdin: child.stdin.take().unwrap(),
        stdout: BufReader::new(child.stdout.take().unwrap()).lines(),
        next_id: 0,
    };

    let (_, initialized) = client
        .call(
            "initialize",
            json!({"protocolVersion": 1, "clientCapabilities": {"elicitation": {"form": {}}}}),
        )
        .await;
    assert_eq!(initialized["result"]["protocolVersion"], 1);
    assert_eq!(
        initialized["result"]["agentCapabilities"]["loadSession"],
        true
    );

    let beep = json!({"name": "beep", "command": "/srv/bin/beep-node", "args": ["mcp"], "env": []});
    let (_, created) = client
        .call(
            "session/new",
            json!({
                "cwd": dir.path(),
                "mcpServers": [beep],
                "systemPrompt": {"mode": "append", "content": [{"type": "text", "text": "Work in small commits."}]},
            }),
        )
        .await;
    let session = created["result"]["sessionId"].as_str().unwrap().to_owned();
    assert_eq!(created["result"]["modes"]["currentModeId"], "default");
    let model = &created["result"]["configOptions"][0];
    assert_eq!(model["category"], "model");
    assert_eq!(model["currentValue"], "default");
    assert_eq!(model["options"][1]["value"], "model-fast");

    let (_, mode) = client
        .call(
            "session/set_mode",
            json!({"sessionId": session, "modeId": "bypassPermissions"}),
        )
        .await;
    assert_eq!(mode["result"], json!({}));

    let prompt =
        json!({"sessionId": session, "prompt": [{"type": "text", "text": "Fix the tests"}]});
    client.next_id += 1;
    let prompt_id = client.next_id;
    client
        .send(json!({"jsonrpc": "2.0", "id": prompt_id, "method": "session/prompt", "params": prompt}))
        .await;
    let mut seen = Vec::new();
    let done = loop {
        let message = client.read().await;
        match message["method"].as_str() {
            Some("session/request_permission") => {
                assert_eq!(message["params"]["toolCall"]["title"], "cargo test");
                assert_eq!(message["params"]["options"][0]["optionId"], "allow-once");
                let allow = json!({"outcome": {"outcome": "selected", "optionId": "allow-once"}});
                client.answer(&message, allow).await;
            }
            Some("elicitation/create") => {
                let params = &message["params"];
                assert_eq!(params["mode"], "form");
                assert_eq!(params["toolCallId"], "t2");
                assert_eq!(params["message"], "Tea or coffee?");
                let field = &params["requestedSchema"]["properties"]["question_0"];
                assert_eq!(field["oneOf"][0]["const"], "Tea");
                assert_eq!(field["oneOf"][0]["description"], "Hot leaves");
                let accept = json!({"action": "accept", "content": {"question_0": "Tea"}});
                client.answer(&message, accept).await;
            }
            _ if message["id"] == prompt_id => break message,
            _ => seen.push(message),
        }
    };
    assert_eq!(done["result"]["stopReason"], "end_turn");
    assert_eq!(done["result"]["usage"]["totalTokens"], 105);

    let text: Vec<&str> = updates(&seen, "agent_message_chunk")
        .filter_map(|update| update["content"]["text"].as_str())
        .collect();
    assert_eq!(text, ["Looking.", "Tea it is."]);
    let call = updates(&seen, "tool_call")
        .next()
        .expect("the Bash call shows");
    assert_eq!(call["toolCallId"], "t1");
    assert_eq!(call["kind"], "execute");
    assert_eq!(call["_meta"]["claudeCode"]["toolName"], "Bash");
    let ended = updates(&seen, "tool_call_update")
        .next()
        .expect("its result shows");
    assert_eq!(ended["status"], "completed");
    assert_eq!(ended["content"][0]["content"]["text"], "test result: ok");
    let usage = updates(&seen, "usage_update").next().expect("usage shows");
    assert_eq!(
        (usage["used"].as_u64(), usage["size"].as_u64()),
        (Some(105), Some(200_000))
    );

    drop(client);
    timeout(Duration::from_secs(10), child.wait())
        .await
        .expect("the bridge ends with its stdin")
        .unwrap();

    let args = std::fs::read_to_string(dir.path().join("args")).unwrap();
    let args: Vec<&str> = args.lines().collect();
    let after = |flag: &str| {
        let at = args.iter().position(|arg| *arg == flag)?;
        args.get(at + 1).copied()
    };
    assert_eq!(after("--session-id"), Some(session.as_str()));
    assert_eq!(after("--permission-prompt-tool"), Some("stdio"));
    let mcp: Value = serde_json::from_str(after("--mcp-config").unwrap()).unwrap();
    assert_eq!(mcp["mcpServers"]["beep"]["command"], "/srv/bin/beep-node");
    assert!(
        !args.contains(&"--disallowedTools"),
        "the client takes forms"
    );
    assert_eq!(args[args.len() - 2..], ["--effort", "high"]);

    let system = std::fs::read_to_string(dir.path().join("system")).unwrap();
    assert_eq!(system, "Work in small commits.");

    let sent = lines(&dir.path().join("stdin"));
    assert_eq!(sent[0]["request"]["subtype"], "initialize");
    assert_eq!(sent[1]["request"]["mode"], "bypassPermissions");
    assert_eq!(sent[2]["message"]["content"][0]["text"], "Fix the tests");
    let allowed = &sent[3]["response"];
    assert_eq!(allowed["request_id"], "c1");
    assert_eq!(allowed["response"]["behavior"], "allow");
    assert_eq!(allowed["response"]["updatedInput"]["command"], "cargo test");
    let answered = &sent[4]["response"];
    assert_eq!(answered["request_id"], "c2");
    assert_eq!(answered["response"]["behavior"], "allow");
    let input = &answered["response"]["updatedInput"];
    assert_eq!(input["answers"]["Tea or coffee?"], "Tea");
    assert_eq!(input["questions"][0]["header"], "Drink");
}
