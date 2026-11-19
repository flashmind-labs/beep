// The fake agents are sh scripts, so most tests run on Unix only.
#![cfg_attr(windows, allow(dead_code, unused_imports))]

use super::*;
use super::{
    orders::{choose, queue_room},
    turns::take_steers,
};
use crate::agents::{
    acp,
    compaction::{COMPACT_PROMPT, COMPACTED_ENTRY},
};
use beep_proto::node::{
    agent::{Alert, AlertKind, OptionKind, StopReason, TaskState},
    harness::{AutoCompact, ModelCompact},
};
use beep_proto::watched;
use beep_proto::{MAX_AGENT_IMAGES, Upload};
use serde_json::Value;
use std::path::PathBuf;
use std::time::Duration;

fn options() -> Vec<AnswerOption> {
    let option = |id: &str, name: &str, kind| AnswerOption {
        id: id.into(),
        name: name.into(),
        kind,
    };
    vec![
        option("allow", "Allow", OptionKind::AllowOnce),
        option("always", "Always Allow", OptionKind::AllowAlways),
        option("reject", "Reject", OptionKind::RejectOnce),
    ]
}

/// The shell helpers fake adapters share: `reply` answers the request
/// last read into `$line` with the id it carried.
const REPLY: &str = r#"
log() { printf '%s\n' "$line" >> "$1"; }
id() { printf '%s\n' "$line" | sed -nE 's/.*"id":("[^"]*"|[0-9]+).*/\1/p'; }
reply() { printf '{"jsonrpc":"2.0","id":%s,"result":%s}\n' "$(id)" "$1"; }
handshake() {
    read -r line; log "$1"
    reply '{"protocolVersion":1,"agentCapabilities":{}}'
    read -r line; log "$1"
    reply '{"sessionId":"s"}'
}
"#;

/// An ACP adapter in a few lines of shell: it can resume sessions and
/// appends each line it reads to the file in `$1`. On the prompt it asks
/// permission once, then ends the turn.
const FAKE_ADAPTER: &str = r#"
read -r line; log "$1"
reply '{"protocolVersion":1,"agentCapabilities":{"loadSession":true,"sessionCapabilities":{"resume":{}}}}'
read -r line; log "$1"
reply '{}'
read -r line; log "$1"
reply '{}'
read -r line; log "$1"
prompt=$(id)
echo '{"jsonrpc":"2.0","id":"p1","method":"session/request_permission","params":{"sessionId":"s","toolCall":{"toolCallId":"t1","title":"Run cargo test"},"options":[{"optionId":"allow","name":"Allow","kind":"allow_once"},{"optionId":"reject","name":"Reject","kind":"reject_once"}]}}'
read -r line; log "$1"
echo '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"Picked it up."}}}}'
printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$prompt"
cat > /dev/null
"#;

/// An adapter that puts a choice to the user on its first prompt, as a
/// pi extension's select does, and waits.
const ASKING_ADAPTER: &str = r#"
handshake "$1"
read -r line; log "$1"
echo '{"jsonrpc":"2.0","id":"p1","method":"session/request_permission","params":{"sessionId":"s","toolCall":{"toolCallId":"pi-ui-1","title":"Which file?"},"options":[{"optionId":"choice-0","name":"notes.txt","kind":"allow_once"},{"optionId":"choice-1","name":"other.txt","kind":"allow_once"}]}}'
cat > /dev/null
"#;

fn fake(name: &str, log: &Path) -> AgentKind {
    script(name, FAKE_ADAPTER, log)
}

fn script(name: &str, script: &str, log: &Path) -> AgentKind {
    AgentKind {
        name: name.into(),
        program: "sh".into(),
        args: vec![
            "-c".into(),
            format!("{REPLY}{script}"),
            "fake-adapter".into(),
            log.display().to_string(),
        ],
        env: Default::default(),
        compact: None,
        compact_models: Vec::new(),
        default_model: None,
    }
}

async fn settle(session: &Session) {
    let mut live = session.live.clone();
    tokio::time::timeout(
        Duration::from_secs(10),
        watched::wait_for(&mut live, |live| {
            live.info.turns >= 1 || matches!(live.status(), Status::Ended | Status::Asking)
        }),
    )
    .await
    .expect("the session settles")
    .unwrap();
}

#[tokio::test(start_paused = true)]
async fn a_slow_start_waits_for_acps_id() {
    let (tx, mut rx) = watch::channel(Live::default());
    let named = named_info(&mut rx);
    tokio::pin!(named);
    assert!(futures::poll!(named.as_mut()).is_pending());
    tokio::time::advance(Duration::from_secs(60)).await;
    assert!(futures::poll!(named.as_mut()).is_pending());
    tx.send_modify(|live| {
        live.agent_session = Some(SessionId::from("acp-session").to_owned());
        live.info.id = SessionId::from("acp-session").to_owned();
    });
    assert_eq!(named.await.unwrap().id, "acp-session");
}

#[tokio::test]
async fn a_failed_start_returns_its_error_without_a_session_id() {
    let (tx, mut rx) = watch::channel(Live::default());
    let named = named_info(&mut rx);
    tokio::pin!(named);
    assert!(futures::poll!(named.as_mut()).is_pending());
    tx.send_modify(|live| {
        live.info.state.status = Status::Ended;
        live.info.state.error = Some("ACP initialization failed".to_owned());
    });
    assert_eq!(named.await.unwrap_err(), "ACP initialization failed");
}

#[tokio::test]
async fn a_dropped_start_returns_an_error_without_a_session_id() {
    let (tx, mut rx) = watch::channel(Live::default());
    drop(tx);
    assert_eq!(
        named_info(&mut rx).await.unwrap_err(),
        "the agent stopped before giving a session id"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn a_takeover_resumes_without_permission_prompts() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();
    let log = dir.join("sent.jsonl");
    let id = "0f3c2a9e-1111-4222-8333-944455556666";
    let session = Session::start(Start {
        prompt: Some(String::from("carry on").into()),
        resume: Some(id.into()),
        earlier: Vec::new(),
        ..start_on(&fake("claude", &log), dir, None)
    })
    .unwrap();
    settle(&session).await;
    let info = session.info();
    assert_eq!(info.turns, 1, "{:?}", info.state.error);
    assert_eq!(info.last_message, "Picked it up.");
    assert!(info.question.is_none());
    assert_eq!(
        watched::read(&session.live, |live| live.agent_session.clone()),
        Some(SessionId::from(id))
    );
    let sent: Vec<Value> = std::fs::read_to_string(&log)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(sent[1]["method"], "session/load");
    assert_eq!(sent[1]["params"]["sessionId"], id);
    assert_eq!(sent[1]["params"]["cwd"], dir.display().to_string());
    assert_eq!(sent[2]["method"], "session/set_mode");
    assert_eq!(sent[2]["params"]["modeId"], "bypassPermissions");
    assert_eq!(
        sent[3]["method"], "session/prompt",
        "the prompt waits for the mode"
    );
    // The stray permission request was allowed, not asked.
    assert_eq!(sent[4]["id"], "p1");
    assert_eq!(sent[4]["result"]["outcome"]["optionId"], "allow");
    let (entries, _) = watched::read(&session.live, |live| live.transcript(10));
    assert!(
        entries
            .iter()
            .any(|entry| entry.text == "Allowed: Run cargo test")
    );
}

#[cfg(unix)]
#[tokio::test]
async fn the_first_prompt_carries_the_brief() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();
    let log = dir.join("sent.jsonl");
    let kind = script("codex", PLAIN_ADAPTER, &log);
    let session = Session::start(Start {
        brief: Some("Worktree first.".into()),
        ..start_on(&kind, dir, None)
    })
    .unwrap();
    settle(&session).await;
    assert_eq!(session.info().turns, 1, "{:?}", session.info().state.error);
    let sent = sent(&log);
    assert_eq!(sent[2]["method"], "session/prompt");
    let blocks: Vec<&str> = sent[2]["params"]["prompt"]
        .as_array()
        .unwrap()
        .iter()
        .map(|block| block["text"].as_str().unwrap())
        .collect();
    assert_eq!(
        blocks,
        [
            crate::agents::tags::context("Worktree first.").as_str(),
            "go"
        ]
    );
}

/// An adapter that takes a system prompt, as Claude Code's and Codex's do:
/// it makes or loads a session and ends its first turn at once.
const SYSTEM_PROMPT_ADAPTER: &str = r#"
read -r line; log "$1"
reply '{"protocolVersion":1,"agentCapabilities":{"loadSession":true,"sessionCapabilities":{"systemPrompt":{}}}}'
read -r line; log "$1"
reply '{"sessionId":"s"}'
read -r line; log "$1"
reply '{"stopReason":"end_turn"}'
cat > /dev/null
"#;

fn beep_server() -> McpServer {
    McpServer::Stdio(agent_client_protocol::v1::schema::McpServerStdio::new(
        "beep",
        "/bin/true",
    ))
}

/// The text of each block of the prompt `sent`.
fn prompt_texts(sent: &Value) -> Vec<&str> {
    sent["params"]["prompt"]
        .as_array()
        .unwrap()
        .iter()
        .map(|block| block["text"].as_str().unwrap())
        .collect()
}

#[cfg(unix)]
#[tokio::test]
async fn an_agent_that_takes_a_system_prompt_gets_the_brief_in_session_new() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();
    let log = dir.join("sent.jsonl");
    let kind = script("codex", SYSTEM_PROMPT_ADAPTER, &log);
    let session = Session::start(Start {
        brief: Some("Worktree first.".into()),
        mcp: Some(beep_server()),
        ..start_on(&kind, dir, None)
    })
    .unwrap();
    settle(&session).await;
    assert_eq!(session.info().turns, 1, "{:?}", session.info().state.error);
    let sent = sent(&log);
    assert_eq!(sent[1]["method"], "session/new");
    let brief = format!(
        "Worktree first.\n\n{}\n\n{}",
        crate::agents::mcp::BRIEF,
        crate::agents::mcp::CHAT
    );
    assert_eq!(
        sent[1]["params"]["systemPrompt"],
        serde_json::json!({"mode": "append", "content": [{"type": "text", "text": brief}]})
    );
    assert_eq!(sent[2]["method"], "session/prompt");
    assert_eq!(prompt_texts(&sent[2]), ["go"]);
}

#[cfg(unix)]
#[tokio::test]
async fn a_resumed_session_gets_the_tools_in_session_load_and_the_brief_in_its_prompt() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();
    let log = dir.join("sent.jsonl");
    let kind = script("codex", SYSTEM_PROMPT_ADAPTER, &log);
    let session = Session::start(Start {
        resume: Some("s".into()),
        brief: Some("This session moved.".into()),
        mcp: Some(beep_server()),
        ..start_on(&kind, dir, None)
    })
    .unwrap();
    settle(&session).await;
    assert_eq!(session.info().turns, 1, "{:?}", session.info().state.error);
    let sent = sent(&log);
    assert_eq!(sent[1]["method"], "session/load");
    assert_eq!(
        sent[1]["params"]["systemPrompt"],
        serde_json::json!({
            "mode": "append",
            "content": [{"type": "text", "text": format!("{}\n\n{}", crate::agents::mcp::BRIEF, crate::agents::mcp::CHAT)}],
        })
    );
    assert_eq!(
        prompt_texts(&sent[2]),
        [
            crate::agents::tags::context("This session moved.").as_str(),
            "go"
        ]
    );
}

#[cfg(unix)]
#[tokio::test]
async fn an_agent_without_a_system_prompt_gets_the_whole_brief_in_its_first_prompt() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();
    let log = dir.join("sent.jsonl");
    let kind = script("pi", PLAIN_ADAPTER, &log);
    let session = Session::start(Start {
        brief: Some("Worktree first.".into()),
        mcp: Some(beep_server()),
        ..start_on(&kind, dir, None)
    })
    .unwrap();
    settle(&session).await;
    let sent = sent(&log);
    assert!(sent[1]["params"].get("systemPrompt").is_none());
    let brief = format!(
        "Worktree first.\n\n{}\n\n{}",
        crate::agents::mcp::BRIEF,
        crate::agents::mcp::CHAT
    );
    assert_eq!(
        prompt_texts(&sent[2]),
        [crate::agents::tags::context(&brief).as_str(), "go"]
    );
}

/// An adapter that starts a session and ends its first turn at once.
const PLAIN_ADAPTER: &str = r#"
handshake "$1"
read -r line; log "$1"
reply '{"stopReason":"end_turn"}'
cat > /dev/null
"#;

#[cfg(unix)]
#[tokio::test]
async fn a_question_of_the_nodes_takes_an_option_or_any_words() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();
    let log = dir.join("sent.jsonl");
    let session = Session::start(Start {
        prompt: Some(String::from("hello").into()),
        ..start_on(&script("fake", PLAIN_ADAPTER, &log), dir, None)
    })
    .unwrap();
    settle(&session).await;
    let ask = |reply| Command::Ask {
        question: Asking {
            text: "Which branch?".into(),
            detail: None,
            options: options(),
        },
        reply,
    };
    let (reply, answer) = oneshot::channel();
    session.order(ask(reply)).await.unwrap();
    let shown = session.info().question.expect("the question shows");
    assert_eq!(shown.text, "Which branch?");
    assert_eq!(session.info().state.status, Status::Asking);
    session
        .order(Command::Answer {
            question: shown.id,
            reply: "always".into(),
        })
        .await
        .unwrap();
    assert_eq!(
        answer.await.unwrap(),
        Answered::Picked(options()[1].clone())
    );
    assert!(session.info().question.is_none());
    let (reply, answer) = oneshot::channel();
    session.order(ask(reply)).await.unwrap();
    let shown = session.info().question.expect("the question shows");
    session
        .order(Command::Answer {
            question: shown.id,
            reply: "the release branch".into(),
        })
        .await
        .unwrap();
    assert_eq!(
        answer.await.unwrap(),
        Answered::Said("the release branch".into())
    );
    // Nothing went to the agent: the node asked, not the agent.
    assert_eq!(sent(&log).len(), 3);
}

