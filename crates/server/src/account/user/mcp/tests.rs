use super::*;
use crate::apps::mcp::{CALL_TIMEOUT, NoAnswer};
use crate::store::TestDb;
use flashmind::core::CancellationToken;
use flashmind::types::tool::ToolContext;
use serde_json::json;
use tokio::{net::TcpListener, sync::mpsc};

#[tokio::test]
async fn fastmail_accounts_are_listed_picked_and_removed() {
    let db = TestDb::new().await.unwrap();
    let user = User::for_test(db.store().await.unwrap(), "alice");
    assert!(user.app_accounts("fastmail").await.unwrap().is_empty());
    for (name, address) in [
        ("fastmail", "me@example.com"),
        ("fastmail-2", "work@example.com"),
    ] {
        user.save_mcp_credentials(name, json!({"client_id": "c"}))
            .await
            .unwrap();
        user.store
            .set_mcp_address(&user.key, name, Some(address))
            .await
            .unwrap();
    }
    // A sign-in that never finished is not an account.
    user.save_mcp_config("fastmail-3", json!({})).await.unwrap();

    let accounts = user.app_accounts("fastmail").await.unwrap();
    let ids: Vec<&str> = accounts.iter().map(|a| a.id.as_str()).collect();
    assert_eq!(ids, ["fastmail", "fastmail-2"]);
    assert_eq!(accounts[1].address.as_deref(), Some("work@example.com"));
    assert_eq!(
        user.mcp_connected().await.unwrap(),
        HashSet::from(["fastmail".to_owned()])
    );
    assert_eq!(user.fastmail_server(None).await.unwrap(), "fastmail");
    assert_eq!(
        user.fastmail_server(Some("Work@example.com"))
            .await
            .unwrap(),
        "fastmail-2"
    );
    assert!(user.fastmail_server(Some("x@example.com")).await.is_err());

    let error = user
        .remove_app_account("fastmail", "mcp_x")
        .await
        .unwrap_err();
    assert!(matches!(error, ApiError::NotFound {}));
    user.remove_app_account("fastmail", "fastmail")
        .await
        .unwrap();
    // The other account becomes the default.
    assert_eq!(user.fastmail_server(None).await.unwrap(), "fastmail-2");
    assert!(
        user.apps()
            .await
            .unwrap()
            .iter()
            .any(|a| a.slug == "fastmail" && a.connected)
    );

    user.disconnect_app("fastmail").await.unwrap();
    assert!(user.app_accounts("fastmail").await.unwrap().is_empty());
    assert!(user.mcp_servers().await.unwrap().is_empty());
}

#[tokio::test]
async fn added_server_tools_are_picked_one_by_one() {
    let db = TestDb::new().await.unwrap();
    let user = User::for_test(db.store().await.unwrap(), "alice");
    let custom = CustomMcp {
        url: "https://notes.example.com/mcp".to_owned(),
        label: "Notes".to_owned(),
        sign_in: false,
        tools: None,
    };
    assert!(
        user.store
            .add_custom_mcp(&user.key, "mcp_notes", &custom)
            .await
            .unwrap()
    );
    let cached: Vec<Value> = (0..30)
        .map(|i| json!({"name": format!("tool_{i}"), "input_schema": {"type": "object"}}))
        .collect();
    user.save_mcp_config(
        "mcp_notes",
        json!({"name": "mcp_notes", "cached_tools": cached}),
    )
    .await
    .unwrap();

    let list = user.list_mcp_tools("mcp_notes").await.unwrap();
    assert_eq!(list.tools.len(), 30);
    assert_eq!(list.tools.iter().filter(|tool| tool.enabled).count(), 25);
    assert_eq!(user.mcp_tools().await.len(), 25);

    let list = user
        .set_mcp_tool("mcp_notes", "tool_29", true)
        .await
        .unwrap();
    assert!(list.tools[29].enabled);
    assert_eq!(user.mcp_tools().await.len(), 26);
    user.set_mcp_tool("mcp_notes", "tool_0", false)
        .await
        .unwrap();
    let servers = user.list_custom_mcp().await.unwrap();
    assert_eq!(servers[0].tools.len(), 25);
    assert!(!servers[0].tools.contains(&"tool_0".to_owned()));
    assert!(matches!(
        user.set_mcp_tool("mcp_notes", "nope", true).await,
        Err(ApiError::NotFound {})
    ));
}

