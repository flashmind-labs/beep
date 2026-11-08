use super::*;
use crate::error::DesktopError;
use beep_mesh::transport_config;
use beep_node::{agents::AgentKind, link::Offers, machine::provision::Provision};
use beep_proto::{
    ApiError,
    node::agent::{Request, protocol},
};
use iroh::{Endpoint, SecretKey, endpoint::presets};
use std::collections::BTreeMap;
use tempfile::TempDir;
use tokio::time::timeout;

const LIMIT: Duration = Duration::from_secs(60);

/// An adapter that puts a choice to the user on its first prompt, ends the turn
/// once it is answered, and does nothing more.
const ASKING: &str = r#"
id() { printf '%s\n' "$line" | sed -nE 's/.*"id":("[^"]*"|[0-9]+).*/\1/p'; }
reply() { printf '{"jsonrpc":"2.0","id":%s,"result":%s}\n' "$(id)" "$1"; }
read -r line; reply '{"protocolVersion":1,"agentCapabilities":{}}'
read -r line; reply '{"sessionId":"s"}'
read -r line
prompt=$(id)
echo '{"jsonrpc":"2.0","id":"p1","method":"session/request_permission","params":{"sessionId":"s","toolCall":{"toolCallId":"pi-ui-1","title":"Which file?"},"options":[{"optionId":"choice-0","name":"notes.txt","kind":"allow_once"},{"optionId":"choice-1","name":"other.txt","kind":"allow_once"}]}}'
read -r line
printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$prompt"
cat > /dev/null
"#;

fn key(question: Option<u32>, machine_name: &str) -> AgentNoticeKey {
    AgentNoticeKey {
        machine_id: "ab12".into(),
        agent: "claude".into(),
        session: "a3f9c1".into(),
        question,
        machine_name: machine_name.into(),
    }
}

#[test]
fn a_stream_that_is_accepted_and_closed_keeps_backing_off() {
    let secs = Duration::from_secs;
    assert!(!held(false, secs(1)));
    assert!(!held(false, HELD - secs(1)));
    assert!(held(true, secs(0)));
    assert!(held(false, HELD));
}

#[test]
fn a_notice_seen_on_both_paths_notifies_once() {
    let mut heard = Heard::default();
    let start = Instant::now();
    let after = |secs| start + Duration::from_secs(secs);
    // The server's and the mesh's names for the machine differ.
    assert!(heard.first(&key(Some(3), "Studio"), Via::Server, start));
    assert!(!heard.first(&key(Some(3), "studio.box"), Via::Machine, after(1)));
    // Matched once; the next turn's finish is news again, and a second
    // one the same way is too: two turns ended.
    assert!(heard.first(&key(None, "Studio"), Via::Machine, after(2)));
    assert!(heard.first(&key(None, "Studio"), Via::Machine, after(3)));
    assert!(!heard.first(&key(None, "Studio"), Via::Server, after(4)));
    // Heard one way only, it stays news; a late twin is a new notice.
    assert!(heard.first(&key(Some(4), "Studio"), Via::Server, after(5)));
    assert!(heard.first(&key(Some(4), "Studio"), Via::Machine, after(5) + TWICE));
}

async fn bind() -> Endpoint {
    Endpoint::builder(presets::Minimal)
        .secret_key(SecretKey::generate())
        .transport_config(transport_config())
        .bind()
        .await
        .expect("the endpoint binds")
}

async fn heard(heard: &mut mpsc::Receiver<Direct>) -> Notice {
    timeout(LIMIT, async {
        loop {
            match heard.recv().await.expect("the stream follows") {
                Direct::Notice(notice) => return *notice,
                Direct::Connected | Direct::Run(_) | Direct::Presence { .. } => {}
            }
        }
    })
    .await
    .expect("a notice came in time")
}

#[tokio::test]
async fn an_agents_question_reaches_the_desktop_and_its_answer_returns() {
    let dir = TempDir::new().expect("a temp dir");
    let work = TempDir::new().expect("a temp dir");
    let agent = AgentKind {
        name: "pi".into(),
        program: "sh".into(),
        args: vec!["-c".into(), ASKING.into(), "fake-adapter".into()],
        env: BTreeMap::new(),
        compact: None,
        compact_models: Vec::new(),
        default_model: None,
    };
    let offers = Offers::local(&[agent], Provision::new(dir.path()));
    let fleet = Arc::new(Fleet::new(bind().await, dir.path(), "Studio", offers));
    fleet.run().await;
    let (following, mut heard_from) = start(fleet.clone());
    let following = tokio::spawn(following);

    // A session starts on this Mac and asks, with no server anywhere.
    let client = fleet.client(fleet.id()).await.expect("this Mac is local");
    let (mut tx, rx) = client.open(&protocol("pi")).await.expect("pi opens");
    tx.send(&Request::Start {
        cwd: Some(work.path().display().to_string()),
        prompt: "go".into(),
        model: None,
        effort: None,
        checkout: None,
        images: Vec::new(),
    })
    .await
    .expect("the start is sent");
    let asked = heard(&mut heard_from).await;
    assert_eq!(asked.title, "pi on Studio asks");
    assert!(asked.body.contains("Which file?"), "{}", asked.body);
    let key = asked.session_id.parse::<AgentNoticeKey>().expect("a key");
    assert_eq!(key.machine_id, fleet.id().to_string());
    assert!(key.question.is_some());

    answer(&fleet, &key, "choice-0")
        .await
        .expect("the answer is taken");
    let done = heard(&mut heard_from).await;
    assert_eq!(done.title, "pi on Studio finished");
    // Answering it again: the session no longer asks.
    let again = answer(&fleet, &key, "choice-0").await;
    assert!(
        matches!(again, Err(DesktopError::Api(ApiError::Conflict { .. }))),
        "{again:?}"
    );
    drop((tx, rx, heard_from));
    timeout(LIMIT, following)
        .await
        .expect("following ends once nobody listens")
        .expect("the task ends cleanly");
    fleet.stop().await;
}
