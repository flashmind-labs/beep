use super::*;
use beep_mesh::Key;
use iroh::{Endpoint, SecretKey, endpoint::presets};
use std::time::Duration;

/// A desktop that answers with the tool and the arguments it got.
struct Echo;

impl DesktopTools for Echo {
    fn call(&self, tool: Tool, args: String) -> BoxFuture<'_, Reply> {
        Box::pin(async move {
            Reply::Done {
                text: format!("{tool} got {args}"),
                failed: false,
            }
        })
    }
}

fn request(tool: Tool) -> Request {
    Request {
        session: "a3f9c1".to_owned(),
        tool,
        args: r#"{"action":"list"}"#.to_owned(),
        approved: Default::default(),
    }
}

#[tokio::test]
async fn a_desktop_answers_for_its_own_sessions_and_for_a_node() {
    let offers = Offers {
        desktop: Some(Arc::new(Echo)),
        ..Offers::default()
    };

    for tool in [Tool::Plugins, Tool::Projects] {
        let expected = Reply::Done {
            text: format!(r#"{tool} got {{"action":"list"}}"#),
            failed: false,
        };

        assert_eq!(call(&offers, &request(tool)).await, expected);
        assert_eq!(answer(offers.desktop.as_ref(), &request(tool)).await, expected);
    }
}

#[tokio::test]
async fn a_node_without_a_mesh_or_a_desktop_says_where_the_tool_runs() {
    let Reply::Done { text, failed } = call(&Offers::default(), &request(Tool::Plugins)).await
    else {
        panic!("not done");
    };
    assert!(
        failed && text.contains("The plugins tool runs in the Beep desktop"),
        "{text}"
    );

    let Reply::Done { text, failed } = answer(None, &request(Tool::Projects)).await else {
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

    let forwarded = tokio::time::timeout(
        Duration::from_secs(10),
        forward(&mesh, &request(Tool::Projects)),
    )
    .await
    .expect("no dial");
    let Reply::Done { text, failed } = forwarded else {
        panic!("not done");
    };
    assert!(failed && text.contains("No desktop of the mesh is online"), "{text}");
}

#[test]
fn old_desktop_reply_maps_to_update_message() {
    let old = Reply::Done {
        text: "This node does not serve Unknown.".to_owned(),
        failed: true,
    };

    let Reply::Done { text, failed } = from_desktop("mac-box", Tool::Projects, Ok(old)) else {
        panic!("not done");
    };
    assert!(failed, "{text}");
    assert_eq!(
        text,
        "mac-box: That desktop's Beep is too old for the projects tool. Update it."
    );
}
