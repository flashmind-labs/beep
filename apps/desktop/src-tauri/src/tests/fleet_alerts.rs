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

#[test]
fn a_stream_that_is_accepted_and_closed_keeps_backing_off() {
    let secs = Duration::from_secs;
    assert!(!held(false, secs(1)));
    assert!(!held(false, HELD - secs(1)));
    assert!(held(true, secs(0)));
    assert!(held(false, HELD));
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
                Direct::Notice { notice, .. } => return *notice,
                Direct::Connected
                | Direct::Run(_)
                | Direct::Presence { .. }
                | Direct::Phone { .. } => {}
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
        bridge: None,
    };
    let endpoint = bind().await;
    let offers =
        Offers::local(&[agent], Provision::new(dir.path())).bound(&endpoint, dir.path(), "Studio");
    let fleet = Arc::new(Fleet::new(endpoint, dir.path(), "Studio", offers).await);
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
        fast: None,
        checkout: None,
        images: Vec::new(),
        assistant: false,
        continues: None,
    })
    .await
    .expect("the start is sent");
    let asked = heard(&mut heard_from).await;
    assert_eq!(asked.title, "go");
    assert!(asked.body.contains("Which file?"), "{}", asked.body);
    let key = asked.session_id.parse::<AgentNoticeKey>().expect("a key");
    assert_eq!(key.machine_id, fleet.id().to_string());
    assert!(key.question.is_some());

    answer(&fleet, &key, "choice-0")
        .await
        .expect("the answer is taken");
    let done = heard(&mut heard_from).await;
    assert_eq!(done.title, "go");
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

#[tokio::test]
async fn an_offline_machine_is_not_asked_again_until_it_is_back() {
    let mac = crate::fleet::tests::start("Studio").await;
    timeout(LIMIT, until_online(&mac.fleet, mac.fleet.id()))
        .await
        .expect("this Mac is online at once");

    let away = SecretKey::generate().public();
    let waited = timeout(Duration::from_millis(200), until_online(&mac.fleet, away)).await;
    assert!(
        waited.is_err(),
        "a machine that is not online is waited for"
    );
}

fn phone(id: &str) -> Device {
    Device {
        id: DeviceId(id.to_owned()),
        name: id.to_owned(),
        platform: beep_proto::node::device::Platform::Android,
        state: beep_proto::node::device::DeviceState::On,
        os: None,
        phone: true,
    }
}

#[test]
fn only_a_phone_that_comes_after_the_first_list_is_new() {
    let mut known = None;
    assert!(new_phones(&mut known, vec![phone("a")]).is_empty());
    let fresh = new_phones(&mut known, vec![phone("a"), phone("b")]);
    assert_eq!(fresh, [phone("b")]);
    assert!(new_phones(&mut known, vec![phone("b")]).is_empty());
    assert_eq!(
        new_phones(&mut known, vec![phone("a"), phone("b")]),
        [phone("a")]
    );
}
