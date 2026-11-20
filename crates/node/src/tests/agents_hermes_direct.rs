use super::ask::{Asked, answer, to_client};
use super::events::{finished, update};
use super::link::{Gateway, Incoming, LinkError, check, socket_url};
use super::serve;
use crate::agents::acp::{self, allow_option};
use agent_client_protocol::{Agent, Builder, Client, ConnectionTo, Dispatch, Handled, Responder, UntypedMessage, on_receive_dispatch};
use beep_proto::node::agent::{AnswerOption, OptionKind};
use beep_proto::SecretText;
use futures::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::time::Duration;
use tokio::{io::AsyncWriteExt, net::TcpListener, sync::mpsc};
use tokio_tungstenite::{
    accept_hdr_async,
    tungstenite::{Message, handshake::server::{Request, Response}},
};

fn approval(allow_session: bool, allow_permanent: bool) -> Value {
    json!({"session_id": "12b450c6", "request_id": "ap1", "command": "rm -rf build", "description": "Delete the build",
        "choices": ["once", "session", "always", "deny"], "allow_permanent": allow_permanent, "allow_session": allow_session,
        "smart_denied": null, "tool_name": "terminal", "gateway_session_id": null})
}

fn selected(option: &str) -> Value {
    json!({"outcome": {"outcome": "selected", "optionId": option}})
}

#[test]
fn a_clarify_becomes_a_form_and_its_answer_goes_back_by_qid() {
    let params = json!({"session_id": "a3026a4a", "questions": [
        {"qid": "q0", "question": "Which color do you prefer?", "choices": ["Red (Recommended)", "Green", "Blue"], "multi_select": false},
        {"qid": "q1", "question": "Which sizes?", "choices": ["S", "M"], "multi_select": true}]});
    let (asked, form) = to_client("clarify", &params).unwrap();
    assert_eq!(form["mode"], "form");
    assert_eq!(form["requestedSchema"]["properties"]["q0"]["oneOf"][2]["const"], "Blue");
    assert_eq!(form["requestedSchema"]["properties"]["q1"]["type"], "array");

    let result = answer(&asked, &json!({"action": "accept", "content": {"q0": "Blue", "q1": ["S", "M"]}}));
    assert_eq!(result, json!({"answers": {"q0": "Blue", "q1": ["S", "M"]}}));
}

#[test]
fn an_approval_offers_always_only_when_hermes_allows_it() {
    let ids = |session, permanent| -> Vec<String> {
        let (_, request) = to_client("approval", &approval(session, permanent)).unwrap();
        request["options"].as_array().unwrap().iter().map(|o| o["optionId"].as_str().unwrap().to_owned()).collect()
    };

    assert_eq!(ids(false, false), ["once", "deny"]);
    assert_eq!(ids(true, false), ["once", "session", "deny"]);
    assert_eq!(ids(true, true), ["once", "session", "always", "deny"]);

    // An answer for an option Hermes did not offer is a refusal.
    let (asked, _) = to_client("approval", &approval(false, false)).unwrap();
    assert_eq!(answer(&asked, &selected("always")), json!({"choice": "deny"}));
}

#[test]
fn an_auto_allowed_approval_answers_once() {
    let (asked, request) = to_client("approval", &approval(true, true)).unwrap();
    let options: Vec<AnswerOption> = request["options"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| AnswerOption {
            id: o["optionId"].as_str().unwrap().to_owned(),
            name: o["name"].as_str().unwrap().to_owned(),
            kind: o["kind"].as_str().unwrap().parse().unwrap_or(OptionKind::Other),
        })
        .collect();

    let picked = allow_option(&options).unwrap();
    assert_eq!(answer(&asked, &selected(&picked.id)), json!({"choice": "once"}));
    assert_eq!(answer(&asked, &selected("session")), json!({"choice": "session"}));
    assert_eq!(answer(&asked, &selected("always")), json!({"choice": "always"}));
}

#[test]
fn a_declined_form_or_rejected_approval_denies() {
    let (asked, _) = to_client("approval", &approval(true, true)).unwrap();
    assert_eq!(answer(&asked, &selected("deny")), json!({"choice": "deny"}));
    assert_eq!(answer(&asked, &json!({"outcome": {"outcome": "cancelled"}})), json!({"choice": "deny"}));

    let asked = Asked::Clarify { qids: vec!["q0".into()] };
    let cancelled = json!({"answers": null, "outcome": "cancelled"});
    assert_eq!(answer(&asked, &json!({"action": "decline"})), cancelled);
    assert_eq!(answer(&asked, &json!({"action": "cancel"})), cancelled);
}

