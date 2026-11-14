use super::*;
use beep_proto::node::tools::Tool;

/// A desktop's plugins that answer with the arguments they got.
struct Echo;

impl PluginTools for Echo {
    fn call(&self, args: String) -> BoxFuture<'_, Reply> {
        Box::pin(async move {
            Reply::Done {
                text: format!("plugins got {args}"),
                failed: false,
            }
        })
    }
}

fn request() -> Request {
    Request {
        session: "a3f9c1".to_owned(),
        tool: Tool::Plugins,
        args: r#"{"action":"list"}"#.to_owned(),
        approved: Default::default(),
    }
}

#[tokio::test]
async fn a_desktop_answers_for_its_own_sessions_and_for_a_node() {
    let offers = Offers {
        plugins: Some(Arc::new(Echo)),
        ..Offers::default()
    };
    let expected = Reply::Done {
        text: r#"plugins got {"action":"list"}"#.to_owned(),
        failed: false,
    };

    assert_eq!(call(&offers, &request()).await, expected);
    assert_eq!(answer(offers.plugins.as_ref(), &request()).await, expected);
}

#[tokio::test]
async fn a_node_without_a_mesh_or_plugins_says_where_plugins_run() {
    let Reply::Done { text, failed } = call(&Offers::default(), &request()).await else {
        panic!("not done");
    };
    assert!(failed && text.contains("Plugins run in the Beep desktop"), "{text}");

    let Reply::Done { text, failed } = answer(None, &request()).await else {
        panic!("not done");
    };
    assert!(failed && text.contains("only the Beep desktop"), "{text}");
}
