//! A removed project's webhooks, removed by a real `beep-node` over iroh at once or once one connects.

use crate::{
    account::user::User,
    memory::watchers::testing::{Harness, Scripted},
    nodes,
    projects::imports::remove_project,
    protocol::{
        DeviceKind,
        node::{
            LinkRx, LinkTx,
            webhook::{PROTOCOL, Watch, WatchReply},
        },
    },
    store::{NewKey, NewWebhook, OrphanWebhooks},
};
use beep_node::link::{Offers, Protocol, Session};
use beep_projects::model::RepoName;
use iroh::{Endpoint, SecretKey, endpoint::presets, protocol::Router};
use std::{sync::Arc, time::Duration};
use tokio::{sync::mpsc, time::timeout};
use uuid::Uuid;

const LIMIT: Duration = Duration::from_secs(30);
const REPO: &str = "acme/widget";

/// A node's `webhook/1` that removes what it is asked to and passes each watch on.
struct FakeWebhooks(mpsc::UnboundedSender<Watch>);

impl Protocol for FakeWebhooks {
    fn name(&self) -> &str {
        PROTOCOL
    }

    fn serve(&self, mut tx: LinkTx, mut rx: LinkRx) -> Session {
        let watches = self.0.clone();
        Box::pin(async move {
            let Some(watch) = rx.recv::<Watch>().await? else {
                return Ok(());
            };
            for &hook in &watch.stale {
                tx.send(&WatchReply::Removed { hook }).await?;
            }
            watches.send(watch)?;
            Ok(())
        })
    }
}

/// Records the webhook `hook` a node made on the repository.
async fn made(user: &User, hook: u64) {
    let id = Uuid::new_v4();
    user.begin_webhook(&NewWebhook {
        id,
        host: "github",
        repo: REPO,
        endpoint: "node-1",
        secret: b"sealed",
    })
    .await
    .expect("noted");
    user.webhook_made(id, hook).await.expect("noted");
}

/// The project made and removed, with `hook` on its repository.
async fn removed_with(harness: &Harness, hook: u64) {
    let alice = &harness.alice;
    made(alice, hook).await;
    let repo = RepoName::parse(REPO).expect("a repo name");
    alice.ensure_project(&repo).await.expect("made");
    assert!(
        remove_project(&harness.app, alice, REPO)
            .await
            .expect("removed")
    );
}

#[tokio::test]
async fn a_removed_projects_webhooks_go_once_a_node_is_online() {
    let harness = Harness::new(Scripted::default()).await;
    let alice = &harness.alice;

    // No node online: the row stays for later.
    removed_with(&harness, 41).await;
    let kept = OrphanWebhooks {
        host: "github".into(),
        repo: REPO.into(),
        hooks: vec![41],
    };
    assert_eq!(alice.orphan_webhooks().await.expect("read"), [kept]);

    // A node connects and removes it.
    let endpoint = Endpoint::builder(presets::Minimal)
        .secret_key(SecretKey::generate())
        .bind()
        .await
        .expect("the server endpoint binds");
    let handler = nodes::NodeHandler::new(harness.app.clone(), endpoint.id());
    let router = Router::builder(endpoint)
        .accept(nodes::NODE_ALPN, handler)
        .spawn();
    let key = SecretKey::generate();
    alice
        .link_key(NewKey {
            endpoint: key.public(),
            name: "build",
            kind: DeviceKind::Desktop {},
        })
        .await
        .expect("the key links");
    let (watched, mut watches) = mpsc::unbounded_channel();
    let offers = Offers {
        agents: Arc::default(),
        extra: vec![Arc::new(FakeWebhooks(watched)) as Arc<dyn Protocol>],
        provision: None,
        tools: Default::default(),
        traffic: Default::default(),
    };
    let node = Endpoint::builder(presets::Minimal)
        .secret_key(key)
        .bind()
        .await
        .expect("the node endpoint binds");
    let addr = router.endpoint().addr();
    let _task =
        tokio::spawn(async move { beep_node::link::connect(&node, addr, "build", &offers).await });
    let watch = timeout(LIMIT, watches.recv())
        .await
        .expect("a watch came")
        .expect("the node runs");
    assert_eq!(watch.stale, [41]);
    assert!(watch.remove_only);
    // The server notes the removal after the node sent it, with no signal
    // to wait on, so this looks until it is gone.
    timeout(LIMIT, async {
        while !alice.orphan_webhooks().await.expect("read").is_empty() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("the row went");

    // With the node online, removing the project removes its webhooks.
    removed_with(&harness, 42).await;
    let watch = watches.try_recv().expect("removed with the project");
    assert_eq!(watch.stale, [42]);
    assert!(alice.orphan_webhooks().await.expect("read").is_empty());
}
