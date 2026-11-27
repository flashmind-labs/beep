use super::*;
use crate::agents::{
    calls::SERVER_NAME,
    mcp_launch::{Invocation, Launch, SOCKET_ENV, TOKEN_ENV},
};
use agent_client_protocol::v1::schema::McpServer;

#[test]
fn a_token_is_good_for_its_own_session_only() {
    let launch = Launch::new("/usr/local/bin/beep-node".into(), "/srv/node".into());
    let token = launch.token(&SessionId::from("a3f9c1")).to_hex().to_string();
    assert!(launch.verify(&SessionId::from("a3f9c1"), &token));
    assert!(!launch.verify(&SessionId::from("b4e0d2"), &token));
    assert!(!launch.verify(&SessionId::from("a3f9c1"), "not hex"));
    let other = Launch::new("/usr/local/bin/beep-node".into(), "/srv/node".into());
    assert!(!other.verify(&SessionId::from("a3f9c1"), &token), "a new run has a new key");
    let McpServer::Stdio(server) = launch.server(&SessionId::from("a3f9c1")) else {
        panic!("the server is not a command");
    };
    assert_eq!(server.name, SERVER_NAME);
    assert_eq!(
        server.args,
        ["--dir", "/srv/node", "mcp", "--session", "a3f9c1"]
    );
    assert_eq!(server.env[0].name, TOKEN_ENV);
    assert_eq!(server.env[0].value, token);
}

#[test]
fn a_session_connects_to_the_socket_the_node_bound() {
    let user = "a".repeat(64);
    let dir = PathBuf::from(format!(
        "/Users/{user}/Library/Application Support/com.flashmind.beep.desktop/node"
    ));
    let launch = Launch::new("/srv/Beep.app/Contents/MacOS/beep".into(), dir.clone());
    let McpServer::Stdio(server) = launch.server(&SessionId::from("a3f9c1")) else {
        panic!("the server is not a command");
    };
    let named = server
        .env
        .iter()
        .find(|var| var.name == SOCKET_ENV)
        .expect("the socket is named");
    assert_eq!(Path::new(&named.value), launch.socket);
    assert!(named.value.len() < 104, "{}", named.value);
    assert_eq!(
        connect_to(Some(named.value.clone().into()), &dir),
        launch.socket
    );
    // A session started before the node named its socket.
    assert_eq!(connect_to(None, &dir), launch.socket);
    assert_eq!(
        connect_to(None, Path::new("/srv/node")),
        Path::new("/srv/node/mcp.sock")
    );
}

#[test]
fn a_sessions_arguments_read_back_as_its_invocation() {
    let launch = Launch::new(
        "/srv/Beep.app/Contents/MacOS/beep".into(),
        "/srv/node".into(),
    );
    let McpServer::Stdio(server) = launch.server(&SessionId::from("a3f9c1")) else {
        panic!("the server is not a command");
    };
    assert_eq!(
        Invocation::parse(&server.args),
        Some(Invocation {
            dir: "/srv/node".into(),
            session: "a3f9c1".into(),
        })
    );
}
