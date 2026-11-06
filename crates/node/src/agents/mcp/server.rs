//! MCP over one connection: newline-delimited JSON-RPC, as the stdio
//! transport speaks it. Calls run side by side, a few at a time, so a long
//! `run_on` does not hold up the next call.

use super::catalog::{self, ASK_USER};
use crate::agents::{Answered, Asking, Line, LineReader};
use anyhow::Result;
use beep_proto::node::{
    agent::{AnswerOption, OptionKind},
    tools::{Approved, Field, Reply, Request, Tool},
};
use futures::{StreamExt, stream::FuturesUnordered};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{future::Future, time::Duration};
use tokio::{
    io::{AsyncBufRead, AsyncWrite, AsyncWriteExt},
    time::timeout,
};

/// The longest message read; a longer one is dropped.
const MAX_MESSAGE: usize = 1024 * 1024;
/// The most calls of one connection running at once; the next waits.
const MAX_RUNNING: usize = 8;
/// How long a question waits for the user.
const ASK_LIMIT: Duration = Duration::from_secs(60 * 60);
/// The MCP versions this server speaks, newest first.
const VERSIONS: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];

const ALLOW: &str = "allow";
const ALWAYS: &str = "always";
const DENY: &str = "deny";

/// Where calls go.
pub(crate) trait Backend: Sync {
    /// Runs one call on the server.
    fn call(&self, request: &Request) -> impl Future<Output = Result<Reply>> + Send;
    /// Puts `question` to the user in `session` and waits for the answer.
    fn ask(&self, session: &str, question: Asking)
    -> impl Future<Output = Result<Answered>> + Send;
}

/// Serves MCP for `session` until the reader ends and every call is
/// answered.
pub(super) async fn serve<B, R, W>(
    backend: &B,
    session: &str,
    reader: R,
    mut writer: W,
) -> Result<()>
where
    B: Backend,
    R: AsyncBufRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut lines = LineReader::new(reader, MAX_MESSAGE);
    let mut running = FuturesUnordered::new();
    let mut open = true;
    loop {
        tokio::select! {
            line = lines.next(), if open && running.len() < MAX_RUNNING => match line? {
                None => open = false,
                Some(Line::TooLong(length)) => {
                    log::warn!("dropped an MCP message of {length} bytes");
                }
                Some(Line::Text(text)) if text.trim().is_empty() => {}
                Some(Line::Text(text)) => match serde_json::from_str::<Value>(&text) {
                    Ok(message) => running.push(answer(backend, session, message)),
                    Err(e) => {
                        let error = failure(Value::Null, -32700, &format!("not JSON: {e}"));
                        write(&mut writer, &error).await?;
                    }
                },
            },
            Some(answered) = running.next() => {
                if let Some(answered) = answered {
                    write(&mut writer, &answered).await?;
                }
            }
            else => return Ok(()),
        }
    }
}

async fn write<W: AsyncWrite + Unpin>(writer: &mut W, message: &Value) -> Result<()> {
    let mut line = serde_json::to_vec(message)?;
    line.push(b'\n');
    writer.write_all(&line).await?;
    writer.flush().await?;
    Ok(())
}

fn success(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn failure(id: Value, code: i32, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

/// The answer to one message; `None` for a notification or a response.
async fn answer<B: Backend>(backend: &B, session: &str, message: Value) -> Option<Value> {
    let method = message.get("method")?.as_str()?.to_owned();
    let id = message.get("id")?.clone();
    let params = message.get("params").cloned().unwrap_or(Value::Null);
    Some(match method.as_str() {
        "initialize" => {
            let asked = params.get("protocolVersion").and_then(Value::as_str);
            let version = asked
                .filter(|asked| VERSIONS.contains(asked))
                .unwrap_or(VERSIONS[0]);
            success(
                id,
                json!({
                    "protocolVersion": version,
                    "capabilities": { "tools": { "listChanged": false } },
                    "serverInfo": { "name": super::SERVER_NAME, "version": env!("CARGO_PKG_VERSION") },
                    "instructions": super::BRIEF,
                }),
            )
        }
        "ping" => success(id, json!({})),
        "tools/list" => success(id, json!({ "tools": catalog::tools() })),
        "tools/call" => {
            let name = params
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let args = params
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));
            let called = call(backend, session, name, args).await;
            success(
                id,
                json!({
                    "content": [{ "type": "text", "text": called.text }],
                    "isError": called.failed,
                }),
            )
        }
        other => failure(id, -32601, &format!("unknown method {other}")),
    })
}

