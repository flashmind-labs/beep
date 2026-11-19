//! Codex's Direct driver, run as the node runs it, against a fake `codex
//! app-server`: a client drives it as the node's session does (initialize,
//! `session/new` with an MCP server and a system prompt, a model switch, a
//! prompt) and answers a command's approval, which reaches Codex as its
//! decision.
#![cfg(unix)]

use beep_node::agents::{AgentIn, AgentOut, Bridge, start};
use serde_json::{Value, json};
use std::{os::unix::fs::PermissionsExt, path::Path, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines},
    process::Command,
    time::timeout,
};

/// A `codex` that answers the handshake, the thread's start and the model
/// list, then plays one turn: a command it asks approval for, a file change,
/// streamed text, reasoning that never streamed, token usage, a plan, text
/// of a subagent's thread and the turn's end. It writes its arguments and
/// what it reads to `$FAKE_DIR`.
const FAKE_CODEX: &str = r#"#!/bin/sh
printf '%s\n' "$@" > "$FAKE_DIR/args"
next() { read -r line; printf '%s\n' "$line" >> "$FAKE_DIR/stdin"; }
ok() {
    id=$(printf '%s\n' "$line" | sed -nE 's/.*"id":([0-9]+).*/\1/p')
    printf '{"id":%s,"result":%s}\n' "$id" "$1"
}
say() { printf '{"method":"%s","params":%s}\n' "$1" "$2"; }
t='"threadId":"thread-1","turnId":"turn-1"'
models='{"data":[{"id":"model-a","model":"model-a","displayName":"gpt-6-luna","description":"Quick","hidden":false,"isDefault":true,"defaultReasoningEffort":"medium","supportedReasoningEfforts":[{"reasoningEffort":"low","description":"Light"},{"reasoningEffort":"medium","description":"Balanced"}],"additionalSpeedTiers":["fast"]},{"id":"model-b","model":"model-b","displayName":"Model B","description":"Slow","hidden":false,"isDefault":false,"defaultReasoningEffort":"low","supportedReasoningEfforts":[{"reasoningEffort":"low","description":"Light"}]}],"nextCursor":null}'
next; ok '{}'
next
next; ok '{"config":{"developer_instructions":"Be kind."}}'
next; ok '{"thread":{"id":"thread-1"},"model":"model-a","reasoningEffort":"medium","serviceTier":null}'
next; ok "$models"
next; ok '{"turn":{"id":"turn-1","items":[],"status":"inProgress"}}'
say turn/started '{"threadId":"thread-1","turn":{"id":"turn-1","items":[],"status":"inProgress"}}'
say item/started '{'"$t"',"item":{"type":"commandExecution","id":"c1","command":"/bin/bash -lc '"'cargo test'"'","cwd":"/srv/app","commandActions":[{"type":"unknown","command":"cargo test"}],"status":"inProgress","source":"agent"}}'
printf '{"id":"r1","method":"item/commandExecution/requestApproval","params":{%s,"itemId":"c1","command":"cargo test","cwd":"/srv/app"}}\n' "$t"
next
say item/completed '{'"$t"',"item":{"type":"commandExecution","id":"c1","command":"cargo test","cwd":"/srv/app","commandActions":[{"type":"unknown","command":"cargo test"}],"status":"completed","aggregatedOutput":"test result: ok\n","exitCode":0}}'
say item/started '{'"$t"',"item":{"type":"fileChange","id":"f1","status":"inProgress","changes":[{"path":"/srv/app/notes.txt","kind":{"type":"update","move_path":null},"diff":"@@ -1,2 +1,2 @@\n a\n-b\n+c\n"}]}}'
say item/completed '{'"$t"',"item":{"type":"fileChange","id":"f1","status":"completed","changes":[]}}'
say item/agentMessage/delta '{'"$t"',"itemId":"m1","delta":"Done."}'
say item/completed '{'"$t"',"item":{"type":"agentMessage","id":"m1","text":"Done."}}'
say item/completed '{'"$t"',"item":{"type":"reasoning","id":"th1","summary":["Thought whole."],"content":[]}}'
say thread/tokenUsage/updated '{'"$t"',"tokenUsage":{"total":{"inputTokens":100,"cachedInputTokens":40,"outputTokens":20,"reasoningOutputTokens":5,"totalTokens":120},"last":{"inputTokens":50,"cachedInputTokens":0,"outputTokens":10,"reasoningOutputTokens":0,"totalTokens":60},"modelContextWindow":200000}}'
say turn/plan/updated '{'"$t"',"plan":[{"step":"Run the tests","status":"completed"},{"step":"Fix them","status":"inProgress"}]}'
say item/agentMessage/delta '{"threadId":"thread-2","turnId":"turn-9","itemId":"m9","delta":"A subagent speaks."}'
say turn/completed '{"threadId":"thread-1","turn":{"id":"turn-1","items":[],"status":"completed"}}'
cat > /dev/null
"#;

