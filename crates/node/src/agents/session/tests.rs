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
    harness::AutoCompact,
};
use beep_proto::{MAX_AGENT_IMAGES, Upload};
use serde_json::Value;

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
    }
}

async fn settle(session: &Session) {
    let mut live = session.live.clone();
    tokio::time::timeout(
        std::time::Duration::from_secs(10),
        live.wait_for(|live| {
            live.info.turns >= 1 || matches!(live.status(), Status::Ended | Status::Asking)
        }),
    )
    .await
    .expect("the session settles")
    .unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn a_takeover_resumes_without_permission_prompts() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();
    let log = dir.join("sent.jsonl");
    let id = "0f3c2a9e-1111-4222-8333-944455556666";
    let session = Session::start(Start {
        prompt: Some("carry on".into()),
        resume: Some(id.into()),
        ..start_on(&fake("claude", &log), dir, None)
    })
    .unwrap();
    settle(&session).await;
    let info = session.info();
    assert_eq!(info.turns, 1, "{:?}", info.state.error);
    assert_eq!(info.last_message, "Picked it up.");
    assert!(info.question.is_none());
    assert_eq!(session.live.borrow().agent_session.as_deref(), Some(id));
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
    let (entries, _) = session.live.borrow().transcript(10);
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
    McpServer::Stdio(agent_client_protocol::schema::v1::McpServerStdio::new(
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
    let brief = format!("Worktree first.\n\n{}", crate::agents::mcp::BRIEF);
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
            "content": [{"type": "text", "text": crate::agents::mcp::BRIEF}],
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
    let brief = format!("Worktree first.\n\n{}", crate::agents::mcp::BRIEF);
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
        prompt: Some("hello".into()),
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
        std::time::Duration::from_secs(10),
        live.wait_for(|live| live.status() == Status::Ended),
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
        prompt: Some("y".repeat(4 * 1024 * 1024)),
        ..start_on(&script("codex", BURSTING_ADAPTER, &log), dir, None)
    })
    .unwrap();
    let mut live = session.live.clone();
    // The timeout only bounds a failure; the turn ending is the signal.
    tokio::time::timeout(
        std::time::Duration::from_secs(60),
        live.wait_for(|live| live.info.turns >= 1 || live.status() == Status::Ended),
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
    tokio::time::timeout(std::time::Duration::from_secs(60), live.wait_for(done))
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

/// Claude Code saving a plot from a shell command and never opening
/// it, then listing the folder, which names an older picture too.
const PLOTTING_ADAPTER: &str = r#"
cat <<'EOF'
UPDATE{"sessionUpdate":"tool_call","toolCallId":"t1","title":"`python -c \"import matplotlib.pyplot as plt; plt.plot([1, 2]); plt.savefig('plot.png')\"`","kind":"execute","status":"pending"}}}
EOF
printf '\211PNG\r\n\032\nplot' > plot.png
echo 'UPDATE{"sessionUpdate":"tool_call_update","toolCallId":"t1","status":"completed"}}}'
echo 'UPDATE{"sessionUpdate":"tool_call","toolCallId":"t2","title":"`ls`","kind":"execute","status":"pending"}}}'
echo 'UPDATE{"sessionUpdate":"tool_call_update","toolCallId":"t2","status":"completed","content":[{"type":"content","content":{"type":"text","text":"old.png\nplot.png"}}]}}}'
echo 'UPDATE{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"Plotted it."}}}}'
reply '{"stopReason":"end_turn"}'
cat > /dev/null
"#;

