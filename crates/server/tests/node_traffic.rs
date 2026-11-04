//! A real `beep-node` reports its traffic, the server keeps lifetime totals
//! per key, and a node's restart neither loses nor doubles them.

use async_trait::async_trait;
use beep_node::link::Offers;
use beep_server::{
    app::{App, ServerTools},
    client::Node,
    models::ModelChoices,
    nodes,
    protocol::{DeviceKind, ListMachineTraffic, node::traffic::TrafficReport},
    push::Pusher,
    store::{NewKey, TestDb},
    transport,
};
use flashmind::types::{
    AgentLlmConfig, CompletionRequest, CompletionStream, LlmProvider, Provider,
};
use iroh::{Endpoint, SecretKey, endpoint::presets, protocol::Router};
use std::{collections::BTreeMap, sync::Arc, time::Duration};
use tokio::time::timeout;

const LINK_TX: &str = "alpn.beep-node/1.tx";

/// These tests never run a model turn.
struct NoModel;

#[async_trait]
impl LlmProvider for NoModel {
    fn name(&self) -> &str {
        "none"
    }
    fn provider(&self) -> Provider {
        Provider::OpenAi
    }
    fn complete(&self, _request: CompletionRequest) -> CompletionStream {
        unreachable!("no model calls")
    }
}

async fn endpoint(key: SecretKey) -> Endpoint {
    Endpoint::builder(presets::Minimal)
        .secret_key(key)
        .bind()
        .await
        .expect("a test endpoint binds")
}

fn report(started_at_ms: u64, bytes: u64) -> TrafficReport {
    TrafficReport {
        started_at_ms,
        counters: BTreeMap::from([("alpn.beep-sync/1.tx".to_owned(), bytes)]),
    }
}

#[tokio::test]
async fn reports_are_stored_and_restarts_keep_totals() {
    let db = TestDb::new().await.unwrap();
    let store = db.store().await.unwrap();
    let model = "openai:test".parse().unwrap();
    let models = ModelChoices::resolve(&NoModel, model, Vec::new()).await;
    let app = App::new(
        store.clone(),
        Arc::new(NoModel),
        AgentLlmConfig::new("openai:test".parse().unwrap()),
        ServerTools::default(),
        models,
    );
    let server_endpoint = endpoint(SecretKey::generate()).await;
    let pusher = Pusher::with_retry_waits(
        server_endpoint.clone(),
        Arc::new(store),
        None,
        vec![Duration::from_millis(200)],
    );
    let server_id = server_endpoint.id();
    let router = Router::builder(server_endpoint)
        .accept(
            nodes::NODE_ALPN,
            nodes::NodeHandler::new(app.clone(), server_id),
        )
        .accept(
            transport::ALPN,
            transport::BeepHandler::new(app.clone(), pusher),
        )
        .spawn();
    let server = router.endpoint().addr();

    let alice = SecretKey::generate();
    let user = app
        .sign_in(&alice.public())
        .await
        .unwrap()
        .expect("a new key is never revoked")
        .user;
    let node_key = SecretKey::generate();
    user.link_key(NewKey {
        endpoint: node_key.public(),
        name: "build box",
        kind: DeviceKind::Desktop {},
    })
    .await
    .unwrap();

    let phone = Node::bind(alice, server.clone(), Arc::new(|_| true))
        .await
        .expect("a phone binds");
    let app_client = phone.connect().await.expect("a phone connects");

    let node = endpoint(node_key.clone()).await;
    let served = tokio::spawn(async move {
        beep_node::link::connect(&node, server, "build box", &Offers::default()).await
    });

    // The first report follows the hello; each read waits on the server.
    let listed = timeout(Duration::from_secs(20), async {
        loop {
            let listed = app_client.call(&ListMachineTraffic {}).await.unwrap();
            if !listed.is_empty() {
                return listed;
            }
        }
    })
    .await
    .expect("the node reported its traffic");
    assert_eq!(listed[0].id, node_key.public().to_string());
    let link = listed[0].counters.iter().find(|c| c.key == LINK_TX);
    assert!(link.expect("the link is counted").bytes > 0);
    served.abort();

    let total = async || -> u64 {
        app_client
            .call(&ListMachineTraffic {})
            .await
            .unwrap()
            .iter()
            .flat_map(|machine| &machine.counters)
            .find(|counter| counter.key == "alpn.beep-sync/1.tx")
            .map_or(0, |counter| counter.bytes)
    };
    let save = async |report: TrafficReport| {
        user.save_node_traffic(&node_key.public(), &report)
            .await
            .unwrap();
    };

    save(report(1000, 100)).await;
    assert_eq!(total().await, 100);
    // The same process: only what it has over its last report counts.
    save(report(1000, 150)).await;
    save(report(1000, 150)).await;
    assert_eq!(total().await, 150);
    // A restart: its counters start over and add on top.
    save(report(2000, 30)).await;
    assert_eq!(total().await, 180);
    save(report(2000, 50)).await;
    assert_eq!(total().await, 200);
    // A late report from the process before is ignored.
    save(report(1000, 170)).await;
    assert_eq!(total().await, 200);
}