/// The bridge's ACP, as the session sees it.
struct Client {
    stdin: AgentIn,
    stdout: Lines<BufReader<AgentOut>>,
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

fn option<'a>(options: &'a Value, id: &str) -> &'a Value {
    options
        .as_array()
        .unwrap()
        .iter()
        .find(|option| option["id"] == id)
        .unwrap_or(&Value::Null)
}

#[tokio::test]
async fn a_turn_with_an_approval_reaches_codex() {
    let dir = tempfile::tempdir().unwrap();
    let codex = dir.path().join("codex");
    std::fs::write(&codex, FAKE_CODEX).unwrap();
    std::fs::set_permissions(&codex, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut cli = Command::new(&codex);
    cli.args(["-c", "model_verbosity=low"])
        .env("FAKE_DIR", dir.path())
        .env("INITIAL_AGENT_MODE", "agent-full-access")
        .env_remove("CODEX_CONFIG")
        .current_dir(dir.path());
    let agent = start(cli, Some(&Bridge::Codex { wsl: None })).expect("the bridge starts");
    let mut running = agent.running;
    let mut client = Client {
        stdin: agent.stdin,
        stdout: BufReader::new(agent.stdout).lines(),
        next_id: 0,
    };

    let (_, initialized) = client
        .call(
            "initialize",
            json!({"protocolVersion": 1, "clientCapabilities": {"elicitation": {"form": {}}}}),
        )
        .await;
    let initialized = &initialized["result"];
    assert_eq!(initialized["agentCapabilities"]["loadSession"], true);
    assert_eq!(initialized["agentInfo"]["name"], "beep-node codex-acp");
    assert_eq!(initialized["_meta"]["steering"]["supported"], true);

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
    let created = &created["result"];
    assert_eq!(created["sessionId"], "thread-1");
    assert_eq!(created["modes"]["currentModeId"], "agent-full-access");
    let options = &created["configOptions"];
    let ids: Vec<&str> = options
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|option| option["id"].as_str())
        .collect();
    assert_eq!(
        ids,
        [
            "mode",
            "collaboration_mode",
            "model",
            "reasoning_effort",
            "fast-mode"
        ]
    );
    let model = option(options, "model");
    assert_eq!(model["category"], "model");
    assert_eq!(model["currentValue"], "model-a");
    assert_eq!(model["options"][0]["name"], "6 Luna");
    assert_eq!(
        option(options, "reasoning_effort")["category"],
        "thought_level"
    );

    let switch = json!({"sessionId": "thread-1", "configId": "model", "value": "model-b"});
    let (_, switched) = client.call("session/set_config_option", switch).await;
    let options = &switched["result"]["configOptions"];
    assert_eq!(option(options, "model")["currentValue"], "model-b");
    assert_eq!(
        option(options, "reasoning_effort")["currentValue"],
        "low",
        "the effort fits the new model"
    );
    assert!(
        option(options, "fast-mode").is_null(),
        "model-b has no fast mode"
    );

    let prompt =
        json!({"sessionId": "thread-1", "prompt": [{"type": "text", "text": "Fix the tests"}]});
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
                let options = &message["params"]["options"];
                assert_eq!(options[0]["optionId"], "allow_once");
                assert_eq!(options[1]["optionId"], "allow_for_session");
                let always =
                    json!({"outcome": {"outcome": "selected", "optionId": "allow_for_session"}});
                client
                    .send(json!({"jsonrpc": "2.0", "id": message["id"], "result": always}))
                    .await;
            }
            _ if message["id"] == prompt_id => break message,
            _ => seen.push(message),
        }
    };
    assert_eq!(done["result"]["stopReason"], "end_turn");
    assert_eq!(done["result"]["usage"]["totalTokens"], 120);
    assert_eq!(
        done["result"]["_meta"]["quota"]["token_count"]["totalTokens"],
        120
    );
    assert_eq!(
        done["result"]["_meta"]["quota"]["model_usage"][0]["model"],
        "model-b"
    );

    let call = updates(&seen, "tool_call")
        .find(|call| call["toolCallId"] == "c1")
        .expect("the command shows");
    assert_eq!(call["title"], "cargo test", "without the shell around it");
    assert_eq!(call["kind"], "execute");
    let ran = updates(&seen, "tool_call_update")
        .find(|update| update["toolCallId"] == "c1")
        .expect("its end shows");
    assert_eq!(ran["status"], "completed");
    assert_eq!(ran["content"][0]["content"]["text"], "test result: ok\n");

    let edit = updates(&seen, "tool_call")
        .find(|call| call["toolCallId"] == "f1")
        .expect("the file change shows");
    assert_eq!(edit["kind"], "edit");
    let diff = &edit["content"][0];
    assert_eq!(diff["type"], "diff");
    assert_eq!(diff["path"], "/srv/app/notes.txt");
    assert_eq!(
        (diff["oldText"].as_str(), diff["newText"].as_str()),
        (Some("a\nb\n"), Some("a\nc\n"))
    );

    let text: Vec<&str> = updates(&seen, "agent_message_chunk")
        .filter_map(|update| update["content"]["text"].as_str())
        .collect();
    assert_eq!(text, ["Done."], "streamed once, and no subagent's text");
    let thought: Vec<&str> = updates(&seen, "agent_thought_chunk")
        .filter_map(|update| update["content"]["text"].as_str())
        .collect();
    assert_eq!(thought, ["Thought whole."]);
    let plan = updates(&seen, "plan").next().expect("the plan shows");
    assert_eq!(plan["entries"][1]["status"], "in_progress");
    let usage = updates(&seen, "usage_update").next().expect("usage shows");
    assert_eq!(
        (usage["used"].as_u64(), usage["size"].as_u64()),
        (Some(60), Some(200_000))
    );

    // The bridge ends with the session's side, and stops the CLI.
    let Client {
        stdin, mut stdout, ..
    } = client;
    drop(stdin);
    let ended = timeout(Duration::from_secs(10), stdout.next_line())
        .await
        .expect("the bridge ends with its stdin");
    assert!(matches!(ended, Ok(None)), "{ended:?}");
    running.stop(Duration::from_secs(5)).await;

    let args = std::fs::read_to_string(dir.path().join("args")).unwrap();
    let args: Vec<&str> = args.lines().collect();
    assert_eq!(args, ["-c", "model_verbosity=low", "app-server"]);

    let sent = lines(&dir.path().join("stdin"));
    assert_eq!(sent[0]["method"], "initialize");
    assert_eq!(sent[0]["params"]["capabilities"]["experimentalApi"], true);
    assert_eq!(sent[1]["method"], "initialized");
    assert_eq!(sent[2]["method"], "config/read");
    let thread = &sent[3];
    assert_eq!(thread["method"], "thread/start");
    let params = &thread["params"];
    assert_eq!(
        params["developerInstructions"],
        "Be kind.\n\nWork in small commits."
    );
    assert_eq!(
        params["config"]["mcp_servers"]["beep"]["command"],
        "/srv/bin/beep-node"
    );
    assert_eq!(params["approvalPolicy"], "never");
    assert_eq!(params["sandbox"], "danger-full-access");
    assert_eq!(sent[4]["method"], "model/list");
    let turn = &sent[5];
    assert_eq!(turn["method"], "turn/start");
    let params = &turn["params"];
    assert_eq!(params["threadId"], "thread-1");
    assert_eq!(params["input"][0]["text"], "Fix the tests");
    assert_eq!(
        (params["model"].as_str(), params["effort"].as_str()),
        (Some("model-b"), Some("low"))
    );
    assert_eq!(params["sandboxPolicy"]["type"], "dangerFullAccess");
    assert_eq!(params["collaborationMode"]["mode"], "default");
    assert_eq!(
        sent[6],
        json!({"id": "r1", "result": {"decision": "acceptForSession"}})
    );
}