#[test]
fn message_and_thinking_deltas_become_chunks() {
    let message = json!({"type": "message.delta", "session_id": "12b450c6", "payload": {"text": " command", "turn_id": "t", "segment": 0, "offset": 5}, "seq": 15});
    assert_eq!(update(&message), vec![json!({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": " command"}})]);

    let reasoning = json!({"type": "reasoning.delta", "session_id": "12b450c6", "payload": {"text": "Let me think"}, "seq": 4});
    assert_eq!(update(&reasoning), vec![json!({"sessionUpdate": "agent_thought_chunk", "content": {"type": "text", "text": "Let me think"}})]);

    // The spinner text is not reasoning.
    let spinner = json!({"type": "thinking.delta", "session_id": "12b450c6", "payload": {"text": "( \u{2d8}\u{2323}\u{2d8})\u{2661} brainstorming..."}, "seq": 4});
    assert!(update(&spinner).is_empty());
    assert!(update(&json!({"type": "sessions.changed", "session_id": "", "payload": {}})).is_empty());
}

#[test]
fn a_tool_runs_from_start_to_complete_and_failures_show() {
    let start = json!({"type": "tool.start", "session_id": "s", "payload": {"tool_id": "call_60ba", "name": "terminal", "context": "echo hello", "args": {"command": "echo hello", "timeout": 30}}, "seq": 9});
    let started = &update(&start)[0];
    assert_eq!(started["sessionUpdate"], "tool_call");
    assert_eq!(started["toolCallId"], "call_60ba");
    assert_eq!(started["status"], "in_progress");
    assert_eq!(started["title"], "terminal");

    let done = json!({"type": "tool.complete", "session_id": "s", "payload": {"tool_id": "call_60ba", "name": "terminal", "args": {"command": "echo hello"}, "duration_s": 0.06, "result": {"output": "hello", "exit_code": 0, "error": null}}, "seq": 10});
    let ended = &update(&done)[0];
    assert_eq!(ended["sessionUpdate"], "tool_call_update");
    assert_eq!(ended["status"], "completed");
    assert_eq!(ended["content"][0]["content"]["text"], "hello");

    let bad_exit = json!({"type": "tool.complete", "payload": {"tool_id": "c", "name": "terminal", "result": {"output": "", "exit_code": 2, "error": null}}});
    assert_eq!(update(&bad_exit)[0]["status"], "failed");
    let errored = json!({"type": "tool.complete", "payload": {"tool_id": "c", "name": "terminal", "result": {"output": null, "exit_code": 0, "error": "boom"}}});
    assert_eq!(update(&errored)[0]["status"], "failed");
}

#[test]
fn a_title_and_the_end_of_a_turn() {
    let title = json!({"type": "session.title", "session_id": "s", "payload": {"session_id": "2026", "title": "Run echo hello in terminal"}, "seq": 5});
    assert_eq!(update(&title), vec![json!({"sessionUpdate": "session_info_update", "title": "Run echo hello in terminal"})]);

    assert!(finished(&json!({"type": "message.complete", "payload": {"text": "done"}})));
    assert!(!finished(&json!({"type": "message.delta", "payload": {"text": "x"}})));
}

type Server = tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>;

fn event(kind: &str) -> Message {
    Message::text(json!({"jsonrpc": "2.0", "method": "event", "params": {"type": kind, "payload": {}}}).to_string())
}

async fn read(server: &mut Server) -> Value {
    loop {
        if let Message::Text(text) = server.next().await.unwrap().unwrap() {
            return serde_json::from_str(&text).unwrap();
        }
    }
}

/// A gateway that sends ready, expects the capabilities call and answers it,
/// then hands the socket to `then`. Yields the port and the query it saw.
async fn fake_gateway<F, Fut>(then: F) -> (u16, tokio::task::JoinHandle<String>)
where
    F: FnOnce(Server) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = ()> + Send,
{
    let (listener, port) = listen().await;
    let task = tokio::spawn(async move {
        let (server, seen) = accept(&listener).await;
        then(server).await;

        seen
    });

    (port, task)
}

async fn listen() -> (TcpListener, u16) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    (listener, port)
}

/// The next connection, made ready and given the capabilities, with the
/// path and query it asked for.
#[allow(clippy::result_large_err)] // the handshake callback's error type is tungstenite's
async fn accept(listener: &TcpListener) -> (Server, String) {
    let (stream, _) = listener.accept().await.unwrap();
    let mut seen = String::new();
    let mut server = accept_hdr_async(stream, |request: &Request, response: Response| {
        seen = format!("{} {}", request.uri().path(), request.uri().query().unwrap_or_default());
        Ok(response)
    })
    .await
    .unwrap();
    server.send(event("gateway.ready")).await.unwrap();
    let capabilities = read(&mut server).await;
    assert_eq!(capabilities["method"], "client.capabilities");
    assert_eq!(capabilities["params"], json!({"server_requests": true}));
    server.send(Message::text(json!({"jsonrpc": "2.0", "id": capabilities["id"], "result": {}}).to_string())).await.unwrap();

    (server, seen)
}

#[tokio::test]
async fn connecting_waits_for_ready_and_sends_the_capabilities() {
    let (port, task) = fake_gateway(|_server| async {}).await;
    let token = SecretText::new("a b&c=d");

    Gateway::connect("127.0.0.1", port, &token).await.unwrap();

    assert_eq!(task.await.unwrap(), "/api/ws token=a+b%26c%3Dd");
}

#[tokio::test]
async fn a_call_gets_its_own_reply_while_events_and_asks_arrive() {
    let (port, _task) = fake_gateway(|mut server| async move {
        let call = read(&mut server).await;
        assert_eq!(call["method"], "prompt.submit");
        server.send(event("message.delta")).await.unwrap();
        let ask = json!({"jsonrpc": "2.0", "id": "srq-1", "method": "approval", "params": {"command": "ls"}});
        server.send(Message::text(ask.to_string())).await.unwrap();
        server.send(Message::text(json!({"jsonrpc": "2.0", "id": 999, "result": "stray"}).to_string())).await.unwrap();
        server.send(Message::text(json!({"jsonrpc": "2.0", "id": call["id"], "result": {"ok": true}}).to_string())).await.unwrap();
        let answered = read(&mut server).await;
        assert_eq!(answered, json!({"jsonrpc": "2.0", "id": "srq-1", "result": {"choice": "once"}}));
        let failing = read(&mut server).await;
        let error = json!({"jsonrpc": "2.0", "id": failing["id"], "error": {"code": -32602, "message": "bad"}});
        server.send(Message::text(error.to_string())).await.unwrap();
    })
    .await;
    let mut gateway = Gateway::connect("127.0.0.1", port, &SecretText::new("t")).await.unwrap();

    let result = gateway.call("prompt.submit", json!({"text": "hi"})).await.unwrap();
    assert_eq!(result, json!({"ok": true}));

    let Some(Incoming::Event(params)) = gateway.next().await else { panic!("event first") };
    assert_eq!(params["type"], "message.delta");
    let Some(Incoming::Ask { id, method, params }) = gateway.next().await else { panic!("ask second") };
    assert_eq!((id.as_str(), method.as_str(), params["command"].as_str()), ("srq-1", "approval", Some("ls")));

    gateway.answer(&id, json!({"choice": "once"})).unwrap();
    let err = gateway.call("session.nope", json!({})).await.unwrap_err();
    assert!(matches!(err, LinkError::Rpc { code: -32602, ref message } if message == "bad"));
}

#[tokio::test]
async fn a_closed_socket_ends_next_and_fails_calls() {
    let (port, _task) = fake_gateway(|mut server| async move {
        let _ = read(&mut server).await;
        server.close(None).await.unwrap();
    })
    .await;
    let mut gateway = Gateway::connect("127.0.0.1", port, &SecretText::new("t")).await.unwrap();

    let pending = gateway.call("anything", json!({})).await;
    assert!(matches!(pending, Err(LinkError::Closed)));
    assert!(gateway.next().await.is_none());
    assert!(matches!(gateway.call("later", json!({})).await, Err(LinkError::Closed)));
}

#[tokio::test]
async fn a_refused_upgrade_is_a_refused_token_without_the_url() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        stream.write_all(b"HTTP/1.1 403 Forbidden\r\ncontent-length: 0\r\nconnection: close\r\n\r\n").await.unwrap();
    });

    let err = Gateway::connect("127.0.0.1", port, &SecretText::new("hunter2")).await.err().unwrap();

    assert!(matches!(err, LinkError::Refused));
    assert_eq!(err.to_string(), "Hermes refused the token");
}

