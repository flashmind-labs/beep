use super::*;
use crate::peer_stream::{forward, from_desktop};
use beep_mesh::Key;
use beep_mesh::Mesh;
use beep_proto::SessionId;
use beep_proto::node::tools::Tool;
use futures::future::BoxFuture;
use iroh::{Endpoint, SecretKey, endpoint::presets};
use std::time::Duration;

/// A desktop that answers with the arguments it got.
struct Echo;

impl ProjectTools for Echo {
    fn call(&self, args: String) -> BoxFuture<'_, Reply> {
        Box::pin(async move {
            Reply::Done {
                text: format!("got {args}"),
                failed: false,
            }
        })
    }
}

fn request() -> Request {
    Request {
        session: SessionId::from("a3f9c1").to_owned(),
        tool: Tool::Projects,
        args: r#"{"action":"hosts"}"#.to_owned(),
        approved: Default::default(),
    }
}

#[tokio::test]
async fn a_desktop_answers_for_its_own_sessions_and_for_a_node() {
    let dir = tempfile::tempdir().unwrap();
    let mut node = Node::test(dir.path()).await;
    node.project_tools = Some(Arc::new(Echo));
    let expected = Reply::Done {
        text: r#"got {"action":"hosts"}"#.to_owned(),
        failed: false,
    };

    assert_eq!(call(&node, &request()).await, expected);
    assert_eq!(
        answer(node.project_tools.as_ref(), &request()).await,
        expected
    );
}

#[tokio::test]
async fn a_node_without_a_mesh_or_a_desktop_says_where_the_tool_runs() {
    let Reply::Done { text, failed } = call(
        &Node::test(tempfile::tempdir().unwrap().path()).await,
        &request(),
    )
    .await
    else {
        panic!("not done");
    };
    assert!(
        failed && text.contains("The project tools run in the Beep desktop"),
        "{text}"
    );

    let Reply::Done { text, failed } = answer(None, &request()).await else {
        panic!("not done");
    };
    assert!(failed && text.contains("only the Beep desktop"), "{text}");
}

#[tokio::test]
async fn forward_skips_observers() {
    let dir = tempfile::tempdir().unwrap();
    let endpoint = Endpoint::builder(presets::Minimal)
        .secret_key(SecretKey::generate())
        .bind()
        .await
        .unwrap();
    // The only device is a desktop marked as an observer, as a phone is.
    let mesh = Mesh::create(dir.path(), endpoint, "phone").await.unwrap();
    mesh.set(Key::Observer(mesh.me().to_string()), Some(Vec::new()))
        .await
        .unwrap();

    let forwarded = tokio::time::timeout(Duration::from_secs(10), forward(&mesh, &request()))
        .await
        .expect("no dial");
    let Reply::Done { text, failed } = forwarded else {
        panic!("not done");
    };
    assert!(
        failed && text.contains("No desktop of the mesh is online"),
        "{text}"
    );
}

#[test]
fn old_desktop_reply_maps_to_update_message() {
    let old = Reply::Done {
        text: "This node does not serve Unknown.".to_owned(),
        failed: true,
    };

    let Reply::Done { text, failed } = from_desktop("mac-box", Ok(old)) else {
        panic!("not done");
    };
    assert!(failed, "{text}");
    assert_eq!(
        text,
        "mac-box: That desktop's Beep is too old for the project tools. Update it."
    );
}