#[cfg(unix)]
#[tokio::test]
async fn a_plot_a_command_saved_shows_once_the_turn_ends() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();
    let old = std::fs::File::create(dir.join("old.png")).unwrap();
    std::io::Write::write_all(&mut &old, b"\x89PNG\r\n\x1a\nold").unwrap();
    old.set_modified(std::time::SystemTime::now() - Duration::from_secs(3600))
        .unwrap();
    let session = started(dir, &PLOTTING_ADAPTER.replace("UPDATE", UPDATE));
    until(&session, |live| {
        live.info.turns >= 1
            && live
                .transcript(10)
                .0
                .iter()
                .any(|e| e.text.contains("[image:"))
    })
    .await;
    let (entries, _) = session.live.borrow().transcript(10);
    let plot = format!(
        "[image: image-{}]",
        beep_proto::image_key(b"\x89PNG\r\n\x1a\nplot")
    );
    let last = entries.last().unwrap();
    assert_eq!(
        last.text,
        format!("Plotted it.\n{plot}"),
        "only the new plot"
    );
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
    let (entries, _) = session.live.borrow().transcript(10);
    let shown = format!("The button overlaps\n[image: image-{key}]");
    assert!(
        entries.iter().any(|entry| entry.text == shown),
        "{entries:?}"
    );
    let kept = session.live.borrow().picture(&key);
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
        prompt: Some("go".into()),
        resume: None,
        model: model.map(ModelId::new),
        effort: None,
        mode: None,
        parent: None,
        follows: None,
        note: None,
        brief: None,
        mcp: None,
        images: Arc::default(),
        usage: Arc::default(),
        alerts: Arc::default(),
        node: NodeMode::default(),
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
    let sent = sent(&log);
    assert_eq!(sent[1]["method"], "session/new");
    assert_eq!(sent[2]["method"], "session/set_model");
    assert_eq!(sent[2]["params"]["sessionId"], "s");
    assert_eq!(sent[2]["params"]["modelId"], "deep");
    assert_eq!(sent[3]["method"], "session/prompt", "the prompt waits");
    let args = std::fs::read_to_string(dir.join("sent.jsonl.args")).unwrap();
    assert_eq!(args.trim(), "", "no flag on the command line");
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
    // It cannot think at another level: refused, nothing sent.
    let refused = session
        .order(Command::Configure {
            model: None,
            effort: Some(Effort::High),
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
    let kind = script("codex", RESUMED_CODEX_ADAPTER, &log);
    let start = Start {
        resume: Some("0f3c2a9e-1111-4222-8333-944455556666".into()),
        ..start_on(&kind, dir, None)
    };
    let session = Session::start(start).unwrap();
    settle(&session).await;
    session
        .order(Command::Configure {
            model: Some("deep".into()),
            effort: None,
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
async fn a_full_window_is_compacted_before_the_next_prompt() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();
    let log = dir.join("sent.jsonl");
    let kind = script("codex", COMPACTING_ADAPTER, &log);
    let session = Session::start(start_on(&kind, dir, None)).unwrap();
    settle(&session).await;
    assert_eq!(prompts(&log), ["go"], "nothing is compacted mid-turn");
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
    let (entries, _) = session.live.borrow().transcript(100);
    let compacted: Vec<_> = entries
        .iter()
        .filter(|entry| entry.text == COMPACTED_ENTRY)
        .collect();
    assert_eq!(compacted.len(), 1, "{entries:?}");
    assert_eq!(compacted[0].kind, EntryKind::Other);
    assert!(
        !entries.iter().any(|entry| entry.text == COMPACT_PROMPT),
        "the node's /compact is not shown as the user's"
    );
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

/// An adapter whose first turn runs one tool call, which finishes only
/// once the test writes to the fifo in `$2`. It answers the node's
/// `session/cancel` by ending the turn cancelled, and ends each later
/// turn at once. It logs every line it reads.
const STEERING_ADAPTER: &str = r#"
read -r line; log "$1"
reply '{"protocolVersion":1,"agentCapabilities":{"promptCapabilities":{"image":true}}}'
read -r line; log "$1"
reply '{"sessionId":"s"}'
read -r line; log "$1"
prompt=$(id)
echo '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s","update":{"sessionUpdate":"tool_call","toolCallId":"t1","title":"Running tests","status":"in_progress"}}}'
read -r go < "$2"
echo '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s","update":{"sessionUpdate":"tool_call_update","toolCallId":"t1","status":"completed"}}}'
read -r line; log "$1"
printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"cancelled"}}\n' "$prompt"
while read -r line; do log "$1"; reply '{"stopReason":"end_turn"}'; done
"#;

#[cfg(unix)]
#[tokio::test]
async fn a_steer_goes_when_the_tool_call_finishes_ahead_of_the_queue() {
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
    session.order(Command::Cancel).await.unwrap();
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
    let (entries, _) = session.live.borrow().transcript(10);
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
        let (entries, _) = session.live.borrow().transcript(10);
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
    assert_eq!(compaction_env("claude", at(Some(65))).await, "65|||");
    assert_eq!(compaction_env("claude", off).await, "|1||");
    assert_eq!(compaction_env("opencode", off).await, "||1|");
    assert_eq!(compaction_env("opencode", at(Some(65))).await, "|||");
    assert_eq!(compaction_env("codex", off).await, "|||");
    let tokens = Some(AutoCompact {
        enabled: true,
        percent: None,
        tokens: Some(350_000),
    });
    assert_eq!(compaction_env("claude", tokens).await, "|1||");
    assert_eq!(
        compaction_env("codex", tokens).await,
        r#"|||{"model_auto_compact_token_limit":350000}"#
    );
}