#[tokio::test]
async fn a_closed_port_is_unreachable_without_the_url_or_token() {
    let port = TcpListener::bind("127.0.0.1:0").await.unwrap().local_addr().unwrap().port();

    let err = Gateway::connect("127.0.0.1", port, &SecretText::new("hunter2")).await.err().unwrap();

    assert!(matches!(err, LinkError::Unreachable(_)));
    assert_eq!(err.to_string(), format!("nothing answers on 127.0.0.1:{port}"));
}

#[tokio::test]
async fn a_check_answers_valid_refused_or_unreachable_without_the_token() {
    use beep_proto::CredentialCheck;
    let token = SecretText::new("hunter2");
    let (port, _task) = fake_gateway(|_server| async {}).await;
    assert_eq!(
        check("127.0.0.1", port, &token).await,
        CredentialCheck::Valid { account: "Hermes".into(), machine: None }
    );

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        stream.write_all(b"HTTP/1.1 401 Unauthorized\r\ncontent-length: 0\r\nconnection: close\r\n\r\n").await.unwrap();
    });
    let refused = check("127.0.0.1", port, &token).await;
    assert_eq!(refused, CredentialCheck::Refused { machine: None, message: "Hermes refused the token".into() });

    let port = TcpListener::bind("127.0.0.1:0").await.unwrap().local_addr().unwrap().port();
    let CredentialCheck::Unreachable { message } = check("127.0.0.1", port, &token).await else {
        panic!("a closed port is unreachable");
    };
    assert!(!message.contains("hunter2") && !message.contains("ws://"), "{message}");
}