#[cfg(unix)]
#[tokio::test]
async fn a_choice_is_still_asked() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();
    let log = dir.join("sent.jsonl");
    let session = Session::start(start_on(&script("pi", ASKING_ADAPTER, &log), dir, None)).unwrap();
    settle(&session).await;
    let info = session.info();
    assert_eq!(info.state.status, Status::Asking);
    let question = info.question.expect("a question");
    assert_eq!(question.text, "Which file?");
    assert_eq!(question.options.len(), 2);
    let sent = std::fs::read_to_string(&log).unwrap();
    assert!(!sent.contains("session/set_mode"));
}

#[cfg(unix)]
#[tokio::test]
async fn a_declining_answer_waits_only_where_there_is_room() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();
    let log = dir.join("sent.jsonl");
    let session = Session::start(start_on(&script("pi", ASKING_ADAPTER, &log), dir, None)).unwrap();
    settle(&session).await;
    let question = session.info().question.expect("a question").id;

    for n in 0..MAX_QUEUED {
        let prompt = Command::Prompt(format!("later {n}").into());
        session.order(prompt).await.unwrap();
    }

    let answer = Command::Answer {
        question,
        reply: "neither, look in docs".into(),
    };
    assert!(session.order(answer).await.is_err());
    assert_eq!(session.info().state.queued as usize, MAX_QUEUED);
    assert!(session.info().question.is_some(), "still open");
    session.close().await;
}

#[cfg(unix)]
#[tokio::test]
async fn a_declining_answer_counts_steers_against_the_queue_limit() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();
    let log = dir.join("sent.jsonl");
    let session = Session::start(start_on(&script("pi", ASKING_ADAPTER, &log), dir, None)).unwrap();
    settle(&session).await;
    let question = session.info().question.expect("a question").id;

    for n in 0..MAX_QUEUED - 1 {
        let prompt = Command::Prompt(format!("later {n}").into());
        session.order(prompt).await.unwrap();
    }

    let steer = Command::Steer("correction".to_owned().into());
    session.order(steer).await.unwrap();

    let answer = Command::Answer {
        question,
        reply: "neither, look in docs".into(),
    };
    assert!(session.order(answer).await.is_err());
    assert!(session.info().question.is_some(), "still open");
    session.close().await;
}

/// An adapter that asks two questions in one form, as Claude Code's
/// AskUserQuestion and Codex's questions reach the node (the user's own
/// words go in `_custom` for Claude Code, `_note` for Codex), logs the
/// answer and ends its turn.
const FORM_ADAPTER: &str = r#"
handshake "$1"
read -r line
prompt=$(id)
echo '{"jsonrpc":"2.0","id":"e1","method":"elicitation/create","params":{"mode":"form","sessionId":"s","message":"Please answer the following questions.","requestedSchema":{"type":"object","properties":{"question_0":{"type":"string","title":"Scope","description":"Which layout?","oneOf":[{"const":"Source","title":"Source"},{"const":"Screen","title":"Screen"}]},"question_0_custom":{"type":"string","title":"Other"},"question_1":{"type":"array","title":"Parts","description":"Which parts?","items":{"anyOf":[{"const":"Sidebar","title":"Sidebar"},{"const":"Composer","title":"Composer"}]}},"question_1_note":{"type":"string","title":"Additional answer or note"}}}}}'
read -r line; log "$1"
printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$prompt"
cat > /dev/null
"#;

#[cfg(unix)]
#[tokio::test]
async fn a_form_is_asked_field_by_field_and_sent_whole() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();
    let log = dir.join("sent.jsonl");
    let session = Session::start(start_on(&script("pi", FORM_ADAPTER, &log), dir, None)).unwrap();
    settle(&session).await;
    let first = session.info().question.expect("the first field");
    assert_eq!(first.text, "Which layout?");
    let names: Vec<&str> = first.options.iter().map(|o| o.name.as_str()).collect();
    assert_eq!(names, ["Source", "Screen", "Skip"]);

    let answer = Command::Answer {
        question: first.id,
        reply: "2".into(),
    };
    session.order(answer).await.unwrap();
    let second = session.info().question.expect("the second field");
    assert_eq!(second.text, "Which parts?");
    // Words typed while a form asks answer it.
    let words = Command::Prompt(String::from("Composer, Toolbar").into());
    session.order(words).await.unwrap();

    let mut live = session.live.clone();
    tokio::time::timeout(
        Duration::from_secs(10),
        watched::wait_for(&mut live, |live| live.info.turns >= 1),
    )
    .await
    .expect("the turn ends once the form is sent")
    .unwrap();
    let sent = sent(&log);
    let answer = sent.last().expect("the form's answer");
    assert_eq!(answer["id"], "e1");
    assert_eq!(
        answer["result"],
        serde_json::json!({
            "action": "accept",
            "content": {
                "question_0": "Screen",
                "question_1": ["Composer"],
                "question_1_note": "Toolbar",
            },
        })
    );
    session.close().await;
}

#[cfg(unix)]
#[tokio::test]
async fn codex_async_form_shows_the_question_without_the_timing_note() {
    const ADAPTER: &str = r#"
handshake "$1"
read -r line
echo '{"jsonrpc":"2.0","id":"e1","method":"elicitation/create","params":{"mode":"form","sessionId":"s","_meta":{"codex":{"delivery":"async"}},"message":"Codex keeps working while you answer.","requestedSchema":{"type":"object","properties":{"question_0":{"type":"string","title":"Which layout should I use?","oneOf":[{"const":"Wide","title":"Wide"},{"const":"Compact","title":"Compact"}]},"question_0_note":{"type":"string","title":"Additional answer or note"}}}}}'
cat > /dev/null
"#;
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();
    let session = Session::start(start_on(
        &script("codex", ADAPTER, &dir.join("sent.jsonl")),
        dir,
        None,
    ))
    .unwrap();
    settle(&session).await;

    let question = session.info().question.expect("the Codex question");
    assert_eq!(question.text, "Which layout should I use?");
    assert_eq!(question.detail, None);
    let names: Vec<&str> = question.options.iter().map(|o| o.name.as_str()).collect();
    assert_eq!(names, ["Wide", "Compact", "Skip"]);

    session.close().await;
}

/// An adapter that puts a choice to the user, then ends its turn without
/// waiting for it, logs the answer the node gives and exits.
const ABANDONING_ADAPTER: &str = r#"
read -r line; reply '{"protocolVersion":1,"agentCapabilities":{}}'
read -r line; reply '{"sessionId":"s"}'
read -r line
prompt=$(id)
echo '{"jsonrpc":"2.0","id":"p1","method":"session/request_permission","params":{"sessionId":"s","toolCall":{"toolCallId":"pi-ui-1","title":"Which file?"},"options":[{"optionId":"choice-0","name":"notes.txt","kind":"allow_once"}]}}'
printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$prompt"
read -r line; log "$1"
"#;

#[cfg(unix)]
#[tokio::test]
async fn a_question_left_open_by_a_turn_is_answered_cancelled() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();
    let log = dir.join("sent.jsonl");
    let session =
        Session::start(start_on(&script("pi", ABANDONING_ADAPTER, &log), dir, None)).unwrap();
    let mut live = session.live.clone();
    // The adapter exits once it hears the answer; without one it waits.
    tokio::time::timeout(
        Duration::from_secs(10),
        watched::wait_for(&mut live, |live| live.status() == Status::Ended),
    )
    .await
    .expect("the question was never answered")
    .unwrap();
    let sent = std::fs::read_to_string(&log).unwrap();
    let answer: Value = serde_json::from_str(sent.trim()).unwrap();
    assert_eq!(answer["id"], "p1");
    assert_eq!(answer["result"]["outcome"]["outcome"], "cancelled");
}

/// An adapter that answers with whatever id each request carries, then
/// writes a burst far larger than a pipe holds before it reads the
/// prompt. A node that stops reading while it writes deadlocks on it.
const BURSTING_ADAPTER: &str = r#"
read -r line; reply '{"protocolVersion":1,"agentCapabilities":{}}'
read -r line; reply '{"sessionId":"s"}'
chunk=$(head -c 2000 /dev/zero | tr '\0' x)
i=0
while [ $i -lt 2000 ]; do
printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"%s"}}}}\n' "$chunk"
i=$((i+1))
done
line=$(head -n 1)
reply '{"stopReason":"end_turn"}'
cat > /dev/null
"#;

#[cfg(unix)]
#[tokio::test]
async fn a_large_prompt_and_a_large_burst_do_not_deadlock() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();
    let log = dir.join("unused");
    let session = Session::start(Start {
        prompt: Some("y".repeat(4 * 1024 * 1024).into()),
        ..start_on(&script("codex", BURSTING_ADAPTER, &log), dir, None)
    })
    .unwrap();
    let mut live = session.live.clone();
    // The timeout only bounds a failure; the turn ending is the signal.
    tokio::time::timeout(
        Duration::from_secs(60),
        watched::wait_for(&mut live, |live| {
            live.info.turns >= 1 || live.status() == Status::Ended
        }),
    )
    .await
    .expect("the session hung: reading waited on writing")
    .unwrap();
    let info = session.info();
    assert_eq!(info.turns, 1, "{:?}", info.state.error);
    assert_eq!(info.last_message.len(), 4 * 1024, "the burst was read");
}

/// Starts the adapter `body` (after the handshake, with `$line` holding
/// the first prompt) as a session that asks permission.
fn started(dir: &Path, body: &str) -> Session {
    started_within(dir, body, STOP_WITHIN)
}

/// `started`, giving the agent `within` to end a turn after Stop.
fn started_within(dir: &Path, body: &str, within: Duration) -> Session {
    let handshake = r#"
read -r line; reply '{"protocolVersion":1,"agentCapabilities":{}}'
read -r line; reply '{"sessionId":"s"}'
read -r line
"#;
    Session::start_within(
        start_on(
            &script("codex", &format!("{handshake}{body}"), &dir.join("log")),
            dir,
            None,
        ),
        within,
    )
    .unwrap()
}

/// Waits until `done` holds. The timeout only bounds a failure.
async fn until(session: &Session, done: impl FnMut(&Live) -> bool) {
    let mut live = session.live.clone();
    tokio::time::timeout(Duration::from_secs(60), watched::wait_for(&mut live, done))
        .await
        .expect("the session got there")
        .unwrap();
}

const UPDATE: &str =
    r#"{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s","update":"#;

#[cfg(unix)]
#[tokio::test]
async fn an_oversized_message_is_dropped_and_the_session_goes_on() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();
    let body = format!(
        r#"
printf '%s{{"sessionUpdate":"agent_message_chunk","content":{{"type":"text","text":"' '{UPDATE}'
head -c {} /dev/zero | tr '\0' x
printf '"}}}}}}}}\n'
echo '{UPDATE}{{"sessionUpdate":"tool_call","toolCallId":"t1","title":"Still here"}}}}}}'
cat > /dev/null
"#,
        acp::MAX_MESSAGE + 1
    );
    let session = started(dir, &body);
    until(&session, |live| {
        live.info.state.summary == "Still here" || live.status() == Status::Ended
    })
    .await;
    let info = session.info();
    assert_eq!(info.state.status, Status::Working);
    let error = info.state.error.unwrap_or_default();
    assert!(error.contains("dropped a 16 MiB message"), "{error}");
}

#[cfg(unix)]
#[tokio::test]
async fn an_update_of_an_unknown_kind_is_skipped() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();
    let body = format!(
        r#"
echo '{UPDATE}{{"sessionUpdate":"something_new","detail":1}}}}}}'
echo '{UPDATE}{{"sessionUpdate":"agent_message_chunk","content":{{"type":"text","text":"done"}}}}}}}}'
reply '{{"stopReason":"end_turn"}}'
cat > /dev/null
"#
    );
    let session = started(dir, &body);
    until(&session, |live| {
        live.info.turns >= 1 || live.status() == Status::Ended
    })
    .await;
    let info = session.info();
    assert_eq!(info.turns, 1, "{:?}", info.state.error);
    assert_eq!(info.last_message, "done");
}

#[cfg(unix)]
#[tokio::test]
async fn closing_stops_the_agent_with_sigterm() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();
    let marker = dir.join("terminated");
    let body = format!(
        r#"
trap 'echo bye > "{}"; exit 0' TERM
echo '{UPDATE}{{"sessionUpdate":"tool_call","toolCallId":"t1","title":"Listening"}}}}}}'
while :; do sleep 1; done
"#,
        marker.display()
    );
    let session = started(dir, &body);
    until(&session, |live| live.info.state.summary == "Listening").await;
    session.close().await;
    assert_eq!(session.info().state.status, Status::Ended);
    assert_eq!(session.info().state.error, None, "closed, not failed");
    assert_eq!(std::fs::read_to_string(&marker).unwrap(), "bye\n");
}