/// A tool's result for the agent.
#[derive(Debug, PartialEq)]
struct Called {
    text: String,
    failed: bool,
}

impl Called {
    fn ok(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            failed: false,
        }
    }

    fn failed(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            failed: true,
        }
    }
}

async fn call<B: Backend>(backend: &B, session: &str, name: &str, args: Value) -> Called {
    if name == ASK_USER {
        return ask_user(backend, session, args).await;
    }
    let tool = match name.parse::<Tool>() {
        // Coding agents get no memories: those tools are the assistant's.
        Ok(
            Tool::Unknown
            | Tool::StandingRules
            | Tool::Remember
            | Tool::Recall
            | Tool::UpdateMemory
            | Tool::Forget,
        )
        | Err(_) => {
            return Called::failed(format!("There is no tool {name}."));
        }
        Ok(tool) => tool,
    };
    let mut request = Request {
        session: session.to_owned(),
        tool,
        args: args.to_string(),
        approved: Approved::No,
    };
    loop {
        let reply = match backend.call(&request).await {
            Ok(reply) => reply,
            Err(e) => return Called::failed(format!("{e:#}")),
        };
        match reply {
            Reply::Done { text, failed } => return Called { text, failed },
            Reply::Ask {
                app,
                action,
                fields,
            } if request.approved == Approved::No => {
                let question = approval(&app, &action, &fields);
                request.approved = match asked(backend, session, question).await {
                    Ok(Answered::Picked(option)) if option.id == ALLOW => Approved::Once,
                    Ok(Answered::Picked(option)) if option.id == ALWAYS => Approved::Always,
                    Ok(_) => {
                        return Called::failed(format!(
                            "The user did not allow {name}. Do not try it again unless they ask."
                        ));
                    }
                    Err(e) => return Called::failed(format!("Not run: {e:#}")),
                };
            }
            Reply::Ask { .. } => {
                return Called::failed("The server asked again after the user allowed it.");
            }
            other => return Called::failed(format!("The server answered {other:?}.")),
        }
    }
}

/// The question that asks to allow a call.
fn approval(app: &str, action: &str, fields: &[Field]) -> Asking {
    let detail = fields
        .iter()
        .map(|field| format!("{}: {}", field.label, field.value))
        .collect::<Vec<_>>()
        .join("\n");
    let option = |id: &str, name: &str, kind| AnswerOption {
        id: id.into(),
        name: name.into(),
        kind,
    };
    Asking {
        text: format!("Allow {action}? ({app})"),
        detail: (!detail.is_empty()).then_some(detail),
        options: vec![
            option(ALLOW, "Allow", OptionKind::AllowOnce),
            option(ALWAYS, "Always allow", OptionKind::AllowAlways),
            option(DENY, "Deny", OptionKind::RejectOnce),
        ],
    }
}