#[test]
fn an_ipv6_host_goes_in_brackets_with_or_without_them() {
    let token = SecretText::new("t");

    for host in ["fd00::1", "[fd00::1]"] {
        assert_eq!(socket_url(host, 9119, &token).unwrap().as_str(), "ws://[fd00::1]:9119/api/ws?token=t");
    }
}

// The bridge, driven by an ACP client over pipes against a fake Hermes.

const STORED: &str = "20261120_190828_8c264b";

/// What the client was sent: a session update, or a request to answer.
enum Seen {
    Update(Value),
    Asked(String, Value, Box<Responder>),
}

fn frame(value: Value) -> Message {
    Message::text(value.to_string())
}

fn hermes_event(live: &str, kind: &str, payload: Value, seq: u64) -> Message {
    frame(json!({"jsonrpc": "2.0", "method": "event", "params": {"type": kind, "session_id": live, "payload": payload, "seq": seq}}))
}

fn clarify(id: &str) -> Value {
    json!({"id": id, "method": "clarify", "params": {"session_id": "live1", "questions": [
        {"qid": "q0", "question": "Which color do you prefer?", "choices": ["Red (Recommended)", "Green", "Blue"], "multi_select": false}]}})
}

fn server_request(request: &Value) -> Message {
    let mut request = request.clone();
    request["jsonrpc"] = json!("2.0");
    frame(request)
}

/// Reads the call `method`, answers it with `result` and yields its params.
async fn replied(server: &mut Server, method: &str, result: Value) -> Value {
    let call = read(server).await;
    assert_eq!(call["method"], method, "{call}");
    server.send(frame(json!({"jsonrpc": "2.0", "id": call["id"], "result": result}))).await.unwrap();

    call["params"].clone()
}

/// Answers `session.create` with the live id `live1`, Hermes picking the
/// folder, and the read of its epoch.
async fn created(server: &mut Server) {
    let params = replied(server, "session.create", json!({"session_id": "live1", "stored_session_id": STORED, "info": {}})).await;
    assert!(params.get("cwd").is_none());
    let read = replied(server, "session.events.since", since(1, json!([]))).await;
    assert_eq!(read, json!({"session_id": "live1", "last_seen": 0}));
}

/// What `session.events.since` gives in `epoch`, with nothing missed.
fn since(epoch: u64, open_requests: Value) -> Value {
    json!({"count": 0, "epoch": epoch, "events": [], "latest_seq": 0, "open_requests": open_requests, "truncated": false})
}

/// Drops the socket mid-turn and resumes on the next one as `live2`, with
/// Hermes's `epoch` then.
async fn dropped_and_resumed(listener: &TcpListener, server: Server, epoch: u64) -> Server {
    drop(server);
    let (mut server, _) = accept(listener).await;
    let resumed = replied(&mut server, "session.resume", json!({"session_id": "live2"})).await;
    assert_eq!(resumed, json!({"session_id": STORED}));
    replied(&mut server, "session.events.since", since(epoch, json!([]))).await;

    server
}