/// An adapter that says whether it takes images (`$2`), then ends each
/// prompt's turn at once.
const IMAGES_ADAPTER: &str = r#"
read -r line; log "$1"
reply "{\"protocolVersion\":1,\"agentCapabilities\":{\"promptCapabilities\":{\"image\":$2}}}"
read -r line; log "$1"
reply '{"sessionId":"s"}'
while read -r line; do log "$1"; reply '{"stopReason":"end_turn"}'; done
"#;

/// A session of an agent that takes images or not, once it is idle.
async fn image_session(dir: &Path, images: bool) -> Session {
    let mut kind = script("codex", IMAGES_ADAPTER, &dir.join("sent.jsonl"));
    kind.args.push(images.to_string());
    let session = Session::start(Start {
        prompt: None,
        ..start_on(&kind, dir, None)
    })
    .unwrap();
    until(&session, |live| {
        matches!(live.status(), Status::Idle | Status::Ended)
    })
    .await;
    assert_eq!(session.info().state.status, Status::Idle);
    session
}

fn shot() -> Prompt {
    Prompt {
        text: "The button overlaps".into(),
        images: vec![Upload {
            name: "shot.png".into(),
            mime: "image/png".into(),
            data: b"\x89PNG\r\n\x1a\n".to_vec(),
        }],
    }
}

#[cfg(unix)]
#[tokio::test]
async fn images_go_to_an_agent_that_takes_them() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();
    let session = image_session(dir, true).await;
    session.order(Command::Prompt(shot())).await.unwrap();
    until(&session, |live| {
        live.info.turns >= 1 || live.status() == Status::Ended
    })
    .await;
    let sent: Vec<Value> = std::fs::read_to_string(dir.join("sent.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let prompt = &sent[2];
    assert_eq!(prompt["method"], "session/prompt");
    assert_eq!(prompt["params"]["prompt"][0]["text"], "The button overlaps");
    assert_eq!(prompt["params"]["prompt"][1]["type"], "image");
    assert_eq!(prompt["params"]["prompt"][1]["mimeType"], "image/png");
    let data = b"\x89PNG\r\n\x1a\n";
    let key = beep_proto::image_key(data);
    let (entries, _) = watched::read(&session.live, |live| live.transcript(10));
    let shown = format!("The button overlaps\n[image: image-{key}]");
    assert!(
        entries.iter().any(|entry| entry.text == shown),
        "{entries:?}"
    );
    let kept = watched::read(&session.live, |live| live.picture(&key));
    assert_eq!(
        kept.map(|picture| picture.data.clone()),
        Some(data.to_vec())
    );
}

#[cfg(unix)]
#[test]
fn the_queue_is_bounded_in_prompts_and_bytes() {
    let text = |len: usize| Prompt::from("x".repeat(len));
    let mut queued = VecDeque::new();
    assert_eq!(queue_room(&queued, &text(10)), Ok(()));

    let image = |len: usize| Prompt {
        text: String::new(),
        images: vec![Upload {
            name: "shot.png".into(),
            mime: "image/png".into(),
            data: vec![0; len],
        }],
    };
    queued.push_back(image(MAX_QUEUED_BYTES / 2));
    queued.push_back(image(MAX_QUEUED_BYTES / 2 - 10));
    assert_eq!(queue_room(&queued, &text(10)), Ok(()), "exactly the limit");
    let full = queue_room(&queued, &text(11)).unwrap_err();
    assert!(full.starts_with("Too much already waits"), "{full}");

    let many: VecDeque<_> = (0..MAX_QUEUED).map(|_| text(1)).collect();
    let full = queue_room(&many, &text(1)).unwrap_err();
    assert_eq!(
        full,
        format!("{MAX_QUEUED} prompts already wait for this turn")
    );
}

#[test]
fn a_reply_picks_an_option_by_id_number_name_or_word() {
    let options = options();
    let pick = |reply| choose(&options, reply).map(|option| option.id.as_str());
    assert_eq!(pick("reject"), Some("reject"));
    assert_eq!(pick("2"), Some("always"));
    assert_eq!(pick("Always Allow"), Some("always"));
    assert_eq!(pick("allow_once"), Some("allow"));
    assert_eq!(pick("yes"), Some("allow"));
    assert_eq!(pick("no"), Some("reject"));
    assert_eq!(pick("4"), None);
    assert_eq!(pick("use the other folder instead"), None);
}

/// An adapter that writes its arguments after `$1` to `$1.args`, offers
/// two models on `session/new`, answers a switch with
/// `$BEEP_FAKE_SWITCH` as its error (an empty result when unset), then
/// ends a turn.
const MODEL_ADAPTER: &str = r#"
out="$1"; shift; printf '%s\n' "$@" > "$out.args"; set -- "$out"
read -r line; log "$1"
reply '{"protocolVersion":1,"agentCapabilities":{}}'
read -r line; log "$1"
reply '{"sessionId":"s","models":{"availableModels":[{"modelId":"fast"},{"modelId":"deep"}],"currentModelId":"fast"}}'
read -r line; log "$1"
case "$line" in
*session/prompt*) ;;
*) if [ -n "$BEEP_FAKE_SWITCH" ]; then printf '{"jsonrpc":"2.0","id":%s,"error":%s}\n' "$(id)" "$BEEP_FAKE_SWITCH"; else reply '{}'; fi
 read -r line; log "$1";;
esac
printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$(id)"
cat > /dev/null
"#;

fn sent(log: &Path) -> Vec<Value> {
    std::fs::read_to_string(log)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

fn start_on<'a>(kind: &'a AgentKind, dir: &'a Path, model: Option<&str>) -> Start<'a> {
    Start {
        id: "c0ffee".into(),
        kind,
        cwd: dir,
        prompt: Some(String::from("go").into()),
        resume: None,
        earlier: Vec::new(),
        timestamps: None,
        title: None,
        model: model.map(ModelId::new),
        effort: None,
        fast: None,
        mode: None,
        parent: None,
        follows: None,
        note: None,
        worktrees: Vec::new(),
        brief: None,
        rule: None,
        mcp: None,
        assistant: false,
        images: Arc::default(),
        usage: Arc::default(),
        alerts: Arc::default(),
        stopping: Arc::default(),
    }
}

#[cfg(unix)]
#[tokio::test]
async fn a_new_session_switches_to_its_model_before_the_first_prompt() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();
    let log = dir.join("sent.jsonl");
    let kind = script("codex", MODEL_ADAPTER, &log);
    let session = Session::start(start_on(&kind, dir, Some("deep"))).unwrap();
    settle(&session).await;
    let info = session.info();
    assert_eq!(info.turns, 1, "{:?}", info.state.error);
    assert_eq!(info.model.as_ref().map(AsRef::as_ref), Some("deep"));
    let sent = sent(&log);
    assert_eq!(sent[1]["method"], "session/new");
    assert_eq!(sent[2]["method"], "session/set_model");
    assert_eq!(sent[2]["params"]["sessionId"], "s");
    assert_eq!(sent[2]["params"]["modelId"], "deep");
    assert_eq!(sent[3]["method"], "session/prompt", "the prompt waits");
    let args = std::fs::read_to_string(dir.join("sent.jsonl.args")).unwrap();
    assert_eq!(args.trim(), "", "no flag on the command line");
}

#[cfg(unix)]
#[tokio::test]
async fn hermes_selects_a_provider_model_and_edit_mode_before_prompting() {
    let temp = tempfile::tempdir().unwrap();
    let log = temp.path().join("sent.jsonl");
    let adapter = r#"
read -r line; log "$1"
reply '{"protocolVersion":1,"agentCapabilities":{"loadSession":true}}'
read -r line; log "$1"
reply '{"sessionId":"s","models":{"availableModels":[{"modelId":"openrouter:vendor/model-a","name":"Model A"},{"modelId":"custom:local:model-b","name":"Model B"}],"currentModelId":"openrouter:vendor/model-a"},"modes":{"currentModeId":"default","availableModes":[{"id":"default","name":"Default"},{"id":"accept_edits","name":"Accept Edits"},{"id":"dont_ask","name":"Do not ask"}]}}'
read -r line; log "$1"; reply '{}'
read -r line; log "$1"; reply '{}'
read -r line; log "$1"; reply '{"stopReason":"end_turn"}'
cat > /dev/null
"#;
    let kind = script("hermes", adapter, &log);
    let session =
        Session::start(start_on(&kind, temp.path(), Some("custom:local:model-b"))).unwrap();
    settle(&session).await;
    assert_eq!(session.info().turns, 1, "{:?}", session.info().state.error);
    let sent = sent(&log);
    assert_eq!(sent[1]["params"]["cwd"], temp.path().to_str().unwrap());
    assert_eq!(sent[2]["method"], "session/set_model");
    assert_eq!(sent[2]["params"]["modelId"], "custom:local:model-b");
    assert_eq!(sent[3]["method"], "session/set_mode");
    assert_eq!(sent[3]["params"]["modeId"], "dont_ask");
    assert_eq!(sent[4]["method"], "session/prompt");
    session.close().await;
}

/// An adapter that offers two models, ends its first turn, then
/// answers the switch that comes after.
const SWITCH_LATER_ADAPTER: &str = r#"
read -r line; log "$1"
reply '{"protocolVersion":1,"agentCapabilities":{}}'
read -r line; log "$1"
reply '{"sessionId":"s","models":{"availableModels":[{"modelId":"fast"},{"modelId":"deep"}],"currentModelId":"fast"}}'
read -r line; log "$1"
printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$(id)"
read -r line; log "$1"
reply '{}'
cat > /dev/null
"#;

#[cfg(unix)]
#[tokio::test]
async fn a_running_session_switches_its_model_for_the_next_turn() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();
    let log = dir.join("sent.jsonl");
    let kind = script("codex", SWITCH_LATER_ADAPTER, &log);
    let session = Session::start(start_on(&kind, dir, None)).unwrap();
    settle(&session).await;
    assert_eq!(
        session.info().model.as_ref().map(AsRef::as_ref),
        Some("fast")
    );
    // It cannot think at another level: refused, nothing sent.
    let refused = session
        .order(Command::Configure {
            model: None,
            effort: Some(Effort::High),
            fast: None,
        })
        .await;
    assert!(
        refused
            .unwrap_err()
            .contains("cannot change how hard it thinks")
    );
    session
        .order(Command::Configure {
            model: Some("deep".into()),
            effort: None,
            fast: None,
        })
        .await
        .unwrap();
    until(&session, |live| {
        live.transcript(10)
            .0
            .iter()
            .any(|entry| entry.text.contains("Switched to deep"))
    })
    .await;
    assert_eq!(
        session.info().model.as_ref().map(AsRef::as_ref),
        Some("deep")
    );
    let sent = sent(&log);
    assert_eq!(sent[3]["method"], "session/set_model");
    assert_eq!(sent[3]["params"]["modelId"], "deep");
}

/// Codex resuming a session: its `session/load` answer offers the model
/// selector among its other config options, as `session/new` does.
const RESUMED_CODEX_ADAPTER: &str = r#"
read -r line; log "$1"
reply '{"protocolVersion":1,"agentCapabilities":{"loadSession":true,"sessionCapabilities":{"resume":{}}}}'
read -r line; log "$1"
reply '{"models":{"availableModels":[{"modelId":"fast[low]"}],"currentModelId":"fast[low]"},"configOptions":[{"id":"collaboration_mode","name":"Collaboration mode","category":"collaboration_mode","type":"select","currentValue":"default","options":[{"value":"default","name":"Default"}]},{"id":"model","name":"Model","category":"model","type":"select","currentValue":"fast","options":[{"value":"fast","name":"Fast"},{"value":"deep","name":"Deep"}]},{"id":"fast-mode","name":"Fast mode","category":"model_config","type":"select","currentValue":"off","options":[{"value":"off","name":"Off"}]}]}'
read -r line; log "$1"
printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$(id)"
read -r line; log "$1"
reply '{}'
cat > /dev/null
"#;

#[cfg(unix)]
#[tokio::test]
async fn a_resumed_session_switches_its_model_with_what_load_offers() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();
    let log = dir.join("sent.jsonl");
    let mut kind = script("codex", RESUMED_CODEX_ADAPTER, &log);
    kind.default_model = Some(ModelId::new("deep"));
    let start = Start {
        resume: Some("0f3c2a9e-1111-4222-8333-944455556666".into()),
        ..start_on(&kind, dir, None)
    };
    let session = Session::start(start).unwrap();
    settle(&session).await;
    assert_eq!(
        session.info().model.as_ref().map(AsRef::as_ref),
        Some("fast")
    );
    session
        .order(Command::Configure {
            model: Some("deep".into()),
            effort: None,
            fast: None,
        })
        .await
        .unwrap();
    until(&session, |live| {
        live.transcript(10)
            .0
            .iter()
            .any(|entry| entry.text.contains("Switched to deep"))
    })
    .await;
    assert_eq!(
        session.info().model.as_ref().map(AsRef::as_ref),
        Some("deep")
    );
    let sent = sent(&log);
    assert_eq!(sent[1]["method"], "session/load");
    assert_eq!(sent[3]["method"], "session/set_config_option");
    assert_eq!(sent[3]["params"]["configId"], "model");
    assert_eq!(sent[3]["params"]["value"], "deep");
}