/// Adds real public servers, one open and one behind OAuth; needs the network.
#[tokio::test]
#[ignore]
async fn added_servers_connect_or_ask_to_sign_in() {
    // The server's endpoint installs it on startup; a test has none.
    let _ = rustls::crypto::ring::default_provider().install_default();
    let db = TestDb::new().await.unwrap();
    let user = User::for_test(db.store().await.unwrap(), "alice");

    let open = user
        .add_custom_mcp("DeepWiki", "https://mcp.deepwiki.com/mcp")
        .await
        .unwrap();
    assert!(open.auth_url.is_none(), "{open:?}");
    assert!(
        open.server.connected && !open.server.tools.is_empty(),
        "{open:?}"
    );
    let names: Vec<String> = user
        .mcp_tools()
        .await
        .iter()
        .map(|tool| tool.name().to_owned())
        .collect();
    assert!(
        names.iter().any(|name| name.starts_with("MCP_DEEPWIKI_")),
        "{names:?}"
    );
    let tools = user.mcp_tools().await;
    let structure = tools
        .iter()
        .find(|tool| tool.name() == "MCP_DEEPWIKI_READ_WIKI_STRUCTURE")
        .expect("DeepWiki lists its wiki structure tool");
    let cancel = CancellationToken::new();
    let args = json!({"repoName": "tokio-rs/tokio"});
    let result = structure
        .execute(ToolContext::new("call", args, None, &cancel))
        .await
        .unwrap();
    assert!(
        result.is_success() && result.output().to_lowercase().contains("tokio"),
        "{result:?}"
    );

    let oauth = user
        .add_custom_mcp("Linear", "https://mcp.linear.app/mcp")
        .await
        .unwrap();
    let auth = oauth.auth_url.expect("Linear asks to sign in");
    assert!(!oauth.server.connected && oauth.server.sign_in);
    assert!(
        auth.contains("redirect_uri=https%3A%2F%2Fbeep.flashmind.com%2Foauth%2Fcallback"),
        "{auth}"
    );

    let refused = user
        .add_custom_mcp("Router", "http://192.168.1.1/mcp")
        .await;
    assert!(refused.is_err());

    assert_eq!(user.list_custom_mcp().await.unwrap().len(), 2);
    user.remove_custom_mcp(&open.server.id).await.unwrap();
    user.remove_custom_mcp(&oauth.server.id).await.unwrap();
    assert!(user.list_custom_mcp().await.unwrap().is_empty());
}

/// A Fastmail that never answers, reporting each connection on the channel.
async fn silent_fastmail() -> (String, mpsc::UnboundedReceiver<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/mcp", listener.local_addr().unwrap());
    let (reached, reaching) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        let mut held = Vec::new();
        while let Ok((stream, _)) = listener.accept().await {
            held.push(stream);
            if reached.send(()).is_err() {
                break;
            }
        }
    });
    (url, reaching)
}

#[tokio::test]
async fn a_silent_fastmail_holds_up_nothing_else() {
    // Another test may have installed a provider first, which is just as good.
    let _ = rustls::crypto::ring::default_provider().install_default();
    let db = TestDb::new().await.unwrap();
    let user = User::for_test(db.store().await.unwrap(), "alice");
    let (url, mut reaching) = silent_fastmail().await;
    user.mcp.point_at(&url);
    user.save_mcp_credentials(FASTMAIL, json!({"client_id": "c"}))
        .await
        .unwrap();
    user.store
        .set_mcp_address(&user.key, FASTMAIL, Some("me@example.com"))
        .await
        .unwrap();
    // The second account's address is not known yet.
    user.save_mcp_credentials("fastmail-2", json!({"client_id": "c"}))
        .await
        .unwrap();

    // The call starts its timeout before anything reaches Fastmail.
    let call = tokio::spawn({
        let user = user.clone();
        async move { user.call_fastmail(None, "search_email", json!({})).await }
    });
    reaching.recv().await.unwrap();
    let stuck = user.mcp_registry().await;

    // What the Fastmail page and a run ask for answers meanwhile.
    let accounts = user.app_accounts(FASTMAIL).await.unwrap();
    assert_eq!(accounts.len(), 2);
    let app = crate::apps::catalog_app(FASTMAIL).unwrap();
    let page = crate::apps::permissions::app_permissions(app, &user.app_rules().await.unwrap());
    assert!(page.actions.iter().any(|a| a.tool == "FASTMAIL_SEND_EMAIL"));
    assert!(
        user.apps()
            .await
            .unwrap()
            .iter()
            .any(|a| a.slug == FASTMAIL && a.connected)
    );
    user.mcp_tools().await;
    assert!(!call.is_finished(), "Fastmail answered after all");

    // The stuck call gives up on its own, and the next one starts afresh.
    tokio::time::pause();
    tokio::time::advance(CALL_TIMEOUT).await;
    let error = call.await.unwrap().unwrap_err();
    tokio::time::resume();
    assert!(error.downcast_ref::<NoAnswer>().is_some(), "{error:#}");
    assert!(!Arc::ptr_eq(&stuck, &user.mcp_registry().await));
    // Disconnecting does not wait on it either.
    user.disconnect_app(FASTMAIL).await.unwrap();
    assert!(user.app_accounts(FASTMAIL).await.unwrap().is_empty());
}