/// Serves the bridge on pipes to Hermes on `port` and runs `test` as its
/// client, with what the bridge sends the client.
async fn with_bridge(port: u16, test: impl AsyncFnOnce(ConnectionTo<Agent>, mpsc::UnboundedReceiver<Seen>)) {
    let (client_out, bridge_in) = tokio::io::duplex(1 << 16);
    let (bridge_out, client_in) = tokio::io::duplex(1 << 16);
    tokio::spawn(serve(bridge_in, bridge_out, "127.0.0.1".to_owned(), port, SecretText::new("t")));
    let (seen_tx, seen) = mpsc::unbounded_channel();
    let (oversized, _) = mpsc::channel(1);

    Builder::new(Client)
        .on_receive_dispatch(
            async move |dispatch: Dispatch, _cx| {
                let seen = match dispatch {
                    Dispatch::Notification(message) => Seen::Update(message.params()["update"].clone()),
                    Dispatch::Request(message, responder) => {
                        Seen::Asked(message.method().to_owned(), message.params().clone(), Box::new(responder))
                    }
                    dispatch => return Ok(Handled::No { message: dispatch, retry: false }),
                };
                let _ = seen_tx.send(seen);
                Ok(Handled::Yes)
            },
            on_receive_dispatch!(),
        )
        .connect_with(acp::transport(client_out, client_in, oversized), async |cx: ConnectionTo<Agent>| {
            // A regression fails rather than hangs.
            tokio::time::timeout(Duration::from_secs(10), test(cx, seen)).await.expect("the bridge test timed out");
            Ok(())
        })
        .await
        .unwrap();
}

async fn call(cx: &ConnectionTo<Agent>, method: &str, params: Value) -> Value {
    cx.send_request(UntypedMessage::new(method, params).unwrap()).block_task().await.unwrap()
}

async fn initialized(cx: &ConnectionTo<Agent>) {
    let init = call(cx, "initialize", json!({"protocolVersion": 1, "clientCapabilities": {"elicitation": {"form": {}}}})).await;
    assert_eq!(init["agentInfo"]["name"], "Hermes");
    assert_eq!(init["agentCapabilities"]["loadSession"], true);
    assert_eq!(init["authMethods"].as_array().map_or(0, Vec::len), 0);
}

/// A new session; its id is Hermes's stored one.
async fn new_session(cx: &ConnectionTo<Agent>) -> Value {
    initialized(cx).await;
    let made = call(cx, "session/new", json!({"cwd": "/srv/project", "mcpServers": []})).await;
    assert_eq!(made["sessionId"], STORED);

    made["sessionId"].clone()
}

fn prompt(cx: &ConnectionTo<Agent>, session: &Value, text: &str) -> impl Future<Output = Value> + use<> {
    let request = json!({"sessionId": session, "prompt": [{"type": "text", "text": text}]});
    let sent = cx.send_request(UntypedMessage::new("session/prompt", request).unwrap()).block_task();
    async move { sent.await.unwrap() }
}

/// The next request to the client, keeping the updates sent before it.
async fn next_ask(seen: &mut mpsc::UnboundedReceiver<Seen>, updates: &mut Vec<Value>) -> (String, Value, Box<Responder>) {
    loop {
        match seen.recv().await.unwrap() {
            Seen::Update(update) => updates.push(update),
            Seen::Asked(method, params, responder) => return (method, params, responder),
        }
    }
}

async fn next_update(seen: &mut mpsc::UnboundedReceiver<Seen>) -> Value {
    match seen.recv().await.unwrap() {
        Seen::Update(update) => update,
        Seen::Asked(method, ..) => panic!("asked {method}, not an update"),
    }
}

fn text_of(update: &Value) -> &str {
    update["content"]["text"].as_str().unwrap_or_default()
}

#[tokio::test]
async fn a_prompt_streams_text_and_ends() {
    let (listener, port) = listen().await;
    let fake = tokio::spawn(async move {
        let (mut server, _) = accept(&listener).await;
        created(&mut server).await;
        let submitted = replied(&mut server, "prompt.submit", json!({})).await;
        assert_eq!(submitted, json!({"session_id": "live1", "text": "Say hi"}));
        server.send(hermes_event("live1", "message.delta", json!({"text": "Hi"}), 3)).await.unwrap();
        server.send(hermes_event("other", "message.delta", json!({"text": "not ours"}), 4)).await.unwrap();
        server.send(hermes_event("live1", "message.complete", json!({"text": "Hi", "status": "complete"}), 5)).await.unwrap();
        server
    });

    with_bridge(port, async |cx, mut seen| {
        let session = new_session(&cx).await;

        let ended = prompt(&cx, &session, "Say hi").await;

        assert_eq!(ended["stopReason"], "end_turn");
        let update = next_update(&mut seen).await;
        assert_eq!((update["sessionUpdate"].as_str(), text_of(&update)), (Some("agent_message_chunk"), "Hi"));
        assert!(seen.try_recv().is_err());
    })
    .await;
    fake.await.unwrap();
}