/// An adapter whose `session/new` offers a thinking option, answers
/// setting it with `$BEEP_FAKE_SET` as its error (an empty result when
/// unset), then ends a turn.
const THINKING_ADAPTER: &str = r#"
read -r line; log "$1"
reply '{"protocolVersion":1,"agentCapabilities":{}}'
read -r line; log "$1"
reply '{"sessionId":"s","configOptions":[{"id":"effort","name":"Effort","category":"thought_level","type":"select","currentValue":"medium","options":[{"value":"medium","name":"Medium"},{"value":"max","name":"Max"}]}]}'
read -r line; log "$1"
case "$line" in
*session/prompt*) ;;
*) if [ -n "$BEEP_FAKE_SET" ]; then printf '{"jsonrpc":"2.0","id":%s,"error":%s}\n' "$(id)" "$BEEP_FAKE_SET"; else reply '{"configOptions":[]}'; fi
 read -r line; log "$1";;
esac
printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$(id)"
cat > /dev/null
"#;

#[cfg(unix)]
#[tokio::test]
async fn a_new_session_sets_its_thinking_before_the_first_prompt() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();
    let log = dir.join("sent.jsonl");
    let kind = script("codex", THINKING_ADAPTER, &log);
    let session = Session::start(Start {
        effort: Some(Effort::Max),
        ..start_on(&kind, dir, None)
    })
    .unwrap();
    settle(&session).await;
    assert_eq!(session.info().turns, 1);
    let sent = sent(&log);
    assert_eq!(sent[2]["method"], "session/set_config_option");
    assert_eq!(sent[2]["params"]["configId"], "effort");
    assert_eq!(sent[2]["params"]["value"], "max");
    assert_eq!(sent[3]["method"], "session/prompt", "the prompt waits");
}

/// An adapter that ends its first turn with `/compact` advertised and
/// the window at 85%, reports a finished compaction for the second
/// prompt, then ends the third turn.
const COMPACTING_ADAPTER: &str = r#"
handshake "$1"
update() { printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s","update":%s}}\n' "$1"; }
read -r line; log "$1"
update '{"sessionUpdate":"available_commands_update","availableCommands":[{"name":"compact","description":"Compact"}]}'
update '{"sessionUpdate":"usage_update","used":85000,"size":100000}'
reply '{"stopReason":"end_turn"}'
read -r line; log "$1"
update '{"sessionUpdate":"compaction_update","compactionId":"c1","status":"in_progress"}'
update '{"sessionUpdate":"compaction_update","compactionId":"c1","status":"completed"}'
update '{"sessionUpdate":"usage_update","used":12000,"size":100000}'
reply '{"stopReason":"end_turn"}'
read -r line; log "$1"
update '{"sessionUpdate":"usage_update","used":13000,"size":100000}'
reply '{"stopReason":"end_turn"}'
cat > /dev/null
"#;

/// An adapter that lists one command on its first turn and a different
/// list, with a skill, on its second.
const COMMANDS_ADAPTER: &str = r#"
handshake "$1"
update() { printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s","update":%s}}\n' "$1"; }
read -r line; log "$1"
update '{"sessionUpdate":"available_commands_update","availableCommands":[{"name":"review","description":"Review the changes","input":{"hint":"[branch]"}}]}'
reply '{"stopReason":"end_turn"}'
read -r line; log "$1"
update '{"sessionUpdate":"available_commands_update","availableCommands":[{"name":"review","description":"Review the changes"},{"name":"skill:plan","description":"Plan the work"}]}'
reply '{"stopReason":"end_turn"}'
cat > /dev/null
"#;

#[cfg(unix)]
#[tokio::test]
async fn the_commands_an_agent_reports_are_kept_with_descriptions_and_follow_changes() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();
    let log = dir.join("sent.jsonl");
    let kind = script("codex", COMMANDS_ADAPTER, &log);
    let session = Session::start(start_on(&kind, dir, None)).unwrap();
    settle(&session).await;
    let first = session.commands();
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].name, "review");
    assert_eq!(first[0].description, "Review the changes");
    assert_eq!(first[0].hint, "[branch]");
    session
        .order(Command::Prompt(Prompt::from("next".to_owned())))
        .await
        .unwrap();
    until(&session, |live| live.info.turns >= 2).await;
    let names: Vec<String> = session.commands().into_iter().map(|c| c.name).collect();
    assert_eq!(names, ["review", "skill:plan"]);
    assert_eq!(session.commands()[1].description, "Plan the work");
}

/// The text of each prompt the adapter got.
fn prompts(log: &Path) -> Vec<String> {
    sent(log)
        .iter()
        .filter(|line| line["method"] == "session/prompt")
        .map(|line| {
            line["params"]["prompt"][0]["text"]
                .as_str()
                .unwrap_or_default()
                .to_owned()
        })
        .collect()
}

#[cfg(unix)]
#[tokio::test]
async fn a_full_window_is_compacted_as_soon_as_its_turn_ends() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();
    let log = dir.join("sent.jsonl");
    let kind = script("codex", COMPACTING_ADAPTER, &log);
    let session = Session::start(start_on(&kind, dir, None)).unwrap();
    settle(&session).await;
    // Nothing waits: the compaction goes before the next message comes, so
    // that message does not wait for it.
    until(&session, |live| {
        live.info
            .context
            .is_some_and(|context| context.used == 12_000)
            && live.status() == Status::Idle
    })
    .await;
    assert_eq!(prompts(&log), ["go", COMPACT_PROMPT]);
    session
        .order(Command::Prompt(Prompt::from("next".to_owned())))
        .await
        .unwrap();
    until(&session, |live| {
        live.info.turns >= 2 || live.status() == Status::Ended
    })
    .await;
    let info = session.info();
    assert_eq!(info.turns, 2, "{:?}", info.state.error);
    assert_eq!(prompts(&log), ["go", COMPACT_PROMPT, "next"]);
    let initialize = &sent(&log)[0];
    assert_eq!(
        initialize["params"]["clientCapabilities"]["session"]["compaction"],
        serde_json::json!({})
    );
    let (entries, _) = watched::read(&session.live, |live| live.transcript(100));
    let compacted: Vec<_> = entries
        .iter()
        .filter(|entry| entry.text == COMPACTED_ENTRY)
        .collect();
    assert_eq!(compacted.len(), 1, "{entries:?}");
    assert_eq!(compacted[0].kind, EntryKind::Compacted);
    assert!(
        !entries.iter().any(|entry| entry.text == COMPACT_PROMPT),
        "the node's /compact is not shown as the user's"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn a_default_model_uses_its_override_instead_of_the_harness_token_limit() {
    let handshake = r#"
read -r line; log "$1"
reply '{"protocolVersion":1,"agentCapabilities":{}}'
read -r line; log "$1"
reply '{"sessionId":"s","models":{"availableModels":[{"modelId":"model-small"}],"currentModelId":"model-small"}}'
"#;
    let adapter = COMPACTING_ADAPTER.replacen("handshake \"$1\"", handshake, 1);

    for enabled in [true, false] {
        let temp = tempfile::tempdir().unwrap();
        let log = temp.path().join("sent.jsonl");
        let mut kind = script("codex", &adapter, &log);
        kind.compact = Some(AutoCompact {
            enabled: true,
            percent: None,
            tokens: Some(90_000),
        });
        kind.compact_models = vec![ModelCompact {
            model: "model-small".into(),
            compact: AutoCompact {
                enabled,
                percent: None,
                tokens: Some(80_000),
            },
        }];
        let session = Session::start(start_on(&kind, temp.path(), None)).unwrap();
        settle(&session).await;
        session
            .order(Command::Prompt(Prompt::from("next".to_owned())))
            .await
            .unwrap();
        until(&session, |live| {
            live.info.turns >= 2 || live.status() == Status::Ended
        })
        .await;
        assert_eq!(session.info().turns, 2);
        let expected = if enabled {
            vec!["go", COMPACT_PROMPT, "next"]
        } else {
            vec!["go", "next"]
        };
        assert_eq!(prompts(&log), expected);
    }
}

#[cfg(unix)]
#[tokio::test]
async fn an_agent_without_a_system_prompt_hears_the_rule_again_after_compacting() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();
    let log = dir.join("sent.jsonl");
    let kind = script("pi", COMPACTING_ADAPTER, &log);
    let session = Session::start(Start {
        rule: Some("Worktree first.".into()),
        ..start_on(&kind, dir, None)
    })
    .unwrap();
    settle(&session).await;
    session
        .order(Command::Prompt(Prompt::from("next".to_owned())))
        .await
        .unwrap();
    until(&session, |live| {
        live.info.turns >= 2 || live.status() == Status::Ended
    })
    .await;
    let rule = crate::agents::tags::context("Worktree first.");
    let sent = sent(&log);
    let prompts: Vec<Vec<&str>> = sent
        .iter()
        .filter(|line| line["method"] == "session/prompt")
        .map(prompt_texts)
        .collect();
    assert_eq!(
        prompts,
        [
            vec![rule.as_str(), "go"],
            vec![COMPACT_PROMPT],
            vec![rule.as_str(), "next"],
        ]
    );
}

#[cfg(unix)]
#[tokio::test]
async fn a_resumed_session_keeps_the_rule_in_its_system_prompt() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();
    let log = dir.join("sent.jsonl");
    let kind = script("codex", SYSTEM_PROMPT_ADAPTER, &log);
    let session = Session::start(Start {
        resume: Some("s".into()),
        rule: Some("Worktree first.".into()),
        mcp: Some(beep_server()),
        ..start_on(&kind, dir, None)
    })
    .unwrap();
    settle(&session).await;
    let sent = sent(&log);
    assert_eq!(sent[1]["method"], "session/load");
    let system = format!(
        "Worktree first.\n\n{}\n\n{}",
        crate::agents::mcp::BRIEF,
        crate::agents::mcp::CHAT
    );
    assert_eq!(
        sent[1]["params"]["systemPrompt"]["content"][0]["text"],
        system.as_str()
    );
    assert_eq!(prompt_texts(&sent[2]), ["go"]);
}

/// An adapter that puts a choice to the user on its first prompt,
/// then ends that turn and the next one.
const TWO_TURN_ADAPTER: &str = r#"
handshake "$1"
say() { printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"%s"}}}}\n' "$1"; }
read -r line; log "$1"
prompt=$(id)
echo '{"jsonrpc":"2.0","id":"p1","method":"session/request_permission","params":{"sessionId":"s","toolCall":{"toolCallId":"pi-ui-1","title":"Which file?"},"options":[{"optionId":"choice-0","name":"notes.txt","kind":"allow_once"},{"optionId":"choice-1","name":"other.txt","kind":"allow_once"}]}}'
read -r line; log "$1"
say "First."
printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$prompt"
read -r line; log "$1"
say "Both done."
reply '{"stopReason":"end_turn"}'
cat > /dev/null
"#;

#[cfg(unix)]
#[tokio::test]
async fn a_question_alerts_and_a_queued_prompt_ends_in_one_alert() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();
    let log = dir.join("sent.jsonl");
    let kind = script("pi", TWO_TURN_ADAPTER, &log);
    let alerts = Arc::new(AlertHub::default());
    let session = Session::start(Start {
        alerts: alerts.clone(),
        ..start_on(&kind, dir, None)
    })
    .unwrap();
    settle(&session).await;
    let question = session.info().question.expect("a question");
    let asked = alerts.drain();
    assert_eq!(asked.len(), 1, "{asked:?}");
    // Named by the id the node lists it under, which is the agent's own
    // once it has given one, so a click on the notice finds the session.
    assert_eq!(asked[0].session, session.info().id);
    assert_eq!(asked[0].session, "s");
    assert_eq!(asked[0].agent, "pi");
    assert_eq!(
        asked[0].kind,
        AlertKind::Asks {
            question: question.clone()
        }
    );
    // Queued behind the asking turn, it runs before anyone hears of it.
    session
        .order(Command::Prompt(Prompt::from("next".to_owned())))
        .await
        .unwrap();
    session
        .order(Command::Answer {
            question: question.id,
            reply: "choice-0".into(),
        })
        .await
        .unwrap();
    until(&session, |live| {
        live.info.turns >= 2 || live.status() == Status::Ended
    })
    .await;
    assert_eq!(session.info().turns, 2, "{:?}", session.info().state.error);
    assert_eq!(
        alerts.drain(),
        [Alert {
            session: "s".into(),
            agent: "pi".into(),
            title: Some("go".into()),
            cwd: dir.display().to_string(),
            kind: AlertKind::TurnEnded {
                stop_reason: StopReason::EndTurn,
                last_message: "Both done.".into(),
                error: None,
            },
        }]
    );
    session.close().await;
    assert_eq!(alerts.drain(), [], "closing it is no news");
}

/// An adapter whose turn starts a Codex subagent and asks a question, then
/// ends while the subagent runs. The subagent ends once the test writes to
/// the fifo in `$2`.
const SUBAGENT_ADAPTER: &str = r#"
handshake "$1"
read -r line; log "$1"
prompt=$(id)
echo '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s","update":{"sessionUpdate":"tool_call","toolCallId":"spawn","title":"spawnAgent","kind":"other","status":"completed","rawInput":{"prompt":"Sleep","senderThreadId":"root","receiverThreadIds":["t1"],"agentsStates":{"t1":{"status":"running"}}}}}}'
echo '{"jsonrpc":"2.0","id":"p1","method":"session/request_permission","params":{"sessionId":"s","toolCall":{"toolCallId":"pi-ui-1","title":"Which file?"},"options":[{"optionId":"choice-0","name":"notes.txt","kind":"allow_once"}]}}'
read -r line; log "$1"
echo '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"Waiting on it."}}}}'
printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$prompt"
read -r go < "$2"
echo '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s","update":{"sessionUpdate":"tool_call","toolCallId":"done","title":"Complete subagent sleep","kind":"other","status":"completed","rawInput":{"agentThreadId":"t1","agentPath":"/root/sleep","activityKind":"completed"}}}}'
cat > /dev/null
"#;