async fn asked<B: Backend>(backend: &B, session: &str, question: Asking) -> Result<Answered> {
    match timeout(ASK_LIMIT, backend.ask(session, question)).await {
        Ok(answered) => answered,
        Err(_) => anyhow::bail!("the user did not answer within an hour"),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AskArgs {
    question: String,
    #[serde(default)]
    options: Vec<String>,
}

async fn ask_user<B: Backend>(backend: &B, session: &str, args: Value) -> Called {
    let args: AskArgs = match serde_json::from_value(args) {
        Ok(args) => args,
        Err(e) => return Called::failed(format!("Bad arguments: {e}")),
    };
    if args.question.trim().is_empty() {
        return Called::failed("The question is empty.");
    }
    let options = args
        .options
        .iter()
        .take(8)
        .enumerate()
        .map(|(at, name)| AnswerOption {
            id: (at + 1).to_string(),
            name: name.clone(),
            kind: OptionKind::Other,
        })
        .collect();
    let question = Asking {
        text: args.question,
        detail: None,
        options,
    };
    match asked(backend, session, question).await {
        Ok(Answered::Picked(option)) => Called::ok(format!("The user picked: {}", option.name)),
        Ok(Answered::Said(text)) => Called::ok(format!("The user answered: {text}")),
        Err(e) => Called::failed(format!("No answer: {e:#}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use tokio::io::{AsyncBufReadExt, BufReader, duplex};

    /// A server that asks first for `run_on` and answers everything else,
    /// and a user who answers with `answer`.
    struct Fake {
        calls: Mutex<Vec<Request>>,
        asked: Mutex<Vec<String>>,
        answer: &'static str,
    }

    impl Fake {
        fn new(answer: &'static str) -> Self {
            Self {
                calls: Mutex::default(),
                asked: Mutex::default(),
                answer,
            }
        }
    }

    impl Backend for Fake {
        async fn call(&self, request: &Request) -> Result<Reply> {
            self.calls.lock().unwrap().push(request.clone());
            Ok(match (request.tool, request.approved) {
                (Tool::RunOn, Approved::No) => Reply::Ask {
                    app: "Your machines".into(),
                    action: "run on".into(),
                    fields: vec![Field {
                        label: "Command".into(),
                        value: "cargo test".into(),
                    }],
                },
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

        async fn ask(&self, session: &str, question: Asking) -> Result<Answered> {
            assert_eq!(session, "a3f9c1");
            self.asked.lock().unwrap().push(question.text.clone());
            Ok(question
                .options
                .into_iter()
                .find(|option| option.id == self.answer)
                .map_or_else(|| Answered::Said(self.answer.into()), Answered::Picked))
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
        let fake = Fake::new(ALLOW);
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
                    json!({ "name": "job_status", "arguments": { "id": "task:1@build-box" } }),
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
                "list_machines",
                "run_on",
                "sync_project",
                "send_files",
                "job_status",
                "cancel",
                "machine_log",
                "start_agent",
                "agent_status",
                "send_to_agent",
                "stop_agent",
                "list_agents",
                "ask_user"
            ]
        );
        let hints = |name: &str| {
            let tools = out[1]["result"]["tools"].as_array().unwrap();
            let tool = tools.iter().find(|tool| tool["name"] == name).unwrap();
            tool["annotations"].clone()
        };
        // Codex asks unless a tool reads only, or is neither destructive nor open world.
        assert_eq!(hints("list_machines")["readOnlyHint"], true);
        assert_eq!(hints("run_on")["destructiveHint"], false);
        assert_eq!(hints("run_on")["openWorldHint"], false);
        assert_eq!(text(&out[2]), "- build-box: online");
        assert_eq!(out[2]["result"]["isError"], false);
        assert_eq!(text(&out[3]), "build-box is offline.");
        assert_eq!(out[3]["result"]["isError"], true);
        assert_eq!(out[4]["error"]["code"], -32601);
        let calls = fake.calls.lock().unwrap();
        assert_eq!(calls[0].tool, Tool::ListMachines);
        assert_eq!(calls[0].session, "a3f9c1");
        assert!(fake.asked.lock().unwrap().is_empty(), "nothing acted");
    }

    #[tokio::test]
    async fn an_acting_call_asks_the_user_first() {
        let run = request(
            1,
            "tools/call",
            json!({ "name": "run_on", "arguments": { "machine": "build-box", "command": "cargo test" } }),
        );
        let allowing = Fake::new(ALLOW);
        let out = exchange(&allowing, std::slice::from_ref(&run)).await;
        assert_eq!(text(&out[0]), "Exit code 0");
        assert_eq!(
            *allowing.asked.lock().unwrap(),
            ["Allow run on? (Your machines)"]
        );
        let approvals: Vec<Approved> = allowing
            .calls
            .lock()
            .unwrap()
            .iter()
            .map(|call| call.approved)
            .collect();
        assert_eq!(approvals, [Approved::No, Approved::Once]);

        let denying = Fake::new(DENY);
        let out = exchange(&denying, &[run]).await;
        assert_eq!(out[0]["result"]["isError"], true);
        assert!(text(&out[0]).contains("did not allow run_on"), "{out:?}");
        assert_eq!(denying.calls.lock().unwrap().len(), 1, "it never ran");
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
}