#[tokio::test]
async fn a_cancel_interrupts_the_turn_and_it_ends_cancelled() {
    let (listener, port) = listen().await;
    let fake = tokio::spawn(async move {
        let (mut server, _) = accept(&listener).await;
        created(&mut server).await;
        replied(&mut server, "prompt.submit", json!({})).await;
        server.send(hermes_event("live1", "message.delta", json!({"text": "Working"}), 3)).await.unwrap();
        let interrupted = replied(&mut server, "session.interrupt", json!({})).await;
        assert_eq!(interrupted, json!({"session_id": "live1"}));
        server.send(hermes_event("live1", "message.complete", json!({"status": "interrupted"}), 4)).await.unwrap();
        server
    });

    with_bridge(port, async |cx, mut seen| {
        let session = new_session(&cx).await;
        let running = prompt(&cx, &session, "Count to a million");

        next_update(&mut seen).await;
        let cancel = UntypedMessage::new("session/cancel", json!({"sessionId": session})).unwrap();
        cx.send_notification(cancel).unwrap();

        assert_eq!(running.await["stopReason"], "cancelled");
    })
    .await;
    fake.await.unwrap();
}

#[tokio::test]
async fn a_clarify_round_trips_through_the_client() {
    let (listener, port) = listen().await;
    let fake = tokio::spawn(async move {
        let (mut server, _) = accept(&listener).await;
        created(&mut server).await;
        replied(&mut server, "prompt.submit", json!({})).await;
        server.send(server_request(&clarify("srq-1"))).await.unwrap();
        let answered = read(&mut server).await;
        assert_eq!(answered, json!({"jsonrpc": "2.0", "id": "srq-1", "result": {"answers": {"q0": "Blue"}}}));
        server.send(hermes_event("live1", "message.complete", json!({"status": "complete"}), 3)).await.unwrap();
        server
    });

    with_bridge(port, async |cx, mut seen| {
        let session = new_session(&cx).await;
        let running = prompt(&cx, &session, "Pick a color");

        let (method, form, responder) = next_ask(&mut seen, &mut Vec::new()).await;
        assert_eq!(method, "elicitation/create");
        assert_eq!(form["sessionId"], STORED);
        assert_eq!(form["requestedSchema"]["properties"]["q0"]["oneOf"][2]["const"], "Blue");
        responder.respond(json!({"action": "accept", "content": {"q0": "Blue"}})).unwrap();

        assert_eq!(running.await["stopReason"], "end_turn");
    })
    .await;
    fake.await.unwrap();
}

#[tokio::test]
async fn an_ask_the_client_cannot_take_is_answered_at_once() {
    let (listener, port) = listen().await;
    let fake = tokio::spawn(async move {
        let (mut server, _) = accept(&listener).await;
        created(&mut server).await;
        let empty = json!({"id": "srq-8", "method": "clarify", "params": {"session_id": "live1", "questions": []}});
        server.send(server_request(&empty)).await.unwrap();
        let unknown = json!({"id": "srq-9", "method": "sudo.password", "params": {"session_id": "live1"}});
        server.send(server_request(&unknown)).await.unwrap();

        let cancelled = read(&mut server).await;
        assert_eq!(cancelled, json!({"jsonrpc": "2.0", "id": "srq-8", "result": {"answers": null, "outcome": "cancelled"}}));
        let refused = read(&mut server).await;
        assert_eq!((refused["id"].as_str(), refused["error"]["code"].as_i64()), (Some("srq-9"), Some(-32601)));
    });

    with_bridge(port, async |cx, mut seen| {
        new_session(&cx).await;

        fake.await.unwrap();

        assert!(seen.try_recv().is_err());
    })
    .await;
}