#[cfg(unix)]
#[tokio::test]
async fn a_turn_alerts_its_end_once_its_subagents_end() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();
    let log = dir.join("sent.jsonl");
    let fifo = dir.join("go");
    let made = std::process::Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .unwrap();
    assert!(made.success());
    let mut kind = script("pi", SUBAGENT_ADAPTER, &log);
    kind.args.push(fifo.display().to_string());
    let alerts = Arc::new(AlertHub::default());
    let session = Session::start(Start {
        alerts: alerts.clone(),
        ..start_on(&kind, dir, None)
    })
    .unwrap();
    settle(&session).await;
    let question = session.info().question.expect("a question");
    assert_eq!(
        alerts
            .drain()
            .into_iter()
            .map(|alert| alert.kind)
            .collect::<Vec<_>>(),
        [AlertKind::Asks {
            question: question.clone()
        }],
        "a question alerts while a subagent runs"
    );

    session
        .order(Command::Answer {
            question: question.id,
            reply: "choice-0".into(),
        })
        .await
        .unwrap();
    until(&session, |live| {
        live.info.turns >= 1 || live.status() == Status::Ended
    })
    .await;
    assert_eq!(session.info().state.subagents, 1);
    assert_eq!(
        alerts.drain(),
        [],
        "the turn ended with its subagent running"
    );

    tokio::task::spawn_blocking(move || std::fs::write(fifo, "go\n"))
        .await
        .unwrap()
        .unwrap();
    until(&session, |live| {
        live.info.state.subagents == 0 || live.status() == Status::Ended
    })
    .await;
    // Orders run in turn, so the alert is in once the session closed.
    session.close().await;
    assert_eq!(
        alerts
            .drain()
            .into_iter()
            .map(|alert| alert.kind)
            .collect::<Vec<_>>(),
        [AlertKind::TurnEnded {
            stop_reason: StopReason::EndTurn,
            last_message: "Waiting on it.".into(),
            error: None,
        }]
    );
}

#[cfg(unix)]
#[tokio::test]
async fn a_turn_alerts_its_end_once_its_subagents_go_quiet() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();
    let log = dir.join("sent.jsonl");
    let fifo = dir.join("go");
    let made = std::process::Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .unwrap();
    assert!(made.success());
    let mut kind = script("pi", SUBAGENT_ADAPTER, &log);
    kind.args.push(fifo.display().to_string());
    let alerts = Arc::new(AlertHub::default());
    let session = Session::start(Start {
        alerts: alerts.clone(),
        ..start_on(&kind, dir, None)
    })
    .unwrap();
    settle(&session).await;
    let question = session.info().question.expect("a question");
    session
        .order(Command::Answer {
            question: question.id,
            reply: "choice-0".into(),
        })
        .await
        .unwrap();
    until(&session, |live| {
        live.info.turns >= 1 || live.status() == Status::Ended
    })
    .await;
    assert_eq!(session.info().state.subagents, 1);
    alerts.drain();

    // The subagent never says it ended.
    tokio::time::pause();
    tokio::time::sleep(turns::SUBAGENTS_QUIET - Duration::from_secs(60)).await;
    assert_eq!(alerts.drain(), [], "the subagent may still be working");
    tokio::time::sleep(Duration::from_secs(120)).await;
    let ended = alerts.drain();
    assert_eq!(ended.len(), 1, "{ended:?}");
    assert!(matches!(ended[0].kind, AlertKind::TurnEnded { .. }));
    tokio::time::sleep(turns::SUBAGENTS_QUIET * 2).await;
    assert_eq!(alerts.drain(), [], "once per turn");
    session.close().await;
}

/// An adapter whose first turn leaves a server running while another tool
/// finishes once the test writes to the fifo in `$2`. It answers the node's
/// `session/cancel` by ending the turn cancelled, and ends each later
/// turn at once. It logs every line it reads.
const STEERING_ADAPTER: &str = r#"
read -r line; log "$1"
reply '{"protocolVersion":1,"agentCapabilities":{"promptCapabilities":{"image":true}}}'
read -r line; log "$1"
reply '{"sessionId":"s"}'
read -r line; log "$1"
prompt=$(id)
echo '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s","update":{"sessionUpdate":"tool_call","toolCallId":"server","title":"Development server","status":"in_progress"}}}'
echo '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s","update":{"sessionUpdate":"tool_call","toolCallId":"t1","title":"Running tests","status":"in_progress"}}}'
read -r go < "$2"
echo '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s","update":{"sessionUpdate":"tool_call_update","toolCallId":"t1","status":"completed"}}}'
read -r line; log "$1"
printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"cancelled"}}\n' "$prompt"
while read -r line; do log "$1"; reply '{"stopReason":"end_turn"}'; done
"#;

#[cfg(unix)]
#[tokio::test]
async fn a_steer_goes_after_one_tool_finishes_despite_a_background_server() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();
    let log = dir.join("sent.jsonl");
    let fifo = dir.join("go");
    let made = std::process::Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .unwrap();
    assert!(made.success());
    let mut kind = script("codex", STEERING_ADAPTER, &log);
    kind.args.push(fifo.display().to_string());
    let session = Session::start(start_on(&kind, dir, None)).unwrap();
    until(&session, |live| {
        live.info.state.summary == "Running tests" || live.status() == Status::Ended
    })
    .await;
    session
        .order(Command::Prompt(Prompt::from("later".to_owned())))
        .await
        .unwrap();
    let mut steer = shot();
    steer.text = "Use the fixture @.beep/uploads/build.log".into();
    session.order(Command::Steer(steer)).await.unwrap();
    assert_eq!(session.info().state.queued, 2);
    assert_eq!(
        session.info().state.queued_messages,
        [
            "Use the fixture @.beep/uploads/build.log\n[Image: shot.png]",
            "later"
        ]
    );
    // Nothing goes while the tool call runs.
    let methods = |log: &Path| -> Vec<String> {
        sent(log)
            .iter()
            .map(|line| line["method"].as_str().unwrap_or("").to_owned())
            .collect()
    };
    assert_eq!(
        methods(&log),
        ["initialize", "session/new", "session/prompt"]
    );
    // The tool call finishes.
    tokio::task::spawn_blocking(move || std::fs::write(fifo, "go\n"))
        .await
        .unwrap()
        .unwrap();
    until(&session, |live| {
        live.info.turns >= 3 || live.status() == Status::Ended
    })
    .await;
    assert_eq!(
        methods(&log),
        [
            "initialize",
            "session/new",
            "session/prompt",
            "session/cancel",
            "session/prompt",
            "session/prompt"
        ]
    );
    assert_eq!(
        prompts(&log),
        ["go", "Use the fixture @.beep/uploads/build.log", "later"]
    );
    let steered = &sent(&log)[4];
    assert_eq!(steered["params"]["prompt"][1]["type"], "image");
    let info = session.info();
    assert_eq!(info.state.status, Status::Idle);
    assert_eq!(info.state.queued, 0);
}

/// An adapter whose first turn runs a tool call that does not finish. It
/// answers the node's `session/cancel` by ending the turn cancelled, and
/// ends each later turn at once. It logs every line it reads.
const STOPPED_ADAPTER: &str = r#"
handshake "$1"
read -r line; log "$1"
prompt=$(id)
echo '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s","update":{"sessionUpdate":"tool_call","toolCallId":"t1","title":"Running tests","status":"in_progress"}}}'
read -r line; log "$1"
printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"cancelled"}}\n' "$prompt"
while read -r line; do log "$1"; reply '{"stopReason":"end_turn"}'; done
"#;

/// A stop from an app keeps what it steered with: the node sends it once
/// the turn stops, as the only copy, since the app no longer sends it again
/// (#378). A stop of a child drops it.
#[cfg(unix)]
#[tokio::test]
async fn a_steer_held_at_stop_goes_once_the_turn_stops() {
    for by_user in [true, false] {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();
        let log = dir.join("sent.jsonl");
        let kind = script("codex", STOPPED_ADAPTER, &log);
        let session = Session::start(start_on(&kind, dir, None)).unwrap();
        until(&session, |live| {
            live.info.state.summary == "Running tests" || live.status() == Status::Ended
        })
        .await;
        let steer = Prompt::from("try again, it failed".to_owned());
        session.order(Command::Steer(steer)).await.unwrap();
        session.order(Command::Cancel { by_user }).await.unwrap();
        let turns = if by_user { 2 } else { 1 };
        until(&session, |live| {
            live.info.turns >= turns || live.status() == Status::Ended
        })
        .await;
        let want: &[&str] = if by_user {
            &["go", "try again, it failed"]
        } else {
            &["go"]
        };
        assert_eq!(prompts(&log), want);
        let info = session.info();
        assert_eq!(info.state.status, Status::Idle);
        assert_eq!(info.state.queued, 0);
        session.close().await;
    }
}

#[test]
fn steers_waiting_together_go_as_one_prompt_within_the_image_limit() {
    let mut steers: VecDeque<Prompt> = VecDeque::new();
    assert_eq!(take_steers(&mut steers), None);
    steers.push_back(Prompt::from("first".to_owned()));
    steers.push_back(shot());
    steers.push_back(Prompt::from("third".to_owned()));
    let merged = take_steers(&mut steers).unwrap();
    assert_eq!(merged.text, "first\n\nThe button overlaps\n\nthird");
    assert_eq!(merged.images.len(), 1);
    assert!(steers.is_empty());

    // One more image than a prompt takes waits for the next prompt.
    steers.extend((0..=MAX_AGENT_IMAGES).map(|_| shot()));
    let merged = take_steers(&mut steers).unwrap();
    assert_eq!(merged.images.len(), MAX_AGENT_IMAGES);
    assert_eq!(steers.len(), 1);
}

#[cfg(unix)]
#[tokio::test]
async fn stop_shows_at_once_and_an_agent_that_ignores_it_is_stopped() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();
    let body = format!(
        r#"
echo '{UPDATE}{{"sessionUpdate":"tool_call","toolCallId":"t1","title":"Building","status":"in_progress"}}}}}}'
while read -r line; do log "$1"; done
"#
    );
    let session = started_within(dir, &body, Duration::from_millis(200));
    until(&session, |live| {
        live.info.state.summary == "Building" || live.status() == Status::Ended
    })
    .await;
    session
        .order(Command::Cancel { by_user: true })
        .await
        .unwrap();
    // Before the agent says anything, the turn reads as stopping.
    let info = session.info();
    assert_eq!(info.state.summary, "Stopping");
    assert_eq!(info.state.status, Status::Working);
    until(&session, |live| live.status() == Status::Ended).await;
    let error = session.info().state.error.unwrap_or_default();
    assert!(error.contains("did not stop within"), "{error}");
    let cancels = sent(&dir.join("log"))
        .iter()
        .filter(|line| line["method"] == "session/cancel")
        .count();
    assert_eq!(cancels, 1);
}

/// Claude Code with session modes: a turn in plan mode asks to run a command,
/// which the node allows, then asks to go ahead with its plan. Keep planning
/// ends the turn. Approval takes it to the mode the option names, `default`,
/// and the session is switched from there.
const PLAN_ADAPTER: &str = r#"
exit_plan='{"sessionId":"s","toolCall":{"toolCallId":"exit","title":"Ready to code?","rawInput":{"plan":"1. Add a flag"}},"options":[{"optionId":"exit-plan-clear-bypass","name":"Clear","kind":"allow_always"},{"optionId":"exit-plan-default","name":"Manual","kind":"allow_once"},{"optionId":"reject","name":"No","kind":"reject_once"}]}'
read -r line; log "$1"
reply '{"protocolVersion":1,"agentCapabilities":{}}'
read -r line; log "$1"
reply '{"sessionId":"s","modes":{"currentModeId":"default","availableModes":[{"id":"default","name":"Manual"},{"id":"acceptEdits","name":"Accept edits"},{"id":"plan","name":"Plan"},{"id":"bypassPermissions","name":"Bypass permissions"}]}}'
read -r line; log "$1"
reply '{}'
read -r line; log "$1"
reply '{}'
read -r line; log "$1"
prompt=$(id)
echo '{"jsonrpc":"2.0","id":"run","method":"session/request_permission","params":{"sessionId":"s","toolCall":{"toolCallId":"t1","title":"Run ls"},"options":[{"optionId":"allow","name":"Allow","kind":"allow_once"},{"optionId":"reject","name":"Reject","kind":"reject_once"}]}}'
read -r line; log "$1"
printf '{"jsonrpc":"2.0","id":"no","method":"session/request_permission","params":%s}\n' "$exit_plan"
read -r line; log "$1"
printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$prompt"
read -r line; log "$1"
prompt=$(id)
printf '{"jsonrpc":"2.0","id":"yes","method":"session/request_permission","params":%s}\n' "$exit_plan"
read -r line; log "$1"
echo '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s","update":{"sessionUpdate":"current_mode_update","currentModeId":"default"}}}'
read -r line; log "$1"
reply '{}'
printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$prompt"
cat > /dev/null
"#;