#[tokio::test]
async fn a_dropped_socket_reconnects_resumes_and_asks_only_what_is_new() {
    let (listener, port) = listen().await;
    let (form_shown, shown) = tokio::sync::oneshot::channel::<()>();
    let fake = tokio::spawn(async move {
        let (mut server, _) = accept(&listener).await;
        created(&mut server).await;
        replied(&mut server, "prompt.submit", json!({})).await;
        server.send(hermes_event("live1", "message.delta", json!({"text": "Let me ask"}), 3)).await.unwrap();
        server.send(server_request(&clarify("srq-1"))).await.unwrap();
        shown.await.unwrap();
        drop(server);

        let (mut server, _) = accept(&listener).await;
        let resumed = replied(&mut server, "session.resume", json!({"session_id": "live2"})).await;
        assert_eq!(resumed, json!({"session_id": STORED}));
        let approval = json!({"id": "srq-2", "method": "approval", "params": {"session_id": "live2", "request_id": "ap1", "command": "ls", "allow_session": false, "allow_permanent": false}});
        let missed = json!({"type": "message.delta", "session_id": "live2", "payload": {"text": " for help"}, "seq": 4});
        let since = json!({"count": 1, "epoch": 1, "events": [missed], "latest_seq": 4, "open_requests": [clarify("srq-1"), approval], "truncated": false});
        let asked_since = replied(&mut server, "session.events.since", since).await;
        assert_eq!(asked_since, json!({"session_id": "live2", "last_seen": 3}));

        let mut answers = Vec::new();
        for _ in 0..2 {
            let answer = read(&mut server).await;
            answers.push((answer["id"].as_str().unwrap().to_owned(), answer["result"].clone()));
        }
        answers.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(answers, [("srq-1".to_owned(), json!({"answers": {"q0": "Blue"}})), ("srq-2".to_owned(), json!({"choice": "once"}))]);
        server.send(hermes_event("live2", "message.complete", json!({"status": "complete"}), 5)).await.unwrap();
        server
    });

    with_bridge(port, async |cx, mut seen| {
        let session = new_session(&cx).await;
        let running = prompt(&cx, &session, "Help me pick");
        let mut updates = Vec::new();

        let (method, _, form) = next_ask(&mut seen, &mut updates).await;
        assert_eq!(method, "elicitation/create");
        form_shown.send(()).unwrap();

        // The form still open is not asked again; the new approval is.
        let (method, request, permission) = next_ask(&mut seen, &mut updates).await;
        assert_eq!(method, "session/request_permission");
        assert_eq!(request["sessionId"], STORED);
        permission.respond(json!({"outcome": {"outcome": "selected", "optionId": "once"}})).unwrap();
        form.respond(json!({"action": "accept", "content": {"q0": "Blue"}})).unwrap();

        assert_eq!(running.await["stopReason"], "end_turn");
        let said: Vec<&str> = updates.iter().map(text_of).collect();
        assert_eq!(said, ["Let me ask", " for help"]);
    })
    .await;
    fake.await.unwrap();
}

#[tokio::test]
async fn hermes_down_after_a_drop_is_tried_again_until_it_is_back() {
    let (listener, port) = listen().await;
    let fake = tokio::spawn(async move {
        let (mut server, _) = accept(&listener).await;
        created(&mut server).await;
        replied(&mut server, "prompt.submit", json!({})).await;
        drop(server);

        // Hermes restarting: the first try finds no socket.
        let (refused, _) = listener.accept().await.unwrap();
        drop(refused);

        let (mut server, _) = accept(&listener).await;
        let resumed = replied(&mut server, "session.resume", json!({"session_id": "live2"})).await;
        assert_eq!(resumed, json!({"session_id": STORED}));
        // The same epoch: Hermes kept the turn running.
        replied(&mut server, "session.events.since", since(1, json!([]))).await;
        server.send(hermes_event("live2", "message.complete", json!({"status": "complete"}), 1)).await.unwrap();
        server
    });

    with_bridge(port, async |cx, _seen| {
        let session = new_session(&cx).await;

        assert_eq!(prompt(&cx, &session, "Keep going").await["stopReason"], "end_turn");
    })
    .await;
    fake.await.unwrap();
}

#[tokio::test]
async fn another_sessions_requests_are_neither_asked_nor_answered() {
    let (listener, port) = listen().await;
    let fake = tokio::spawn(async move {
        let (mut server, _) = accept(&listener).await;
        created(&mut server).await;
        replied(&mut server, "prompt.submit", json!({})).await;
        let approval = json!({"id": "srq-7", "method": "approval", "params": {"session_id": "other", "request_id": "ap7", "command": "rm -rf /"}});
        let unknown = json!({"id": "srq-8", "method": "sudo.password", "params": {"session_id": "other"}});
        server.send(server_request(&approval)).await.unwrap();
        server.send(server_request(&unknown)).await.unwrap();
        server.send(hermes_event("live1", "message.complete", json!({"status": "complete"}), 3)).await.unwrap();

        // The next thing the bridge sends is the next prompt, not an answer.
        let next = replied(&mut server, "prompt.submit", json!({})).await;
        assert_eq!(next["text"], "Again");
        server.send(hermes_event("live1", "message.complete", json!({"status": "complete"}), 4)).await.unwrap();
        server
    });

    with_bridge(port, async |cx, mut seen| {
        let session = new_session(&cx).await;

        assert_eq!(prompt(&cx, &session, "Go").await["stopReason"], "end_turn");
        assert_eq!(prompt(&cx, &session, "Again").await["stopReason"], "end_turn");

        assert!(seen.try_recv().is_err());
    })
    .await;
    fake.await.unwrap();
}

#[tokio::test]
async fn a_restarted_hermes_ends_the_pending_prompt_cancelled() {
    let (listener, port) = listen().await;
    let fake = tokio::spawn(async move {
        let (mut server, _) = accept(&listener).await;
        created(&mut server).await;
        replied(&mut server, "prompt.submit", json!({})).await;
        dropped_and_resumed(&listener, server, 2).await
    });

    with_bridge(port, async |cx, _seen| {
        let session = new_session(&cx).await;

        assert_eq!(prompt(&cx, &session, "Keep going").await["stopReason"], "cancelled");
    })
    .await;
    fake.await.unwrap();
}

#[tokio::test]
async fn a_cancel_hermes_cannot_carry_out_ends_the_prompt_cancelled() {
    let (listener, port) = listen().await;
    let fake = tokio::spawn(async move {
        let (mut server, _) = accept(&listener).await;
        created(&mut server).await;
        replied(&mut server, "prompt.submit", json!({})).await;
        server.send(hermes_event("live1", "message.delta", json!({"text": "Working"}), 3)).await.unwrap();
        let interrupt = read(&mut server).await;
        assert_eq!(interrupt["method"], "session.interrupt");
        let error = json!({"jsonrpc": "2.0", "id": interrupt["id"], "error": {"code": 4009, "message": "no turn running"}});
        server.send(frame(error)).await.unwrap();
        server
    });

    with_bridge(port, async |cx, mut seen| {
        let session = new_session(&cx).await;
        let running = prompt(&cx, &session, "Count to a million");

        next_update(&mut seen).await;
        cx.send_notification(UntypedMessage::new("session/cancel", json!({"sessionId": session})).unwrap()).unwrap();

        assert_eq!(running.await["stopReason"], "cancelled");
    })
    .await;
    fake.await.unwrap();
}

#[tokio::test]
async fn loading_a_stored_session_resumes_it_and_asks_what_is_open() {
    let (listener, port) = listen().await;
    let fake = tokio::spawn(async move {
        let (mut server, _) = accept(&listener).await;
        let resumed = replied(&mut server, "session.resume", json!({"session_id": "live9"})).await;
        assert_eq!(resumed, json!({"session_id": STORED}));
        let earlier = json!({"type": "message.delta", "session_id": "live9", "payload": {"text": "Earlier"}, "seq": 7});
        let since = json!({"count": 1, "epoch": 1, "events": [earlier], "latest_seq": 7, "open_requests": [clarify("srq-5")], "truncated": false});
        let asked_since = replied(&mut server, "session.events.since", since).await;
        assert_eq!(asked_since, json!({"session_id": "live9", "last_seen": 0}));

        let answered = read(&mut server).await;
        assert_eq!(answered, json!({"jsonrpc": "2.0", "id": "srq-5", "result": {"answers": {"q0": "Blue"}}}));
        server
    });

    with_bridge(port, async |cx, mut seen| {
        initialized(&cx).await;

        call(&cx, "session/load", json!({"sessionId": STORED, "cwd": "/srv/project", "mcpServers": []})).await;

        let mut updates = Vec::new();
        let (method, form, responder) = next_ask(&mut seen, &mut updates).await;
        assert_eq!(updates.iter().map(text_of).collect::<Vec<_>>(), ["Earlier"]);
        assert_eq!((method.as_str(), &form["sessionId"]), ("elicitation/create", &json!(STORED)));
        responder.respond(json!({"action": "accept", "content": {"q0": "Blue"}})).unwrap();
        fake.await.unwrap();
    })
    .await;
}

#[tokio::test]
async fn the_node_starts_the_hermes_assistant_without_beeps_brief_or_rules() {
    use crate::agents::{Agents, Begin, HermesLink};
    let (listener, port) = listen().await;
    let fake = tokio::spawn(async move {
        let (mut server, _) = accept(&listener).await;
        created(&mut server).await;
        replied(&mut server, "prompt.submit", json!({})).await
    });
    let agents = Agents::new(Vec::new());
    agents.set_instructions(Some("Saved rules for coding agents".into()));
    agents.set_hermes(Some(HermesLink { host: "127.0.0.1".into(), port, token: SecretText::new("t") }));
    let kind = agents.named(beep_proto::node::agent::HERMES_GATEWAY).expect("the hidden kind");
    let begin = Begin {
        prompt: Some("Say hi".into()),
        brief: Some("Beep's brief".into()),
        ..Begin::default()
    };

    let _session = agents.start(&kind, Some("/"), begin).unwrap();

    let submitted = tokio::time::timeout(Duration::from_secs(10), fake).await.expect("Hermes is prompted").unwrap();
    assert_eq!(submitted, json!({"session_id": "live1", "text": "Say hi"}));
}