#[cfg(unix)]
#[tokio::test]
async fn plan_mode_waits_for_approval_and_goes_back_to_the_mode_before_it() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();
    let log = dir.join("sent.jsonl");
    let kind = script("claude", PLAN_ADAPTER, &log);
    let session = Session::start(Start {
        prompt: None,
        ..start_on(&kind, dir, None)
    })
    .unwrap();
    until(&session, |live| live.status() == Status::Idle).await;
    let info = session.info();
    assert_eq!(info.mode.as_deref(), Some("bypassPermissions"));
    let names: Vec<&str> = info.modes.iter().map(|mode| mode.name.as_str()).collect();
    assert_eq!(names, ["Full access", "Plan", "Manual", "Accept edits"]);

    let refused = session.order(Command::SetMode("yolo".into())).await;
    assert!(refused.unwrap_err().contains("no mode yolo"));
    session
        .order(Command::SetMode("plan".into()))
        .await
        .unwrap();
    assert_eq!(session.info().mode.as_deref(), Some("plan"));

    // The command is allowed as ever; the plan waits for the user.
    let prompt = |text: &str| Command::Prompt(Prompt::from(text.to_owned()));
    let plan_ready = |live: &Live| live.info.question.is_some();
    session.order(prompt("plan it")).await.unwrap();
    until(&session, plan_ready).await;
    let info = session.info();
    assert_eq!(info.plan_ready.as_deref(), Some("1. Add a flag"));
    let question = info.question.unwrap();
    let names: Vec<&str> = question.options.iter().map(|o| o.name.as_str()).collect();
    assert_eq!(names, ["Approve and start", "Keep planning"]);
    let (entries, _) = watched::read(&session.live, |live| live.transcript(10));
    assert!(entries.iter().any(|entry| entry.text == "Allowed: Run ls"));

    // Keep planning: the turn ends, still in plan mode.
    let answer = |id, reply: &str| Command::Answer {
        question: id,
        reply: reply.to_owned(),
    };
    session
        .order(answer(question.id, &question.options[1].id))
        .await
        .unwrap();
    until(&session, |live| live.info.turns == 1).await;
    let info = session.info();
    assert_eq!(info.mode.as_deref(), Some("plan"));
    assert_eq!(info.plan_ready, None);

    // Approve and start: back to Full access, the mode before plan mode.
    session.order(prompt("again")).await.unwrap();
    until(&session, plan_ready).await;
    let question = session.info().question.unwrap();
    session
        .order(answer(question.id, &question.options[0].id))
        .await
        .unwrap();
    until(&session, |live| live.info.turns == 2).await;
    assert_eq!(session.info().mode.as_deref(), Some("bypassPermissions"));

    let sent = sent(&log);
    let answered = |id: &str| {
        let answer = sent.iter().find(|line| line["id"] == id).unwrap();
        answer["result"]["outcome"]["optionId"].clone()
    };
    assert_eq!(answered("run"), "allow");
    assert_eq!(answered("no"), "reject");
    assert_eq!(answered("yes"), "exit-plan-default");
    let modes: Vec<&Value> = sent
        .iter()
        .filter(|line| line["method"] == "session/set_mode")
        .map(|line| &line["params"]["modeId"])
        .collect();
    assert_eq!(modes, ["bypassPermissions", "plan", "bypassPermissions"]);
}

/// Claude Code with `asyncTasks`: one prompt ends its turn with two commands
/// left in the background (the second announced before its call is marked),
/// and a turn of its own that is not a command. A stop request stops the first
/// and the second ends by itself.
const BACKGROUND_ADAPTER: &str = r#"
read -r line; log "$1"
reply '{"protocolVersion":1,"agentCapabilities":{}}'
read -r line; log "$1"
reply '{"sessionId":"s"}'
update() { printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s","update":%s}}\n' "$1"; }
marked='"_meta":{"jetbrains":{"air":{"version":1,"asyncTasks":{"backgrounded":true}}}}'
while read -r line; do
    log "$1"
    case "$line" in
    *'"session/prompt"'*)
        prompt=$(id)
        update '{"sessionUpdate":"tool_call","toolCallId":"t1","title":"Run sleep 60","kind":"execute","status":"in_progress"}'
        update '{"sessionUpdate":"async_task_spawned","asyncTaskId":"bg1","name":"sleep 60","taskType":"local_bash","description":"Run sleep 60","showInTranscript":true,"canStop":true,"outputFilePath":"/srv/tasks/bg1.output","toolCallId":"t1"}'
        update '{"sessionUpdate":"tool_call_update","toolCallId":"t1","status":"completed",'"$marked"'}'
        update '{"sessionUpdate":"tool_call","toolCallId":"t2","title":"Run make watch","kind":"execute","status":"in_progress"}'
        update '{"sessionUpdate":"tool_call_update","toolCallId":"t2","status":"completed",'"$marked"'}'
        update '{"sessionUpdate":"async_task_spawned","asyncTaskId":"bg2","name":"make watch","taskType":"local_bash","description":"Run make watch","showInTranscript":true,"canStop":true,"toolCallId":"t2"}'
        update '{"sessionUpdate":"async_task_progress","asyncTaskId":"bg2","description":"Run make watch (building)"}'
        update '{"sessionUpdate":"async_task_spawned","asyncTaskId":"turn1","name":"Claude is working","taskType":"turn","description":"","showInTranscript":false,"canStop":true}'
        printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$prompt"
        ;;
    *'"_session/async_task/stop"'*)
        reply '{"stopped":true}'
        update '{"sessionUpdate":"async_task_state_update","asyncTaskId":"bg1","state":"stopped"}'
        update '{"sessionUpdate":"async_task_state_update","asyncTaskId":"bg2","state":"completed","summary":"exit 0"}'
        ;;
    *) reply '{}' ;;
    esac
done
"#;

#[cfg(unix)]
#[tokio::test]
async fn background_commands_show_as_running_and_stop_without_ending_the_turn() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();
    let log = dir.join("sent.jsonl");
    let kind = script("claude", BACKGROUND_ADAPTER, &log);
    let session = Session::start(start_on(&kind, dir, None)).unwrap();

    // The turn ended; both commands run, and the turn's own task is not one.
    until(&session, |live| {
        live.info.turns == 1 && live.info.state.tasks.len() == 2
    })
    .await;
    let tasks = session.info().state.tasks;
    let running = |id: &str| tasks.iter().find(|task| task.id == id).unwrap().clone();
    assert_eq!(running("bg1").state, TaskState::Running);
    assert!(running("bg1").can_stop);
    assert_eq!(running("bg1").tool_call.as_deref(), Some("t1"));
    assert_eq!(
        running("bg1").output_file.as_deref(),
        Some("/srv/tasks/bg1.output")
    );
    assert_eq!(running("bg2").description, "Run make watch (building)");
    // Their calls completed, yet read as running, in whichever order the
    // adapter said so.
    let shown = |session: &Session| -> Vec<String> {
        let (entries, _) = watched::read(&session.live, |live| live.transcript(10));
        entries
            .into_iter()
            .filter(|entry| entry.kind == EntryKind::Tool)
            .map(|entry| entry.text)
            .collect()
    };
    assert_eq!(
        shown(&session),
        ["Run sleep 60 [in_progress]", "Run make watch [in_progress]"]
    );
    assert_eq!(running("bg1").entry, Some(1));
    assert_eq!(running("bg2").entry, Some(2));

    let missing = session.order(Command::StopTask("nope".into())).await;
    assert!(missing.unwrap_err().contains("no task nope"));
    session
        .order(Command::StopTask("bg1".into()))
        .await
        .unwrap();
    until(&session, |live| {
        live.info.state.tasks.iter().all(|task| task.state.ended())
    })
    .await;

    // One stopped, one ended by itself; their calls read as done now.
    let tasks = session.info().state.tasks;
    assert_eq!(tasks[0].state, TaskState::Stopped);
    assert_eq!(tasks[1].state, TaskState::Completed);
    assert_eq!(tasks[1].summary.as_deref(), Some("exit 0"));
    assert_eq!(
        shown(&session),
        ["Run sleep 60 [completed]", "Run make watch [completed]"]
    );
    let ended = session.order(Command::StopTask("bg1".into())).await;
    assert!(ended.unwrap_err().contains("has ended"));

    let sent = sent(&log);
    let capabilities = &sent[0]["params"]["clientCapabilities"]["_meta"]["jetbrains"]["air"];
    assert_eq!(capabilities["version"], 1);
    assert_eq!(
        capabilities["capabilities"],
        serde_json::json!(["asyncTasks"])
    );
    let stops: Vec<&Value> = sent
        .iter()
        .filter(|line| line["method"] == "_session/async_task/stop")
        .collect();
    assert_eq!(stops.len(), 1);
    assert_eq!(stops[0]["params"]["sessionId"], "s");
    assert_eq!(stops[0]["params"]["asyncTaskId"], "bg1");
    assert!(sent.iter().all(|line| line["method"] != "session/cancel"));
    assert_eq!(session.info().state.status, Status::Idle);
}

/// An adapter that writes the compaction variables it started with to
/// the file in `$2`, then exits, which ends the session.
const ENV_ADAPTER: &str = r#"
printf '%s|%s|%s|%s\n' "$CLAUDE_AUTOCOMPACT_PCT_OVERRIDE" "$DISABLE_AUTO_COMPACT" "$OPENCODE_DISABLE_AUTOCOMPACT" "$CODEX_CONFIG" > "$2"
"#;

/// What `ENV_ADAPTER` prints when the node sets each of its variables to
/// `set`, an empty one meaning removed, and passes on the rest as this
/// process has them.
fn printed(set: [Option<&str>; 4]) -> String {
    const VARS: [&str; 4] = [
        "CLAUDE_AUTOCOMPACT_PCT_OVERRIDE",
        "DISABLE_AUTO_COMPACT",
        "OPENCODE_DISABLE_AUTOCOMPACT",
        "CODEX_CONFIG",
    ];
    let line = VARS
        .iter()
        .zip(set)
        .map(|(name, set)| {
            set.map_or_else(|| std::env::var(name).unwrap_or_default(), str::to_owned)
        })
        .collect::<Vec<_>>()
        .join("|");
    line.trim().to_owned()
}

/// What the adapter of agent `name` saw in its environment, given its
/// `compact` setting.
async fn compaction_env(name: &str, compact: Option<AutoCompact>) -> String {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();
    let log = dir.join("sent.jsonl");
    let seen = dir.join("env");
    let mut kind = script(name, ENV_ADAPTER, &log);
    kind.args.push(seen.display().to_string());
    kind.compact = compact;
    let session = Session::start(start_on(&kind, dir, None)).unwrap();
    settle(&session).await;
    std::fs::read_to_string(&seen).unwrap().trim().to_owned()
}

#[cfg(unix)]
#[tokio::test]
async fn codex_resumes_with_the_session_model_in_its_startup_config() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();
    let seen = dir.join("env");
    let mut kind = script(
        "codex",
        r#"printf '%s' "$CODEX_CONFIG" > "$2""#,
        &dir.join("sent.jsonl"),
    );
    kind.args.push(seen.display().to_string());
    kind.compact = Some(AutoCompact {
        enabled: true,
        percent: None,
        tokens: Some(350_000),
    });

    for (env, expected) in [
        (
            None,
            serde_json::json!({"model": "saved-model", "model_auto_compact_token_limit": 350_000}),
        ),
        (
            Some(r#"{"model":"default-model","model_auto_compact_token_limit":200000}"#),
            serde_json::json!({"model": "saved-model", "model_auto_compact_token_limit": 200_000}),
        ),
    ] {
        if let Some(env) = env {
            kind.env.insert("CODEX_CONFIG".to_owned(), env.to_owned());
        }
        let start = Start {
            resume: Some("held-1".into()),
            ..start_on(&kind, dir, Some("saved-model"))
        };
        let session = Session::start(start).unwrap();
        settle(&session).await;

        let config: Value = serde_json::from_slice(&std::fs::read(&seen).unwrap()).unwrap();
        assert_eq!(config, expected);
    }
}

#[cfg(unix)]
#[tokio::test]
async fn agent_processes_inherit_the_main_checkouts_cargo_target() {
    use crate::agents::checkout::tests::{repo, run};

    let (_temp, root) = repo("agent-cargo-target");
    std::fs::write(root.join("Cargo.toml"), "[workspace]\n").unwrap();
    run(&root, &["add", "Cargo.toml"]);
    run(&root, &["commit", "--quiet", "-m", "Add workspace"]);
    run(&root, &["worktree", "add", "--quiet", ".worktrees/change"]);
    let cwd = root.join(".worktrees/change");
    let seen = root.join("env");
    let log = root.join("sent.jsonl");
    let mut kind = script("test", r#"printf '%s' "$CARGO_TARGET_DIR" > "$2""#, &log);
    kind.args.push(seen.display().to_string());
    kind.env.insert(
        "CARGO_HOME".to_owned(),
        root.join("cargo-home").display().to_string(),
    );
    let expected = dunce::canonicalize(&root).unwrap().join("target");
    let expected = std::env::var("CARGO_TARGET_DIR").unwrap_or_else(|_| {
        if std::env::var_os("CARGO_BUILD_TARGET_DIR").is_some() {
            String::new()
        } else {
            expected.display().to_string()
        }
    });

    let session = Session::start(start_on(&kind, &cwd, None)).unwrap();
    settle(&session).await;
    assert_eq!(std::fs::read_to_string(&seen).unwrap(), expected);

    kind.env
        .insert("CARGO_TARGET_DIR".to_owned(), "custom-output".to_owned());
    let session = Session::start(start_on(&kind, &cwd, None)).unwrap();
    settle(&session).await;
    assert_eq!(std::fs::read_to_string(&seen).unwrap(), "custom-output");
}

#[cfg(unix)]
#[tokio::test]
async fn an_agent_with_its_own_compaction_starts_with_the_setting() {
    let at = |percent| {
        Some(AutoCompact {
            enabled: true,
            percent,
            tokens: None,
        })
    };
    let off = Some(AutoCompact {
        enabled: false,
        percent: None,
        tokens: None,
    });
    assert_eq!(
        compaction_env("claude", at(Some(65))).await,
        printed([Some("65"), Some(""), None, None])
    );
    assert_eq!(
        compaction_env("claude", off).await,
        printed([None, Some("1"), None, None])
    );
    assert_eq!(
        compaction_env("opencode", off).await,
        printed([None, None, Some("1"), None])
    );
    assert_eq!(
        compaction_env("opencode", at(Some(65))).await,
        printed([None, None, Some(""), None])
    );
    assert_eq!(compaction_env("codex", off).await, printed([None; 4]));
    let tokens = Some(AutoCompact {
        enabled: true,
        percent: None,
        tokens: Some(350_000),
    });
    assert_eq!(
        compaction_env("claude", tokens).await,
        printed([None, Some("1"), None, None])
    );
    assert_eq!(
        compaction_env("codex", tokens).await,
        printed([
            None,
            None,
            None,
            Some(r#"{"model_auto_compact_token_limit":350000}"#)
        ])
    );
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn closing_a_session_stops_a_child_that_left_its_group() {
    let temp = tempfile::tempdir().unwrap();
    let log = temp.path().join("sent");
    let kind = script(
        "test",
        r#"
setsid sh -c 'echo $$ > "$1.pid"; trap "" TERM; exec sleep 300' worker "$1" </dev/null >/dev/null 2>&1 &
sleep 301 </dev/null >/dev/null 2>&1 &
echo $! > "$1.group"
handshake "$1"
read -r line
reply '{"stopReason":"end_turn"}'
cat >/dev/null
"#,
        &log,
    );
    let session = Session::start(start_on(&kind, temp.path(), None)).unwrap();
    settle(&session).await;
    let escaped = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let Ok(pid) = std::fs::read_to_string(log.with_extension("pid"))
                && let Ok(pid) = pid.trim().parse::<u32>()
                && let Some(row) = session
                    .processes
                    .list()
                    .await
                    .unwrap()
                    .into_iter()
                    .find(|p| p.pid == pid && p.command.contains("sleep 300"))
            {
                break row;
            }

            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("the escaped child is listed");
    assert!(escaped.left_group);
    assert!(escaped.command.contains("sleep 300"));
    assert!(escaped.started_at > 0);
    let group_pid = std::fs::read_to_string(log.with_extension("group"))
        .unwrap()
        .trim()
        .parse::<u32>()
        .unwrap();
    let rows = session.processes.list().await.unwrap();
    assert!(rows.iter().any(|p| p.pid == group_pid && !p.left_group));
    assert!(rows.iter().all(|p| p.pid != session.processes.root()));
    session.processes.stop(group_pid).await.unwrap();
    assert!(
        session
            .processes
            .list()
            .await
            .unwrap()
            .iter()
            .all(|p| p.pid != group_pid)
    );
    assert!(session.processes.stop(std::process::id()).await.is_err());
    assert!(
        session
            .processes
            .stop(session.processes.root())
            .await
            .is_err()
    );
    tokio::time::timeout(Duration::from_secs(10), session.close())
        .await
        .expect("close finishes");
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let stat = std::fs::read_to_string(format!("/proc/{}/stat", escaped.pid));

            if stat.is_err()
                || stat.is_ok_and(|s| s.rsplit_once(") ").is_some_and(|(_, f)| f.starts_with('Z')))
            {
                break;
            }

            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("the escaped child stopped, including one ignoring TERM");
}

/// Holds the tool completion and original turn's end on separate FIFO reads.
/// Immediate modes leave the tool running while accepting steering.
const INJECTING_ADAPTER: &str = r#"
read -r line; log "$1"
reply '{"protocolVersion":1,"agentInfo":{"name":"@agentclientprotocol/codex-acp","version":"2.4.0"},"agentCapabilities":{"promptCapabilities":{"image":true}},"_meta":{"steering":{"supported":true}}}'
read -r line; log "$1"; reply '{"sessionId":"s"}'
read -r line; log "$1"; prompt=$(id)
echo '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s","update":{"sessionUpdate":"tool_call","toolCallId":"t1","title":"Running tests","status":"in_progress"}}}'
if [ "$2" != immediate ] && [ "$2" != multiple ]; then
    read -r go < "$3"
    echo '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s","update":{"sessionUpdate":"tool_call_update","toolCallId":"t1","status":"completed"}}}'
fi
read -r line; log "$1"
if [ "$2" = multiple ]; then read -r go < "$3"; fi
case "$2" in
    injected|immediate|multiple)
        echo '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s","update":{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"make it blue"}}}}'
        echo '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"Adjusted."}}}}'
        reply '{"outcome":"injected"}'
        if [ "$2" = multiple ]; then
            read -r line; log "$1"
            reply '{"outcome":"injected"}'
        fi
        read -r go < "$3"
        ;;
    idle-first)
        printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$prompt"
        reply '{"outcome":"promptRequired","reason":"noRunningTurn"}'
        ;;
    idle-last)
        reply '{"outcome":"promptRequired","reason":"noRunningTurn"}'
        echo '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s","update":{"sessionUpdate":"session_info_update","title":"Settling"}}}'
        read -r go < "$3"
        ;;
    rejected)
        printf '{"jsonrpc":"2.0","id":%s,"error":{"code":-32601,"message":"Method not found"}}\n' "$(id)"
        read -r line; log "$1"
        ;;
esac
if [ "$2" != idle-first ]; then
    printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$prompt"
fi
while read -r line; do log "$1"; reply '{"stopReason":"end_turn"}'; done
"#;

#[cfg(unix)]
fn injecting(dir: &Path, outcome: &str) -> (AgentKind, PathBuf, PathBuf) {
    let log = dir.join("sent.jsonl");
    let fifo = dir.join("go");
    assert!(
        std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap()
            .success()
    );
    let mut kind = script("codex", INJECTING_ADAPTER, &log);
    kind.args
        .extend([outcome.to_owned(), fifo.display().to_string()]);
    (kind, log, fifo)
}

#[cfg(unix)]
async fn release(fifo: &Path) {
    let fifo = fifo.to_owned();
    tokio::task::spawn_blocking(move || std::fs::write(fifo, "go\n"))
        .await
        .unwrap()
        .unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn a_steer_injects_immediately_while_a_tool_is_running() {
    let temp = tempfile::tempdir().unwrap();
    let (kind, log, fifo) = injecting(temp.path(), "immediate");
    let session = Session::start(start_on(&kind, temp.path(), None)).unwrap();
    until(&session, |live| live.info.state.summary == "Running tests").await;
    let mut steer = shot();
    steer.text = "make it blue".into();
    session.order(Command::Steer(steer)).await.unwrap();
    until(&session, |live| live.info.state.queued == 0).await;
    let requests = sent(&log);
    assert_eq!(requests.len(), 4);
    assert_eq!(requests[3]["method"], "_session/steering");
    assert_eq!(requests[3]["params"]["prompt"][0]["text"], "make it blue");
    assert_eq!(requests[3]["params"]["prompt"][1]["type"], "image");
    assert_eq!(
        requests[3]["params"]["_meta"]["steering"]["idleBehavior"],
        "promptRequired"
    );
    assert_eq!(session.info().turns, 0);
    assert_eq!(session.info().state.status, Status::Working);
    let (entries, _) = watched::read(&session.live, |live| live.transcript(20));
    assert_eq!(
        entries.iter().filter(|e| e.kind == EntryKind::User).count(),
        2
    );
    assert_eq!(entries.last().unwrap().text, "Adjusted.");
    release(&fifo).await;
    settle(&session).await;
    assert_eq!(session.info().turns, 1);
    assert_eq!(prompts(&log), ["go"]);
}

#[cfg(unix)]
#[tokio::test]
async fn steers_waiting_for_an_injection_reply_continue_while_tools_run() {
    let temp = tempfile::tempdir().unwrap();
    let (kind, log, fifo) = injecting(temp.path(), "multiple");
    let session = Session::start(start_on(&kind, temp.path(), None)).unwrap();
    until(&session, |live| live.info.state.summary == "Running tests").await;
    session
        .order(Command::Steer("first correction".to_owned().into()))
        .await
        .unwrap();
    session
        .order(Command::Steer("second correction".to_owned().into()))
        .await
        .unwrap();
    assert_eq!(session.info().state.queued, 2);
    release(&fifo).await;
    until(&session, |live| live.info.state.queued == 0).await;
    let requests = sent(&log);
    let steering: Vec<_> = requests
        .iter()
        .filter(|request| request["method"] == "_session/steering")
        .collect();
    assert_eq!(steering.len(), 2);
    assert_eq!(
        steering[0]["params"]["prompt"][0]["text"],
        "first correction"
    );
    assert_eq!(
        steering[1]["params"]["prompt"][0]["text"],
        "second correction"
    );
    assert_eq!(session.info().turns, 0);
    assert_eq!(session.info().state.status, Status::Working);
    release(&fifo).await;
    settle(&session).await;
    assert_eq!(prompts(&log), ["go"]);
}

#[cfg(unix)]
#[tokio::test]
async fn an_idle_injection_is_sent_once_as_a_prompt_in_either_reply_order() {
    for outcome in ["idle-first", "idle-last", "rejected"] {
        let temp = tempfile::tempdir().unwrap();
        let (kind, log, fifo) = injecting(temp.path(), outcome);
        let session = Session::start(start_on(&kind, temp.path(), None)).unwrap();
        until(&session, |live| live.info.state.summary == "Running tests").await;
        session
            .order(Command::Steer("correction".to_owned().into()))
            .await
            .unwrap();
        session
            .order(Command::Prompt("later".to_owned().into()))
            .await
            .unwrap();
        release(&fifo).await;

        if outcome == "idle-last" {
            until(&session, |live| {
                live.info.title.as_deref() == Some("Settling")
            })
            .await;
            assert_eq!(session.info().state.queued, 2);
            assert_eq!(sent(&log).len(), 4, "wait for the original prompt to end");
            release(&fifo).await;
        }

        until(&session, |live| live.info.turns >= 3).await;
        assert_eq!(prompts(&log), ["go", "correction", "later"], "{outcome}");
        let requests = sent(&log);
        assert_eq!(
            requests
                .iter()
                .filter(|r| r["method"] == "_session/steering")
                .count(),
            1
        );
        assert_eq!(
            requests
                .iter()
                .filter(|r| r["method"] == "session/cancel")
                .count(),
            usize::from(outcome == "rejected")
        );
        assert_eq!(session.info().state.queued, 0);
    }
}

#[cfg(unix)]
#[tokio::test]
async fn an_explicit_close_refuses_late_uploads_before_the_agent_stops() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();
    let log = dir.join("sent.jsonl");
    let kind = script("codex", PLAIN_ADAPTER, &log);
    let session = Session::start(start_on(&kind, dir, None)).unwrap();
    settle(&session).await;
    session
        .order(Command::Close { by_user: true })
        .await
        .unwrap();
    let info = session.info();
    // The close must win even before validating the upload's name or touching disk.
    let error = crate::agents::uploads::write(
        &info.agent,
        &info.id,
        "../late.mp4",
        0,
        1,
        b"x",
        Some(&session),
    )
    .unwrap_err();
    assert!(error.to_string().contains("session is closed"), "{error:#}");
    until(&session, |live| live.status() == Status::Ended).await;
}

#[cfg(unix)]
#[tokio::test]
async fn a_harness_default_model_is_used_unless_the_session_picks_one() {
    for (requested, expected) in [(None, "deep"), (Some("fast"), "fast")] {
        let temp = tempfile::tempdir().unwrap();
        let log = temp.path().join("sent.jsonl");
        let mut kind = script("codex", MODEL_ADAPTER, &log);
        kind.default_model = Some(ModelId::new("deep"));
        let session = Session::start(start_on(&kind, temp.path(), requested)).unwrap();
        settle(&session).await;
        assert_eq!(session.info().turns, 1, "{:?}", session.info().state.error);
        let sent = sent(&log);
        let switched = sent
            .iter()
            .find(|message| message["method"] == "session/set_model");
        assert_eq!(switched.unwrap()["params"]["modelId"], expected);
    }
}

#[cfg(unix)]
#[tokio::test]
async fn saved_instructions_reach_new_and_resumed_sessions_without_editing_files() {
    use crate::agents::{Agents, running::Begin};
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();
    let local = dir.join("CLAUDE.md");
    std::fs::write(&local, "Local instructions.\n").unwrap();
    let agents = Agents::default();

    for (index, resume, text) in [
        (0, None, "Use short sentences."),
        (1, Some(SessionId::from("s")), "Use plain words."),
    ] {
        agents.set_instructions(Some(text.to_owned()));
        let log = dir.join(format!("sent-{index}.jsonl"));
        let kind = script("codex", SYSTEM_PROMPT_ADAPTER, &log);
        let session = agents
            .start(
                &kind,
                dir.to_str(),
                Begin {
                    prompt: Some("go".into()),
                    resume,
                    ..Begin::default()
                },
            )
            .unwrap();
        settle(&session).await;
        let sent = sent(&log);
        assert_eq!(
            sent[1]["params"]["systemPrompt"]["content"][0]["text"],
            text
        );
        assert_eq!(
            std::fs::read_to_string(&local).unwrap(),
            "Local instructions.\n"
        );
        assert!(!dir.join("AGENTS.md").exists());
        session
            .order(Command::Close { by_user: true })
            .await
            .unwrap();
        until(&session, |live| live.status() == Status::Ended).await;
    }
}

/// Claude Code with steering and `asyncTasks`: the prompt launches a
/// background subagent (its `Agent` call completes at once, launched
/// `isAsync`), starts a background command, and runs a foreground command
/// that does not finish. `session/cancel` ends the turn and spares the
/// subagent, as Claude Code does when the client stops tasks one by one.
/// With `$2` set to `ended`, the subagent reports at once and Claude Code's
/// own turn after it ends.
const BACKGROUND_SUBAGENT_ADAPTER: &str = r#"
read -r line; log "$1"
reply '{"protocolVersion":1,"agentInfo":{"name":"@agentclientprotocol/claude-agent-acp","version":"0.101.0"},"agentCapabilities":{},"_meta":{"steering":{"supported":true}}}'
update() { printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s","update":%s}}\n' "$1"; }
while read -r line; do
    log "$1"
    case "$line" in
    *'"session/new"'*) reply '{"sessionId":"s"}' ;;
    *'"session/prompt"'*)
        prompt=$(id)
        update '{"sessionUpdate":"tool_call","toolCallId":"agent1","title":"Task","kind":"think","status":"pending","rawInput":{"subagent_type":"Explore","prompt":"Look around","run_in_background":true}}'
        update '{"sessionUpdate":"tool_call_update","toolCallId":"agent1","_meta":{"claudeCode":{"toolResponse":{"status":"async_launched","isAsync":true}}}}'
        update '{"sessionUpdate":"tool_call_update","toolCallId":"agent1","status":"completed"}'
        update '{"sessionUpdate":"tool_call","toolCallId":"t1","title":"Run make watch","kind":"execute","status":"in_progress"}'
        update '{"sessionUpdate":"async_task_spawned","asyncTaskId":"bg1","name":"make watch","taskType":"local_bash","description":"Run make watch","showInTranscript":true,"canStop":true,"toolCallId":"t1"}'
        update '{"sessionUpdate":"tool_call_update","toolCallId":"t1","status":"completed","_meta":{"jetbrains":{"air":{"version":1,"asyncTasks":{"backgrounded":true}}}}}'
        if [ "$2" = ended ]; then
            update '{"sessionUpdate":"async_task_spawned","asyncTaskId":"turn1","name":"Claude is working","taskType":"turn","description":"","showInTranscript":false,"canStop":true}'
            update '{"sessionUpdate":"async_task_state_update","asyncTaskId":"turn1","state":"completed"}'
        fi
        update '{"sessionUpdate":"tool_call","toolCallId":"t2","title":"Run cargo test","kind":"execute","status":"in_progress"}'
        ;;
    *'"session/cancel"'*)
        update '{"sessionUpdate":"tool_call_update","toolCallId":"t2","status":"failed"}'
        printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"cancelled"}}\n' "$prompt"
        ;;
    *'"_session/steering"'*)
        update '{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"use the other port"}}'
        reply '{"outcome":"injected"}'
        ;;
    *'"_session/async_task/stop"'*)
        reply '{"stopped":true}'
        update '{"sessionUpdate":"async_task_state_update","asyncTaskId":"bg1","state":"stopped"}'
        ;;
    *) reply '{}' ;;
    esac
done
"#;

#[cfg(unix)]
async fn with_a_background_subagent(dir: &Path, ended: bool) -> (Session, PathBuf) {
    let log = dir.join("sent.jsonl");
    let mut kind = script("claude", BACKGROUND_SUBAGENT_ADAPTER, &log);
    kind.args
        .push(if ended { "ended" } else { "runs" }.to_owned());
    let session = Session::start(start_on(&kind, dir, None)).unwrap();
    until(&session, |live| {
        live.info.state.summary == "Run cargo test" || live.status() == Status::Ended
    })
    .await;
    let state = session.info().state;
    assert_eq!(state.status, Status::Working);
    assert_eq!(state.subagents, u32::from(!ended), "the launched subagent");
    assert_eq!(state.tasks.len(), 1, "the command runs");
    (session, log)
}

#[cfg(unix)]
fn methods(log: &Path) -> Vec<String> {
    sent(log)
        .iter()
        .filter_map(|line| line["method"].as_str().map(str::to_owned))
        .collect()
}

/// A steer goes to the main agent at once, while its subagent, a background
/// command and a foreground command all still run (#566).
#[cfg(unix)]
#[tokio::test]
async fn a_steer_reaches_the_main_agent_while_subagents_and_commands_run() {
    let temp = tempfile::tempdir().unwrap();
    let (session, log) = with_a_background_subagent(temp.path(), false).await;
    session
        .order(Command::Steer("use the other port".to_owned().into()))
        .await
        .unwrap();
    until(&session, |live| live.info.state.queued == 0).await;

    assert!(methods(&log).contains(&"_session/steering".to_owned()));
    assert!(!methods(&log).contains(&"session/cancel".to_owned()));
    let state = session.info().state;
    assert_eq!(state.status, Status::Working);
    assert_eq!(state.subagents, 1);
    session.close().await;
}

/// Stop ends the main turn only: the subagent and the background command
/// keep running and the session keeps showing work (#565).
#[cfg(unix)]
#[tokio::test]
async fn stop_ends_the_main_turn_and_leaves_subagents_running() {
    let temp = tempfile::tempdir().unwrap();
    let (session, log) = with_a_background_subagent(temp.path(), false).await;
    session
        .order(Command::Cancel { by_user: true })
        .await
        .unwrap();
    until(&session, |live| live.info.turns == 1).await;

    let info = session.info();
    assert_eq!(info.state.status, Status::Idle);
    assert_eq!(info.state.subagents, 1, "the subagent still runs");
    assert!(info.state.runs(), "the session still shows work");
    assert_eq!(info.state.tasks[0].state, TaskState::Running);
    let methods = methods(&log);
    assert_eq!(methods.iter().filter(|m| *m == "session/cancel").count(), 1);
    assert!(!methods.contains(&"_session/async_task/stop".to_owned()));
    session.close().await;
}

/// Stop all while a subagent runs stops the agent itself, since Claude Code
/// stops a subagent only with it; the next message resumes the session.
#[cfg(unix)]
#[tokio::test]
async fn stop_all_with_a_running_subagent_stops_the_agent() {
    let temp = tempfile::tempdir().unwrap();
    let (session, _log) = with_a_background_subagent(temp.path(), false).await;
    session
        .order(Command::StopAll { by_user: true })
        .await
        .unwrap();
    until(&session, |live| live.status() == Status::Ended).await;

    let state = session.info().state;
    assert_eq!(state.subagents, 0);
    assert!(state.tasks.iter().all(|task| task.state.ended()));
    let error = state.error.unwrap_or_default();
    assert!(error.contains("stopped with its subagents"), "{error}");
}

/// Stop all with no subagent left stops the turn and each background
/// command, and the session stays.
#[cfg(unix)]
#[tokio::test]
async fn stop_all_without_subagents_stops_the_turn_and_background_commands() {
    let temp = tempfile::tempdir().unwrap();
    let (session, log) = with_a_background_subagent(temp.path(), true).await;
    session
        .order(Command::StopAll { by_user: true })
        .await
        .unwrap();
    until(&session, |live| {
        live.info.turns == 1 && live.info.state.tasks.iter().all(|task| task.state.ended())
    })
    .await;

    let methods = methods(&log);
    assert!(methods.contains(&"session/cancel".to_owned()));
    assert!(methods.contains(&"_session/async_task/stop".to_owned()));
    assert_eq!(session.info().state.status, Status::Idle);
    session.close().await;
}

#[cfg(unix)]
#[tokio::test]
async fn an_image_sent_during_resume_waits_for_image_capabilities() {
    for images in [true, false] {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();
        let fifo = dir.join("initialized");
        assert!(
            std::process::Command::new("mkfifo")
                .arg(&fifo)
                .status()
                .unwrap()
                .success()
        );
        let adapter = IMAGES_ADAPTER
            .replacen(
                "read -r line; log \"$1\"",
                "read -r line; log \"$1\"; read -r ready < \"$3\"",
                1,
            )
            .replace("reply '{\"sessionId\":\"s\"}'", "reply '{}'");
        let adapter = adapter.replace(
            r#"agentCapabilities\":{"#,
            r#"agentCapabilities\":{\"loadSession\":true,"#,
        );
        let mut kind = script("codex", &adapter, &dir.join("sent.jsonl"));
        kind.args
            .extend([images.to_string(), fifo.display().to_string()]);
        let session = Session::start(Start {
            prompt: None,
            resume: Some("s".into()),
            ..start_on(&kind, dir, None)
        })
        .unwrap();
        session
            .order(Command::Steer(shot()))
            .await
            .expect("accepted while initializing");
        assert_eq!(session.info().state.status, Status::Starting);
        tokio::task::spawn_blocking(move || std::fs::write(fifo, "go\n"))
            .await
            .unwrap()
            .unwrap();
        until(&session, |live| {
            live.info.turns == 1 || live.status() == Status::Ended
        })
        .await;
        let sent: Vec<Value> = std::fs::read_to_string(dir.join("sent.jsonl"))
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let prompt = sent
            .iter()
            .find(|line| line["method"] == "session/prompt")
            .expect("prompt delivered");
        let blocks = prompt["params"]["prompt"].as_array().unwrap();
        assert_eq!(blocks.iter().any(|block| block["type"] == "image"), images);
        assert_eq!(session.info().turns, 1);
        session.close().await;
    }
}

#[cfg(unix)]
#[path = "agents_session_v2.rs"]
mod v2;

#[cfg(unix)]
#[tokio::test]
async fn codex_sets_fast_before_prompt_and_can_turn_it_off() {
    let temp = tempfile::tempdir().unwrap();
    let log = temp.path().join("sent.jsonl");
    let adapter = r#"
read -r line; log "$1"
reply '{"protocolVersion":1,"agentCapabilities":{}}'
read -r line; log "$1"
reply '{"sessionId":"s","configOptions":[{"id":"fast-mode","name":"Fast mode","type":"select","currentValue":"off","options":[{"value":"off","name":"Off"},{"value":"on","name":"On"}]}]}'
read -r line; log "$1"; reply '{}'
read -r line; log "$1"; reply '{"stopReason":"end_turn"}'
read -r line; log "$1"; reply '{}'
cat > /dev/null
"#;
    let kind = script("codex", adapter, &log);
    let session = Session::start(Start {
        fast: Some(true),
        ..start_on(&kind, temp.path(), None)
    })
    .unwrap();
    settle(&session).await;
    assert_eq!(session.info().turns, 1);
    session
        .order(Command::Configure {
            model: None,
            effort: None,
            fast: Some(false),
        })
        .await
        .unwrap();
    until(&session, |live| live.fast == Some(false)).await;
    let sent = sent(&log);
    assert_eq!(sent[2]["params"]["configId"], "fast-mode");
    assert_eq!(sent[2]["params"]["value"], "on");
    assert_eq!(sent[3]["method"], "session/prompt");
    assert_eq!(sent[4]["params"]["value"], "off");
    session.close().await;
}

#[cfg(unix)]
#[tokio::test]
async fn codex_discovers_fast_mode_after_selecting_a_model() {
    let temp = tempfile::tempdir().unwrap();
    let log = temp.path().join("sent.jsonl");
    let adapter = r#"
read -r line; log "$1"; reply '{"protocolVersion":1,"agentCapabilities":{}}'
read -r line; log "$1"
reply '{"sessionId":"s","configOptions":[{"id":"model","name":"Model","category":"model","type":"select","currentValue":"model-a","options":[{"value":"model-a","name":"A"},{"value":"model-b","name":"B"}]}]}'
read -r line; log "$1"
reply '{"configOptions":[{"id":"fast-mode","name":"Fast mode","type":"select","currentValue":"off","options":[{"value":"off","name":"Off"},{"value":"on","name":"On"}]}]}'
read -r line; log "$1"; reply '{}'
read -r line; log "$1"; reply '{"stopReason":"end_turn"}'
cat > /dev/null
"#;
    let kind = script("codex", adapter, &log);
    let session = Session::start(Start {
        fast: Some(true),
        ..start_on(&kind, temp.path(), Some("model-b"))
    })
    .unwrap();
    settle(&session).await;
    assert_eq!(session.info().turns, 1);
    let sent = sent(&log);
    assert_eq!(sent[2]["params"]["value"], "model-b");
    assert_eq!(sent[3]["params"]["configId"], "fast-mode");
    assert_eq!(sent[3]["params"]["value"], "on");
    assert_eq!(sent[4]["method"], "session/prompt");
    session.close().await;
}
